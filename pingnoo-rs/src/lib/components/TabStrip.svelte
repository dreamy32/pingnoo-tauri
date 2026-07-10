<script lang="ts">
  import { app } from "../app.svelte";
  import { mask } from "../masking";

  let newHost = $state("");
  let favOpen = $state(false);
  let favWrap: HTMLDivElement | undefined = $state();

  function openNew(e: Event) {
    e.preventDefault();
    const h = newHost.trim();
    if (!h) return;
    app.newSession(h);
    newHost = "";
  }

  function openFav(host: string, ipVersion: "v4" | "v6", intervalMs: number) {
    app.newSession(host, ipVersion, intervalMs);
    favOpen = false;
  }

  function onWindowClick(e: MouseEvent) {
    if (favOpen && favWrap && !favWrap.contains(e.target as Node)) favOpen = false;
  }
  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") favOpen = false;
  }

  function onTabKeydown(e: KeyboardEvent, id: number) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      app.setActive(id);
    }
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={onWindowKeydown} />

<div class="strip">
  <div class="tabs" role="tablist">
    {#each app.sessions as s (s.id)}
      <div
        class="tab"
        class:active={s.id === app.activeId}
        role="tab"
        tabindex="0"
        aria-selected={s.id === app.activeId}
        onclick={() => app.setActive(s.id)}
        onkeydown={(e) => onTabKeydown(e, s.id)}>
        <span class="dot" class:live={s.running} class:err={!!s.error}></span>
        <span class="label">{mask(s.target) || "new"}</span>
        <button
          class="close"
          title="close tab"
          aria-label="close tab"
          onclick={(e) => {
            e.stopPropagation();
            app.closeSession(s.id);
          }}>✕</button>
      </div>
    {/each}
  </div>

  <form class="new" onsubmit={openNew}>
    <input
      type="text"
      placeholder="add target — host or IP"
      bind:value={newHost}
      spellcheck="false"
      autocomplete="off" />
    <button class="add" type="submit" title="add target">＋</button>
  </form>

  <div class="fav-wrap" bind:this={favWrap}>
    <button class="fav-btn" title="favourites" onclick={() => (favOpen = !favOpen)}>★</button>
    {#if favOpen}
      <div class="fav-menu">
        {#if app.favourites.length === 0}
          <div class="fav-empty">no favourites yet — star a target</div>
        {:else}
          {#each app.favourites as f (f.host)}
            <div class="fav-row">
              <button class="fav-open" onclick={() => openFav(f.host, f.ipVersion, f.intervalMs)}>
                {mask(f.name || f.host)}
              </button>
              <button class="fav-del" title="remove" onclick={() => app.removeFavourite(f.host)}>✕</button>
            </div>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .strip {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }
  .tabs {
    display: flex;
    gap: 4px;
    overflow-x: auto;
    flex: 0 1 auto;
    max-width: 62%;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 8px 8px 0 0;
    background: var(--surface);
    color: var(--muted);
    cursor: pointer;
    font-size: 13px;
    white-space: nowrap;
    max-width: 220px;
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
    background: var(--input);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 150px;
    font-family: var(--mono);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
    flex: none;
  }
  .dot.live {
    background: var(--good);
  }
  .dot.err {
    background: var(--danger);
  }
  .close {
    background: none;
    border: none;
    padding: 0 2px;
    cursor: pointer;
    color: var(--muted);
    font-size: 11px;
    opacity: 0.6;
  }
  .close:hover {
    opacity: 1;
    color: var(--danger);
  }
  .new {
    display: flex;
    gap: 4px;
    flex: 1 1 auto;
    min-width: 160px;
  }
  .new input {
    flex: 1 1 auto;
    padding: 7px 10px;
    font-size: 13px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
    outline: none;
  }
  .new input:focus {
    border-color: var(--accent);
  }
  .add {
    width: 34px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--accent);
    color: #fff;
    font-size: 16px;
    cursor: pointer;
  }
  .fav-wrap {
    position: relative;
  }
  .fav-btn {
    width: 34px;
    height: 34px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--input);
    color: var(--warn);
    font-size: 15px;
    cursor: pointer;
  }
  .fav-menu {
    position: absolute;
    right: 0;
    top: 40px;
    z-index: 10;
    min-width: 220px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
    padding: 6px;
  }
  .fav-empty {
    padding: 10px;
    color: var(--muted);
    font-size: 12px;
    font-style: italic;
  }
  .fav-row {
    display: flex;
    align-items: center;
  }
  .fav-open {
    flex: 1 1 auto;
    text-align: left;
    padding: 8px 10px;
    background: none;
    border: none;
    color: var(--text);
    cursor: pointer;
    border-radius: 6px;
    font-family: var(--mono);
    font-size: 13px;
  }
  .fav-open:hover {
    background: var(--hover);
  }
  .fav-del {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 6px;
  }
  .fav-del:hover {
    color: var(--danger);
  }
</style>
