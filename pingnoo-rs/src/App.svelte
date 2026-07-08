<script lang="ts">
  import { onMount } from "svelte";
  import { store } from "./lib/store.svelte";
  import ControlBar from "./lib/components/ControlBar.svelte";
  import HopTable from "./lib/components/HopTable.svelte";
  import LatencyChart from "./lib/components/LatencyChart.svelte";

  let theme = $state<"dark" | "light">("dark");

  function applyTheme() {
    document.documentElement.dataset.theme = theme;
  }
  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
    localStorage.setItem("pingnoo-theme", theme);
    applyTheme();
  }

  onMount(() => {
    const saved = localStorage.getItem("pingnoo-theme") as "dark" | "light" | null;
    if (saved) theme = saved;
    else if (window.matchMedia("(prefers-color-scheme: light)").matches) theme = "light";
    applyTheme();
    store.init();
  });
</script>

<main>
  <header>
    <ControlBar />
    <button class="theme" onclick={toggleTheme} title="toggle theme" aria-label="toggle theme">
      {theme === "dark" ? "☾" : "☀"}
    </button>
  </header>

  {#if store.error}
    <div class="banner">
      <strong>⚠</strong>
      <span>{store.error}</span>
      <button onclick={() => (store.error = null)} aria-label="dismiss">✕</button>
    </div>
  {/if}

  <section class="table-pane">
    <HopTable />
  </section>

  <section class="chart-pane">
    <div class="pane-title">latency over time <span>(click a hop to toggle)</span></div>
    <div class="chart-host"><LatencyChart /></div>
  </section>
</main>

<style>
  main {
    display: grid;
    grid-template-rows: auto auto 1.1fr 0.9fr;
    height: 100vh;
    overflow: hidden;
  }
  header {
    position: relative;
  }
  .theme {
    position: absolute;
    top: 12px;
    right: 14px;
    width: 34px;
    height: 34px;
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
    padding: 10px 16px;
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
    font-size: 14px;
  }
  .table-pane {
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  .chart-pane {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg);
  }
  .pane-title {
    padding: 8px 16px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
  }
  .pane-title span {
    text-transform: none;
    letter-spacing: 0;
    opacity: 0.7;
  }
  .chart-host {
    flex: 1 1 auto;
    padding: 4px 12px 12px;
    min-height: 0;
  }
</style>
