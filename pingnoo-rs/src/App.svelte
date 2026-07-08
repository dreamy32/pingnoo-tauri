<script lang="ts">
  import { onMount } from "svelte";
  import { app } from "./lib/app.svelte";
  import { settings } from "./lib/settings.svelte";
  import TabStrip from "./lib/components/TabStrip.svelte";
  import SessionView from "./lib/components/SessionView.svelte";
  import SettingsModal from "./lib/components/SettingsModal.svelte";

  // Load persisted settings synchronously, before the persist effect runs, so
  // it never clobbers saved values with defaults.
  settings.load();

  onMount(async () => {
    await app.init();
    if (app.sessions.length === 0) {
      app.newSession("1.1.1.1", settings.defaultIpVersion, settings.defaultIntervalMs, false);
    }
  });

  // Apply + persist theme/settings reactively.
  $effect(() => {
    document.documentElement.dataset.theme = settings.theme;
  });
  $effect(() => {
    settings.serialize(); // touch all fields for dependency tracking
    settings.persist();
  });

  function toggleTheme() {
    settings.theme = settings.theme === "dark" ? "light" : "dark";
  }
</script>

<main>
  <header>
    <div class="brand">
      <span class="logo">◎</span>
      <span class="name">Pingnoo</span>
      {#if app.engine && !app.engine.available}
        <span class="warn-note" title={app.engine.description}>⚠ engine needs privileges</span>
      {/if}
    </div>
    <div class="head-actions">
      <button class="icon" title="settings" onclick={() => (app.showSettings = true)}>⚙</button>
      <button class="icon" title="toggle theme" onclick={toggleTheme}>
        {settings.theme === "dark" ? "☾" : "☀"}
      </button>
    </div>
  </header>

  <TabStrip />

  <div class="body">
    {#if app.active}
      {#key app.active.id}
        <SessionView session={app.active} />
      {/key}
    {:else}
      <div class="empty">
        <div class="empty-inner">
          <span class="big-logo">◎</span>
          <p>Add a target above to start analysing a route.</p>
        </div>
      </div>
    {/if}
  </div>

  {#if app.showSettings}
    <SettingsModal />
  {/if}
</main>

<style>
  main {
    display: grid;
    grid-template-rows: auto auto 1fr;
    height: 100vh;
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 16px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 700;
  }
  .logo {
    color: var(--accent);
    font-size: 20px;
  }
  .name {
    font-size: 16px;
  }
  .warn-note {
    margin-left: 10px;
    font-size: 12px;
    font-weight: 500;
    color: var(--warn);
  }
  .head-actions {
    display: flex;
    gap: 6px;
  }
  .icon {
    width: 34px;
    height: 34px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--input);
    color: var(--text);
    cursor: pointer;
    font-size: 15px;
  }
  .body {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow: hidden;
  }
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1 1 auto;
    color: var(--muted);
  }
  .empty-inner {
    text-align: center;
  }
  .big-logo {
    font-size: 48px;
    color: var(--border);
    display: block;
    margin-bottom: 12px;
  }
</style>
