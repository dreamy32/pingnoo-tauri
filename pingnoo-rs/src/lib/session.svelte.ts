// One trace session = one tab. Encapsulates its own Channel, ring buffers, rAF
// render loop and reactive view state, so multiple targets run concurrently and
// independently (the legacy EditorManager model).
//
// The hot path is unchanged from the single-session prototype: Channel.onmessage
// is O(1) (stash latest), and a decoupled rAF loop is the only thing that
// touches reactive state or the chart — so nothing ever freezes.

import { Channel, invoke } from "@tauri-apps/api/core";
import type { Hop, IpVersion, TraceUpdate } from "./types";

const RING = 600;

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

  #renderedSeq = -1;
  #xs: number[] = [];
  #series = new Map<number, number[]>();
  #sessionId: number | null = null;

  constructor(id: number, target: string, ipVersion: IpVersion, intervalMs: number) {
    this.id = id;
    this.target = target;
    this.ipVersion = ipVersion;
    this.intervalMs = intervalMs;
  }

  chartData(): { xs: number[]; series: { ttl: number; ys: number[] }[] } {
    const series = [...this.#series.entries()]
      .filter(([ttl]) => !this.hidden.has(ttl))
      .sort((a, b) => a[0] - b[0])
      .map(([ttl, ys]) => ({ ttl, ys }));
    return { xs: this.#xs, series };
  }

  toggleHop(ttl: number) {
    const next = new Set(this.hidden);
    if (next.has(ttl)) next.delete(ttl);
    else next.add(ttl);
    this.hidden = next;
    this.chartVersion++;
  }

  async start() {
    if (this.running) return;
    this.#reset();
    this.error = null;
    this.running = true;

    const channel = new Channel<TraceUpdate>();
    // Apply on arrival. The backend coalesces to ~60Hz and the real rate is
    // ~1/s, so this is cheap and — unlike a requestAnimationFrame loop — keeps
    // updating even when the window is backgrounded (rAF pauses when hidden).
    channel.onmessage = (msg) => {
      if (msg.seq !== this.#renderedSeq) {
        this.#renderedSeq = msg.seq;
        this.#apply(msg);
      }
    };

    try {
      this.#sessionId = await invoke<number>("start_session", {
        args: {
          host: this.target.trim(),
          ipVersion: this.ipVersion,
          intervalMs: this.intervalMs,
          maxHops: 30,
        },
        channel,
      });
    } catch (e) {
      this.running = false;
      this.error = `${e}`;
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

    const x = u.seq;
    this.#xs.push(x);
    const seen = new Set<number>();
    for (const h of u.hops) {
      seen.add(h.ttl);
      let ys = this.#series.get(h.ttl);
      if (!ys) {
        ys = new Array(this.#xs.length - 1).fill(NaN);
        this.#series.set(h.ttl, ys);
      }
      ys.push(h.currentMs ?? NaN);
    }
    for (const [ttl, ys] of this.#series) {
      if (!seen.has(ttl)) ys.push(NaN);
    }
    if (this.#xs.length > RING) {
      const drop = this.#xs.length - RING;
      this.#xs.splice(0, drop);
      for (const ys of this.#series.values()) ys.splice(0, drop);
    }
    this.chartVersion++;
  }
}
