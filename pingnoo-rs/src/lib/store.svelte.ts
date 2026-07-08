// The streaming store — the heart of the "never freezes" design.
//
// The Channel `onmessage` handler is O(1): it only stashes the newest snapshot
// in a plain (non-reactive) variable. A single requestAnimationFrame loop,
// decoupled from message arrival, is the *only* thing that touches reactive
// state and the chart — so a fast producer can never stall paint, and the chart
// history is bounded (no unbounded-growth leak like the legacy app).

import { Channel, invoke } from "@tauri-apps/api/core";
import type { EngineInfo, Hop, IpVersion, TraceUpdate } from "./types";

/** Points kept per hop for the live chart (rolling window). */
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

class TraceStore {
  // ---- reactive view state (read by components) --------------------------
  running = $state(false);
  target = $state("1.1.1.1");
  intervalMs = $state(1000);
  resolved = $state<string | null>(null);
  completed = $state(false);
  error = $state<string | null>(null);
  engine = $state<EngineInfo | null>(null);
  rounds = $state(0);
  hops = $state<HopRow[]>([]);
  /** Bumped whenever the chart buffers change; the chart reacts to this. */
  chartVersion = $state(0);
  /** ttl values the user has hidden on the chart. */
  hidden = $state<Set<number>>(new Set());

  // ---- hot-path buffers (NOT reactive) -----------------------------------
  #latest: TraceUpdate | null = null;
  #renderedSeq = -1;
  #xs: number[] = [];
  #series = new Map<number, number[]>(); // ttl -> y values aligned to #xs
  #sessionId: number | null = null;
  #channel: Channel<TraceUpdate> | null = null;
  #raf = 0;

  async init() {
    try {
      const engines = await invoke<EngineInfo[]>("list_engines");
      this.engine = engines[0] ?? null;
      if (this.engine && !this.engine.available) {
        this.error =
          "The ICMP engine needs raw-socket privileges on this machine. " +
          "On Linux: setcap cap_net_raw+ep on the binary, or run via sudo.";
      }
    } catch (e) {
      this.error = `Could not query engines: ${e}`;
    }
  }

  /** x/y data for the chart, in ttl order, honouring hidden series. */
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
    this.running = true;

    const channel = new Channel<TraceUpdate>();
    // O(1) hot path: just stash the latest snapshot.
    channel.onmessage = (msg) => {
      this.#latest = msg;
    };
    this.#channel = channel;

    try {
      this.#sessionId = await invoke<number>("start_session", {
        args: {
          host: this.target.trim(),
          ipVersion: "v4" as IpVersion,
          intervalMs: this.intervalMs,
          maxHops: 30,
        },
        channel,
      });
      this.error = null;
      this.#loop();
    } catch (e) {
      this.running = false;
      this.error = `${e}`;
    }
  }

  async stop() {
    if (!this.running) return;
    this.running = false;
    cancelAnimationFrame(this.#raf);
    this.#raf = 0;
    const id = this.#sessionId;
    this.#sessionId = null;
    this.#channel = null;
    if (id !== null) {
      try {
        await invoke("stop_session", { id });
      } catch {
        /* ignore */
      }
    }
  }

  #reset() {
    this.#latest = null;
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

  // The single render loop: decoupled from data arrival.
  #loop = () => {
    const u = this.#latest;
    if (u && u.seq !== this.#renderedSeq) {
      this.#renderedSeq = u.seq;
      this.#apply(u);
    }
    if (this.running) {
      this.#raf = requestAnimationFrame(this.#loop);
    }
  };

  #apply(u: TraceUpdate) {
    // 1) reactive table + status (Svelte updates only changed cells).
    this.hops = u.hops.map(toRow);
    this.resolved = u.resolvedAddr;
    this.completed = u.completed;
    this.rounds = u.seq;

    // 2) append one aligned column to the rolling chart buffers.
    const x = u.seq;
    this.#xs.push(x);
    const seen = new Set<number>();
    for (const h of u.hops) {
      seen.add(h.ttl);
      let ys = this.#series.get(h.ttl);
      if (!ys) {
        // backfill NaNs so every series is aligned to #xs.
        ys = new Array(this.#xs.length - 1).fill(NaN);
        this.#series.set(h.ttl, ys);
      }
      ys.push(h.currentMs ?? NaN);
    }
    // hops that vanished this round still need an aligned NaN.
    for (const [ttl, ys] of this.#series) {
      if (!seen.has(ttl)) ys.push(NaN);
    }
    // trim to the rolling window.
    if (this.#xs.length > RING) {
      const drop = this.#xs.length - RING;
      this.#xs.splice(0, drop);
      for (const ys of this.#series.values()) ys.splice(0, drop);
    }
    this.chartVersion++;
  }
}

export const store = new TraceStore();
