<script lang="ts">
  import { app } from "../app.svelte";
  import { settings } from "../settings.svelte";
  import { detectPublicIp } from "../masking";

  let detecting = $state(false);
  let detectFailed = $state(false);

  async function detect() {
    detecting = true;
    detectFailed = false;
    const ip = await detectPublicIp();
    if (ip) settings.maskPublicIp = ip;
    else detectFailed = true;
    detecting = false;
  }

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") app.showSettings = false;
  }

  function addRule() {
    settings.maskRules = [...settings.maskRules, { pattern: "", replacement: "***" }];
  }
  function removeRule(i: number) {
    settings.maskRules = settings.maskRules.filter((_, idx) => idx !== i);
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div
  class="overlay"
  role="button"
  tabindex="0"
  onclick={() => (app.showSettings = false)}
  onkeydown={(e) => e.key === "Escape" && (app.showSettings = false)}>
  <div class="modal" role="dialog" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
    <header>
      <h2>Settings</h2>
      <button class="x" onclick={() => (app.showSettings = false)} aria-label="close">✕</button>
    </header>

    <section>
      <h3>Latency thresholds</h3>
      <div class="row">
        <label>Warning above <input type="number" min="1" bind:value={settings.warnMs} /> ms</label>
        <label>Critical above
          <input type="number" min={settings.warnMs + 1} bind:value={settings.critMs} /> ms</label>
      </div>
      <div class="bands">
        <span class="band good">ideal &lt; {settings.warnMs}</span>
        <span class="band warn">warning &lt; {settings.effectiveCritMs}</span>
        <span class="band bad">critical ≥ {settings.effectiveCritMs}</span>
      </div>
    </section>

    <section>
      <h3>Defaults</h3>
      <div class="row">
        <label>Interval
          <select bind:value={settings.defaultIntervalMs}>
            <option value={500}>0.5s</option>
            <option value={1000}>1s</option>
            <option value={2500}>2.5s</option>
            <option value={5000}>5s</option>
          </select>
        </label>
        <label>IP version
          <select bind:value={settings.defaultIpVersion}>
            <option value="v4">IPv4</option>
            <option value="v6">IPv6</option>
          </select>
        </label>
        <label>Theme
          <select bind:value={settings.theme}>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
      </div>
    </section>

    <section>
      <h3>Host masking <span class="hint">(redacts addresses for safe screenshots)</span></h3>
      <label class="check">
        <input type="checkbox" bind:checked={settings.maskEnabled} /> Enable masking
      </label>
      <div class="row">
        <label class="grow">Public IP
          <input type="text" placeholder="e.g. 203.0.113.5" bind:value={settings.maskPublicIp} />
        </label>
        <button class="detect" onclick={detect} disabled={detecting}>
          {detecting ? "…" : detectFailed ? "Failed — retry" : "Detect"}
        </button>
      </div>

      <div class="rules">
        <div class="rules-head">
          <span>Regex rules</span>
          <button class="add" onclick={addRule}>＋ add</button>
        </div>
        {#each settings.maskRules as rule, i (i)}
          <div class="rule">
            <input class="pat" placeholder="pattern" bind:value={rule.pattern} />
            <span class="arrow">→</span>
            <input class="rep" placeholder="replacement" bind:value={rule.replacement} />
            <button class="del" onclick={() => removeRule(i)} aria-label="remove">✕</button>
          </div>
        {/each}
      </div>
    </section>

    {#if app.engine}
      <section>
        <h3>Engine</h3>
        <p class="engine">{app.engine.description}</p>
      </section>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 40px 16px;
    z-index: 100;
    overflow: auto;
  }
  .modal {
    width: 560px;
    max-width: 100%;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: 17px;
  }
  .x {
    background: none;
    border: none;
    color: var(--muted);
    font-size: 16px;
    cursor: pointer;
  }
  section {
    padding: 16px 20px;
    border-bottom: 1px solid var(--border-faint);
  }
  h3 {
    margin: 0 0 12px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
  }
  .hint {
    text-transform: none;
    letter-spacing: 0;
    opacity: 0.7;
  }
  .row {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    align-items: flex-end;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: 13px;
    color: var(--text);
  }
  label.grow {
    flex: 1 1 auto;
  }
  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
  }
  input[type="number"],
  input[type="text"],
  select {
    padding: 7px 10px;
    color: var(--text);
    background: var(--input);
    border: 1px solid var(--border);
    border-radius: 8px;
    font-size: 13px;
  }
  input[type="number"] {
    width: 90px;
  }
  .bands {
    display: flex;
    gap: 8px;
    margin-top: 12px;
    font-size: 12px;
  }
  .band {
    padding: 4px 10px;
    border-radius: 6px;
  }
  .band.good {
    background: color-mix(in srgb, var(--good) 22%, transparent);
    color: var(--good);
  }
  .band.warn {
    background: color-mix(in srgb, var(--warn) 22%, transparent);
    color: var(--warn);
  }
  .band.bad {
    background: color-mix(in srgb, var(--danger) 22%, transparent);
    color: var(--danger);
  }
  .detect {
    padding: 8px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--input);
    color: var(--text);
    cursor: pointer;
  }
  .rules {
    margin-top: 14px;
  }
  .rules-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
    color: var(--muted);
    margin-bottom: 8px;
  }
  .add {
    background: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--accent);
    cursor: pointer;
    padding: 4px 8px;
  }
  .rule {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }
  .pat,
  .rep {
    flex: 1 1 auto;
    font-family: var(--mono);
  }
  .arrow {
    color: var(--muted);
  }
  .del {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
  }
  .del:hover {
    color: var(--danger);
  }
  .engine {
    margin: 0;
    font-size: 13px;
    color: var(--muted);
  }
</style>
