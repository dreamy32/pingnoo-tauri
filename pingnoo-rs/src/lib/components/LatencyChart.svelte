<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { hopColor } from "../colors";
  import type { Session } from "../session.svelte";

  let { session }: { session: Session } = $props();

  let el: HTMLDivElement;
  let chart: uPlot | null = null;
  let seriesKey = "";
  let ro: ResizeObserver | null = null;

  function build(ttls: number[]) {
    if (chart) {
      chart.destroy();
      chart = null;
    }
    const series: uPlot.Series[] = [
      { label: "round" },
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
      cursor: { y: false, points: { show: true } },
      scales: { x: { time: false } },
      axes: [
        {
          stroke: "var(--muted)",
          grid: { stroke: "var(--grid)", width: 1 },
          ticks: { stroke: "var(--grid)" },
        },
        {
          stroke: "var(--muted)",
          grid: { stroke: "var(--grid)", width: 1 },
          ticks: { stroke: "var(--grid)" },
          size: 52,
          values: (_u, splits) => splits.map((v) => `${v}ms`),
        },
      ],
      series,
    };
    chart = new uPlot(opts, [[]], el);
  }

  function render() {
    // Page Visibility: skip canvas work while hidden (rAF is throttled there
    // anyway, but this also skips setData); onVisible repaints once on return.
    if (document.hidden) return;
    const { xs, series } = session.chartData();
    const ttls = series.map((s) => s.ttl);
    const key = ttls.join(",");
    if (!chart || key !== seriesKey) {
      seriesKey = key;
      build(ttls);
    }
    if (!chart) return;
    const data: uPlot.AlignedData = [
      xs,
      ...series.map((s) => s.ys.map((v) => (Number.isNaN(v) ? null : v))),
    ];
    chart.setData(data);
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

  // Reactive trigger: the session bumps chartVersion in its rAF loop.
  $effect(() => {
    session.chartVersion;
    if (el) render();
  });
</script>

<div class="chart" bind:this={el}></div>

<style>
  .chart {
    width: 100%;
    height: 100%;
    min-height: 180px;
  }
  :global(.uplot .u-axis) {
    color: var(--muted);
  }
</style>
