// Host masking — redacts IPs/hostnames before display, for safe screenshots.
// The signature Pingnoo privacy feature (public-IP + regex maskers). Runs in the
// UI for this MVP; a future phase can move it server-side so raw values never
// reach the DOM at all.

import { settings } from "./settings.svelte";

/** Applies the active maskers to a display string. */
export function mask(value: string | null | undefined): string | null {
  if (value == null) return null;
  if (!settings.maskEnabled) return value;

  let out = value;
  if (settings.maskPublicIp) {
    out = out.split(settings.maskPublicIp).join("×.×.×.×");
  }
  for (const rule of settings.maskRules) {
    if (!rule.pattern) continue;
    try {
      out = out.replace(new RegExp(rule.pattern, "g"), rule.replacement ?? "");
    } catch {
      /* skip invalid regex */
    }
  }
  return out;
}

/** Best-effort public-IP detection for the public-IP masker. */
export async function detectPublicIp(): Promise<string | null> {
  try {
    const res = await fetch("https://api.ipify.org?format=json");
    if (!res.ok) return null;
    const j = await res.json();
    return typeof j.ip === "string" ? j.ip : null;
  } catch {
    return null;
  }
}
