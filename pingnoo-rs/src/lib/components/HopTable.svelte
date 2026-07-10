<script lang="ts">
  import { app } from "../app.svelte";
  import { mask } from "../masking";
  import { flag } from "../geoip";
  import { settings } from "../settings.svelte";
  import { latencyClass, hopColor, fmtMs } from "../colors";
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();

  // Trigger GeoIP lookups for any hop addresses we haven't seen yet
  // (no-op while masking is enabled — see app.ensureGeo).
  $effect(() => {
    for (const h of session.hops) {
      if (h.addr) app.ensureGeo(h.addr);
    }
  });

  function location(addr: string | null): string {
    // Locality reveals the route just like the address does — masked together.
    if (!addr || settings.maskEnabled) return "";
    const g = app.geo[addr];
    if (!g) return "";
    const parts = [g.city, g.country].filter(Boolean);
    return `${flag(g.countryCode)} ${parts.join(", ")}`.trim();
  }
</script>

<div class="wrap">
  <table>
    <thead>
      <tr>
        <th class="c-ttl">#</th>
        <th class="c-host">Host</th>
        <th class="c-loc">Location</th>
        <th class="num">Loss</th>
        <th class="num">Sent</th>
        <th class="num">Last</th>
        <th class="num">Avg</th>
        <th class="num">Min</th>
        <th class="num">Max</th>
        <th class="num">Jitter</th>
      </tr>
    </thead>
    <tbody>
      {#each session.hops as hop (hop.ttl)}
        {@const hidden = session.hidden.has(hop.ttl)}
        <tr class:hidden onclick={() => session.toggleHop(hop.ttl)} title="click to toggle on chart">
          <td class="c-ttl">
            <span class="swatch" style="background:{hopColor(hop.ttl)}"></span>{hop.ttl}
          </td>
          <td class="c-host">
            {#if hop.addr}
              <span class="addr">{mask(hop.host ?? hop.addr)}</span>
            {:else}
              <span class="waiting">waiting…</span>
            {/if}
          </td>
          <td class="c-loc">{location(hop.addr)}</td>
          <td class="num" class:loss={hop.lossPct > 0}>{hop.lossPct.toFixed(0)}%</td>
          <td class="num dim">{hop.sent}</td>
          <td class="num {latencyClass(hop.currentMs)}">{fmtMs(hop.currentMs)}</td>
          <td class="num {latencyClass(hop.avgMs)}">{fmtMs(hop.avgMs)}</td>
          <td class="num dim">{fmtMs(hop.minMs)}</td>
          <td class="num dim">{fmtMs(hop.maxMs)}</td>
          <td class="num dim">{fmtMs(hop.jitterMs)}</td>
        </tr>
      {:else}
        <tr class="empty">
          <td colspan="10">
            {session.running ? "discovering route…" : "press Start to begin"}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .wrap {
    overflow: auto;
    height: 100%;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    text-align: right;
    padding: 8px 12px;
    font-weight: 600;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--muted);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  th.c-ttl,
  th.c-host,
  th.c-loc {
    text-align: left;
  }
  tbody td {
    padding: 7px 12px;
    border-bottom: 1px solid var(--border-faint);
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  tbody tr {
    cursor: pointer;
  }
  tbody tr:hover td {
    background: var(--hover);
  }
  tr.hidden {
    opacity: 0.4;
  }
  .c-ttl {
    text-align: left;
    color: var(--muted);
  }
  .swatch {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    margin-right: 8px;
    vertical-align: middle;
  }
  .c-host {
    text-align: left;
    font-family: var(--mono);
    color: var(--text);
  }
  .c-loc {
    text-align: left;
    color: var(--muted);
    font-size: 12px;
  }
  .waiting {
    color: var(--muted);
    font-style: italic;
  }
  .dim {
    color: var(--muted);
  }
  .loss {
    color: var(--danger);
    font-weight: 600;
  }
  .empty td {
    text-align: center;
    color: var(--muted);
    padding: 28px;
    font-style: italic;
  }
  .lat-good {
    color: var(--good);
  }
  .lat-warn {
    color: var(--warn);
    font-weight: 600;
  }
  .lat-bad {
    color: var(--danger);
    font-weight: 700;
  }
  .lat-none {
    color: var(--muted);
  }
</style>
