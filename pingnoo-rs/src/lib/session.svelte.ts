// One trace session = one tab. Encapsulates its own Channel, chart buffers and
// reactive view state, so multiple targets run concurrently and independently
// (the legacy EditorManager model).
//
// Updates apply on Channel message arrival: the backend coalesces snapshots to
// at most ~60/s (really ~1/interval), each apply touches only the values that
// changed (Svelte 5 fine-grained reactivity), and — unlike a rAF loop — arrival
// -driven applies keep working while the window is backgrounded.

import { Channel, invoke } from "@tauri-apps/api/core";
import { settings } from "./settings.svelte";
import type { Hop, IpVersion, TraceUpdate } from "./types";

/// Hard cap on chart history per hop (~14 h at a 1 s interval). The visible
/// range is chosen by the per-tab window selector; this only bounds memory.
const MAX_POINTS = 50_000;

export interface HopRow {
  ttl: number;
  addr: string | null;
  host: string | null;
  code: Hop["code"];
  currentMs: number | null;
  avgMs: number | null;
  minMs: number | null;
  maxMs: number | null;
  jitterMs: number | null;
  lossPct: number;
  sent: number;
}

function toRow(h: Hop): HopRow {
  return {
    ttl: h.ttl,
    addr: h.addr,
    host: h.host,
    code: h.code,
    currentMs: h.currentMs,
    avgMs: h.stats.avgMs,
    minMs: h.stats.minMs,
    maxMs: h.stats.maxMs,
    jitterMs: h.stats.jitterMs,
    lossPct: h.stats.lossPct,
    sent: h.stats.sent,
  };
}

export class Session {
  readonly id: number;
  target = $state("");
  ipVersion = $state<IpVersion>("v4");
  intervalMs = $state(1000);

  running = $state(false);
  resolved = $state<string | null>(null);
  completed = $state(false);
  error = $state<string | null>(null);
  rounds = $state(0);
  hops = $state<HopRow[]>([]);
  chartVersion = $state(0);
  hidden = $state<Set<number>>(new Set());
  /// Visible chart window in seconds (the "Viewport duration" selector).
  windowSecs = $state(settings.windowSecs);

  #renderedSeq = -1;
  #xs: number[] = []; // epoch seconds, stamped on arrival
  // null = timeout (loss); undefined = hop did not exist yet (no data) — the
  // chart renders both as gaps but only null earns a loss marker.
  #series = new Map<number, (number | null | undefined)[]>();
  #sessionId: number | null = null;

  constructor(id: number, target: string, ipVersion: IpVersion, intervalMs: number) {
    this.id = id;
    this.target = target;
    this.ipVersion = ipVersion;
    this.intervalMs = intervalMs;
  }

  /** Chart data restricted to the trailing `windowSecs` of history. */
  chartData(windowSecs: number): {
    xs: number[];
    series: { ttl: number; ys: (number | null | undefined)[] }[];
  } {
    const n = this.#xs.length;
    if (n === 0) return { xs: [], series: [] };
    // Binary-search the first sample inside the window.
    const cutoff = this.#xs[n - 1] - windowSecs;
    let lo = 0;
    let hi = n - 1;
    let start = n - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (this.#xs[mid] >= cutoff) {
        start = mid;
        hi = mid - 1;
      } else {
        lo = mid + 1;
      }
    }
    const xs = this.#xs.slice(start);
    const series = [...this.#series.entries()]
      .filter(([ttl]) => !this.hidden.has(ttl))
      .sort((a, b) => a[0] - b[0])
      .map(([ttl, ys]) => ({ ttl, ys: ys.slice(start) }));
    return { xs, series };
  }

  toggleHop(ttl: number) {
    const next = new Set(this.hidden);
    if (next.has(ttl)) next.delete(ttl);
    else next.add(ttl);
    this.hidden = next;
    this.chartVersion++;
  }

