// Shared visual encodings: latency thresholds and per-hop series colours.

/** CSS class for a latency cell, coloured by threshold. */
export function latencyClass(ms: number | null | undefined): string {
  if (ms == null) return "lat-none";
  if (ms < 40) return "lat-good";
  if (ms < 100) return "lat-ok";
  if (ms < 200) return "lat-warn";
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
