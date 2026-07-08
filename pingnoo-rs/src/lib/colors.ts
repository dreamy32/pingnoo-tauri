// Shared visual encodings: latency thresholds (driven by user settings) and
// per-hop series colours.

import { settings } from "./settings.svelte";

/** CSS class for a latency cell, coloured by the configured thresholds. */
export function latencyClass(ms: number | null | undefined): string {
  if (ms == null) return "lat-none";
  if (ms < settings.warnMs) return "lat-good";
  if (ms < settings.critMs) return "lat-warn";
  return "lat-bad";
}

/** Stable, well-spread colour for a hop's chart series, keyed by ttl. */
export function hopColor(ttl: number): string {
  const hue = (ttl * 47) % 360;
  return `hsl(${hue} 72% 55%)`;
}

/** Format an optional millisecond value for display. */
export function fmtMs(ms: number | null | undefined, digits = 1): string {
  if (ms == null) return "—";
  return ms.toFixed(digits);
}
