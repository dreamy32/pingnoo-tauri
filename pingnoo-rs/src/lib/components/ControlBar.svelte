<script lang="ts">
  import { store } from "../store.svelte";

  function onSubmit(e: Event) {
    e.preventDefault();
    if (store.running) store.stop();
    else store.start();
  }
</script>

<form class="bar" onsubmit={onSubmit}>
  <div class="brand">
    <span class="logo">◎</span>
    <span class="name">Pingnoo</span>
  </div>

  <input
    class="host"
    type="text"
    placeholder="host or IP (e.g. 1.1.1.1)"
    bind:value={store.target}
    spellcheck="false"
    autocomplete="off"
    disabled={store.running}
  />

  <label class="interval">
    <span>every</span>
    <select bind:value={store.intervalMs} disabled={store.running}>
      <option value={500}>0.5s</option>
      <option value={1000}>1s</option>
      <option value={2500}>2.5s</option>
      <option value={5000}>5s</option>
    </select>
  </label>

  <button class="go" class:running={store.running} type="submit">
    {store.running ? "Stop" : "Start"}
  </button>

  <div class="status">
    {#if store.resolved}
      <span class="pill">{store.resolved}</span>
    {/if}
    {#if store.running}
      <span class="pill live">● live · round {store.rounds}</span>
    {/if}
    {#if store.completed}
      <span class="pill done">route complete</span>
    {/if}
  </div>
</form>

{#if store.engine}
  <div class="engine">engine: {store.engine.description}</div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
    letter-spacing: 0.02em;
  }
  .logo {
    color: var(--accent);
    font-size: 20px;
  }
  .name {
    font-size: 16px;
  }
  .host {
    flex: 1 1 auto;
    min-width: 160px;
    padding: 9px 12px;
    font-size: 14px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
    outline: none;
  }
  .host:focus {
    border-color: var(--accent);
  }
  .interval {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-size: 13px;
  }
  select {
    padding: 8px 10px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .go {
    padding: 9px 22px;
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
  .go:hover {
    filter: brightness(1.08);
  }
  .status {
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
  .engine {
    padding: 6px 16px;
    font-size: 12px;
    color: var(--muted);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
</style>
