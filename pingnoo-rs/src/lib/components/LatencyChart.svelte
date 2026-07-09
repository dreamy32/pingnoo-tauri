<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { hopColor } from "../colors";
  import { settings } from "../settings.svelte";
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();

  let el: HTMLDivElement;
  let tip: HTMLDivElement | null = null;
  let chart: uPlot | null = null;
  let chartKey = "";
  let ro: ResizeObserver | null = null;
  let ttlsRef: number[] = [];

  // Canvas cannot resolve CSS var() strings — resolve them to concrete colors
  // at build time and rebuild when the theme flips.
  function cssColor(name: string): string {
    return (
      getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888888"
    );
  }

  function fmtTime(x: number): string {
    return new Date(x * 1000).toLocaleTimeString();
  }

  /** Hover tooltip: time + each visible hop's latency (or "timeout") at cursor. */
  function tooltipPlugin(): uPlot.Plugin {
    return {
      hooks: {
        init: (u: uPlot) => {
          tip = document.createElement("div");
          tip.className = "pn-tip";
          tip.style.display = "none";
          u.over.appendChild(tip);
          u.over.addEventListener("mouseleave", () => {
            if (tip) tip.style.display = "none";
          });
        },
        setCursor: (u: uPlot) => {
          if (!tip) return;
          const { left, top, idx } = u.cursor;
          if (idx == null || left == null || top == null || left < 0) {
            tip.style.display = "none";
            return;
          }
          const rows: string[] = [];
          for (let si = 1; si < u.series.length; si++) {
            if (!u.series[si].show) continue;
            const ttl = ttlsRef[si - 1];
            const v = u.data[si][idx];
            const val =
              v == null
                ? `<span class="lost">timeout</span>`
                : `${(v as number).toFixed(1)} ms`;
            rows.push(
              `<div class="row"><span class="sw" style="background:${hopColor(ttl)}"></span>` +
                `<span class="h">hop ${ttl}</span><span class="v">${val}</span></div>`,
            );
          }
          tip.innerHTML =
            `<div class="t">${fmtTime(u.data[0][idx] as number)}</div>` + rows.join("");
          tip.style.display = "block";
          // Flip near the edges so the tooltip stays inside the plot.
          const w = u.over.clientWidth;
          const h = u.over.clientHeight;
          const tw = tip.offsetWidth;
          const th = tip.offsetHeight;
          let lx = left + 14;
          if (lx + tw > w) lx = Math.max(0, left - tw - 14);
          let ly = top + 14;
          if (ly + th > h) ly = Math.max(0, top - th - 14);
          tip.style.left = `${lx}px`;
          tip.style.top = `${ly}px`;
        },
      },
    };
  }

  /** Loss markers: a red tick at the bottom for every sample where any visible
   *  hop timed out — losses are visible at a glance instead of silent gaps. */
  function lossPlugin(): uPlot.Plugin {
    return {
      hooks: {
        draw: (u: uPlot) => {
          const ctx = u.ctx;
          const dpr = window.devicePixelRatio || 1;
          const yBot = u.bbox.top + u.bbox.height;
          ctx.save();
          ctx.fillStyle = cssColor("--danger");
          const xs = u.data[0];
          for (let ix = 0; ix < xs.length; ix++) {
            let lost = false;
            for (let si = 1; si < u.series.length; si++) {
              if (u.series[si].show && u.data[si][ix] == null) {
                lost = true;
                break;
              }
            }
            if (lost) {
              const xp = u.valToPos(xs[ix] as number, "x", true);
              ctx.fillRect(xp - dpr, yBot - 7 * dpr, 2 * dpr, 7 * dpr);
            }
          }
          ctx.restore();
        },
      },
    };
  }

  function build(ttls: number[]) {
    chart?.destroy();
    chart = null;
    tip = null;
    ttlsRef = ttls;
    const cMuted = cssColor("--muted");
    const cGrid = cssColor("--grid");

    const series: uPlot.Series[] = [
      {},
      ...ttls.map((ttl) => ({
        label: `hop ${ttl}`,
        stroke: hopColor(ttl),
        width: 1.5,
        points: { show: false },
        spanGaps: false,
      })),
    ];
    const opts: uPlot.Options = {
      width: el.clientWidth || 600,
      height: el.clientHeight || 240,
      legend: { show: false },
      // Hovering near a line focuses it and dims the rest.
      focus: { alpha: 0.25 },
      cursor: {
        // Drag-zoom is disabled on purpose: with live streaming the selection
        // was wiped by the next tick. The window selector owns the x range.
        drag: { x: false, y: false, setScale: false },
        focus: { prox: 24 },
        points: { show: true, size: 6 },
        y: false,
      },
      scales: { x: { time: true } },
      axes: [
        { stroke: cMuted, grid: { stroke: cGrid, width: 1 }, ticks: { stroke: cGrid } },
        {
          stroke: cMuted,
          grid: { stroke: cGrid, width: 1 },
          ticks: { stroke: cGrid },
          size: 56,
          values: (_u, splits) => splits.map((v) => `${v} ms`),
        },
      ],
      series,
      plugins: [tooltipPlugin(), lossPlugin()],
    };
    chart = new uPlot(opts, [[]], el);
  }

  function render() {
    // Page Visibility: skip canvas work while hidden; repaint once on return.
    if (document.hidden) return;
    const { xs, series } = session.chartData(session.windowSecs);
    const ttls = series.map((s) => s.ttl);
    const key = `${ttls.join(",")}|${settings.theme}`;
    if (!chart || key !== chartKey) {
      chartKey = key;
      build(ttls);
    }
    if (!chart) return;
    chart.setData([xs, ...series.map((s) => s.ys)] as uPlot.AlignedData);
  }

  function onVisibility() {
    if (!document.hidden) render();
  }

  onMount(() => {
    build([]);
    ro = new ResizeObserver(() => {
      if (chart) chart.setSize({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    document.addEventListener("visibilitychange", onVisibility);
  });

  onDestroy(() => {
    document.removeEventListener("visibilitychange", onVisibility);
    ro?.disconnect();
    chart?.destroy();
  });

  // Reactive triggers: new data, window change, theme change.
  $effect(() => {
    session.chartVersion;
    session.windowSecs;
    settings.theme;
    if (el) render();
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 100%;
    min-height: 180px;
    position: relative;
  }
  :global(.uplot .u-axis) {
    color: var(--muted);
  }
  :global(.pn-tip) {
    position: absolute;
    z-index: 20;
    pointer-events: none;
    min-width: 150px;
    padding: 8px 10px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.35);
    font-size: 12px;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  :global(.pn-tip .t) {
    color: var(--muted);
    margin-bottom: 6px;
    font-size: 11px;
  }
  :global(.pn-tip .row) {
    display: flex;
    align-items: center;
    gap: 6px;
    line-height: 1.6;
  }
  :global(.pn-tip .sw) {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: none;
  }
  :global(.pn-tip .h) {
    color: var(--muted);
  }
  :global(.pn-tip .v) {
    margin-left: auto;
    font-weight: 600;
  }
  :global(.pn-tip .lost) {
    color: var(--danger);
    font-weight: 700;
  }
</style>
