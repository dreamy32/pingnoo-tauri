<script lang="ts">
  import { app } from "../app.svelte";
  import { mask } from "../masking";
  import { fmtMs } from "../colors";
  import type { Session } from "../session.svelte";
  import HopTable from "./HopTable.svelte";
  import LatencyChart from "./LatencyChart.svelte";

  let { session }: { session: Session } = $props();

  let copied = $state(false);

  function toggleRun() {
    if (session.running) session.stop();
    else session.start();
  }

  function toggleFav() {
    if (app.isFavourite(session.target)) app.removeFavourite(session.target);
    else
      app.addFavourite({
        host: session.target,
        name: session.target,
        ipVersion: session.ipVersion,
        intervalMs: session.intervalMs,
      });
  }

  async function copyReport() {
    const lines = [
      `Pingnoo trace to ${mask(session.target)}` +
        (session.resolved ? ` (${mask(session.resolved)})` : ""),
      `#\tHost\tLoss%\tSent\tLast\tAvg\tMin\tMax\tJitter`,
      ...session.hops.map((h) =>
        [
          h.ttl,
          mask(h.host ?? h.addr) ?? "*",
          h.lossPct.toFixed(0),
          h.sent,
          fmtMs(h.currentMs),
          fmtMs(h.avgMs),
          fmtMs(h.minMs),
          fmtMs(h.maxMs),
          fmtMs(h.jitterMs),
        ].join("\t"),
      ),
    ];
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* ignore */
    }
  }
</script>

<div class="control">
  {#if session.running}
    <span class="target-static">{mask(session.target)}</span>
  {:else}
    <input class="target" bind:value={session.target} spellcheck="false" autocomplete="off" />
  {/if}

  <label class="opt">
    <span>every</span>
    <select bind:value={session.intervalMs} disabled={session.running}>
      <option value={500}>0.5s</option>
      <option value={1000}>1s</option>
      <option value={2500}>2.5s</option>
      <option value={5000}>5s</option>
    </select>
  </label>

  <div class="seg" class:disabled={session.running}>
    <button class:on={session.ipVersion === "v4"} disabled={session.running}
      onclick={() => (session.ipVersion = "v4")}>IPv4</button>
    <button class:on={session.ipVersion === "v6"} disabled={session.running}
      onclick={() => (session.ipVersion = "v6")}>IPv6</button>
  </div>

  <button class="go" class:running={session.running} onclick={toggleRun}>
    {session.running ? "Stop" : "Start"}
  </button>

  <div class="pills">
    {#if session.resolved}<span class="pill">{mask(session.resolved)}</span>{/if}
    {#if session.running}<span class="pill live">● live · round {session.rounds}</span>{/if}
    {#if session.completed}<span class="pill done">route complete</span>{/if}
  </div>

  <div class="actions">
    <button class="icon" title="favourite" onclick={toggleFav}>
      {app.isFavourite(session.target) ? "★" : "☆"}
    </button>
    <button class="icon" title="copy report" onclick={copyReport}>
      {copied ? "✓" : "⧉"}
    </button>
  </div>
</div>

{#if session.error}
  <div class="banner">
    <strong>⚠</strong>
    <span>{session.error}</span>
    <button onclick={() => (session.error = null)} aria-label="dismiss">✕</button>
  </div>
{/if}

<div class="panes">
  <section class="table-pane"><HopTable {session} /></section>
  <section class="chart-pane">
    <div class="pane-title">
      <span class="pt-label">latency over time</span>
      <span class="pt-hint">(hover for details · click a hop row to toggle · red ticks = loss)</span>
      <label class="pt-window">
        show
        <select bind:value={session.windowSecs}>
          <option value={60}>1 min</option>
          <option value={300}>5 min</option>
          <option value={900}>15 min</option>
          <option value={3600}>1 hour</option>
          <option value={21600}>6 hours</option>
          <option value={43200}>12 hours</option>
        </select>
      </label>
    </div>
    <div class="chart-host"><LatencyChart {session} /></div>
  </section>
</div>

<style>
  .control {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .target,
  .target-static {
    flex: 1 1 auto;
    min-width: 140px;
    font-family: var(--mono);
    font-size: 14px;
  }
  .target {
    padding: 8px 11px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
    outline: none;
  }
  .target:focus {
    border-color: var(--accent);
  }
  .target-static {
    color: var(--text);
    font-weight: 600;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 13px;
  }
  select {
    padding: 7px 9px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .seg {
    display: flex;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }
  .seg.disabled {
    opacity: 0.55;
  }
  .seg button {
    padding: 7px 10px;
    font-size: 12px;
    background: var(--input);
    color: var(--muted);
    border: none;
    cursor: pointer;
  }
  .seg button.on {
    background: var(--accent);
    color: #fff;
  }
  .go {
    padding: 8px 20px;
    font-size: 14px;
    font-weight: 600;
    color: #fff;
    background: var(--accent);
    border: none;
    border-radius: 8px;
    cursor: pointer;
  }
  .go.running {
    background: var(--danger);
  }
  .pills {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .pill {
    font-size: 12px;
    padding: 4px 9px;
    border-radius: 999px;
    background: var(--input);
    color: var(--muted);
    border: 1px solid var(--border);
    font-variant-numeric: tabular-nums;
  }
  .pill.live {
    color: var(--good);
  }
  .pill.done {
    color: var(--accent);
  }
  .actions {
    margin-left: auto;
    display: flex;
    gap: 6px;
  }
  .icon {
    width: 32px;
    height: 32px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--input);
    color: var(--text);
    cursor: pointer;
    font-size: 15px;
  }
  .banner {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 14px;
    background: color-mix(in srgb, var(--danger) 14%, var(--surface));
    border-bottom: 1px solid var(--border);
    color: var(--text);
    font-size: 13px;
  }
  .banner button {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .panes {
    display: grid;
    grid-template-rows: 1.1fr 0.9fr;
    flex: 1 1 auto;
    min-height: 0;
    overflow: hidden;
  }
  .table-pane {
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  .chart-pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }
  .pane-title {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 14px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .pt-hint {
    text-transform: none;
    letter-spacing: 0;
    opacity: 0.7;
  }
  .pt-window {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    text-transform: none;
    letter-spacing: 0;
  }
  .pt-window select {
    padding: 4px 8px;
    font-size: 12px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .chart-host {
    flex: 1 1 auto;
    padding: 4px 12px 12px;
    min-height: 0;
  }
</style>