  // Monotonic-seq guard: replays after a reattach may arrive out of order
  // relative to live updates — only ever move forward.
  #makeChannel(): Channel<TraceUpdate> {
    const channel = new Channel<TraceUpdate>();
    channel.onmessage = (msg) => {
      if (msg.seq > this.#renderedSeq) {
        this.#renderedSeq = msg.seq;
        this.#apply(msg);
      }
    };
    return channel;
  }

  async start() {
    if (this.running) return;
    this.#reset();
    this.error = null;
    this.running = true;

    try {
      const id = await invoke<number>("start_session", {
        args: {
          host: this.target.trim(),
          ipVersion: this.ipVersion,
          intervalMs: this.intervalMs,
          maxHops: 30,
        },
        channel: this.#makeChannel(),
      });
      this.#sessionId = id;
      // The tab may have been stopped/closed while the invoke was in flight —
      // don't leak a headless backend session.
      if (!this.running) {
        try {
          await invoke("stop_session", { id });
        } catch {
          /* ignore */
        }
      }
    } catch (e) {
      this.running = false;
      this.error = `${e}`;
    }
  }

  /**
   * Binds this tab to an already-running backend session and replays its
   * latest snapshot. Used after a page reload (rebuild tabs from
   * `list_sessions`) and on restore-from-minimize (idempotent re-subscribe
   * after the suspend path detached the channel).
   */
  async attach(backendId: number) {
    this.#sessionId = backendId;
    this.running = true;
    this.error = null;
    try {
      await invoke("attach_session", { id: backendId, channel: this.#makeChannel() });
    } catch (e) {
      this.running = false;
      this.error = `${e}`;
    }
  }

  /** Re-subscribes the stream if this tab is bound to a running session. */
  async reattach() {
    if (this.running && this.#sessionId !== null) {
      await this.attach(this.#sessionId);
    }
  }

  async stop() {
    if (!this.running) return;
    this.running = false;
    const id = this.#sessionId;
    this.#sessionId = null;
    if (id !== null) {
      try {
        await invoke("stop_session", { id });
      } catch {
        /* ignore */
      }
    }
  }

  async dispose() {
    await this.stop();
  }

  #reset() {
    this.#renderedSeq = -1;
    this.#xs = [];
    this.#series.clear();
    this.hops = [];
    this.rounds = 0;
    this.resolved = null;
    this.completed = false;
    this.hidden = new Set();
    this.chartVersion++;
  }

  #apply(u: TraceUpdate) {
    if (u.error) {
      this.error = u.error;
      void this.stop();
      return;
    }
    this.hops = u.hops.map(toRow);
    this.resolved = u.resolvedAddr;
    this.completed = u.completed;
    this.rounds = u.seq;

    // Stamp on arrival: wall-clock x axis. null (not NaN) marks a timeout so
    // uPlot breaks the line and the loss plugin can paint a marker.
    const x = Date.now() / 1000;
    this.#xs.push(x);
    const seen = new Set<number>();
    for (const h of u.hops) {
      seen.add(h.ttl);
      let ys = this.#series.get(h.ttl);
      if (!ys) {
        // Backfill with undefined ("didn't exist yet"), NOT null — a hop that
        // appears mid-trace must not read as retroactive packet loss.
        ys = new Array(this.#xs.length - 1).fill(undefined);
        this.#series.set(h.ttl, ys);
      }
      ys.push(h.currentMs ?? null);
    }
    // Hops the backend no longer reports (pruned phantom hops past the
    // destination) leave the chart entirely, matching the table.
    for (const ttl of [...this.#series.keys()]) {
      if (!seen.has(ttl)) this.#series.delete(ttl);
    }
    if (this.#xs.length > MAX_POINTS) {
      const drop = this.#xs.length - MAX_POINTS;
      this.#xs.splice(0, drop);
      for (const ys of this.#series.values()) ys.splice(0, drop);
    }
    this.chartVersion++;
  }
}
