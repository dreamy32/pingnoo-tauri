// Host masking — redacts IPs/hostnames before display, for safe screenshots.
// The signature Pingnoo privacy feature (public-IP + regex maskers). Runs in the
// UI for this MVP; a future phase can move it server-side so raw values never
// reach the DOM at all.

import { settings } from "./settings.svelte";

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** Applies the active maskers to a display string. */
export function mask(value: string | null | undefined): string | null {
  if (value == null) return null;
  if (!settings.maskEnabled) return value;

  let out = value;
  const pub = settings.maskPublicIp?.trim();
  if (pub) {
    // Boundary-guarded so 1.2.3.4 does not also blank the middle of 11.2.3.45.
    try {
      const re = new RegExp(`(?<![\\d.])${escapeRegex(pub)}(?![\\d.])`, "g");
      out = out.replace(re, "×.×.×.×");
    } catch {
      out = out.split(pub).join("×.×.×.×");
    }
  }
  for (const rule of settings.maskRules) {
    if (!rule.pattern) continue;
    try {
      // Replacement via callback so "$&"-style patterns in the user's
      // replacement string are inserted literally, never re-expanding the
      // text being masked.
      out = out.replace(new RegExp(rule.pattern, "g"), () => rule.replacement ?? "");
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
    return typeof j.ip === "string" ? j.ip.trim() : null;
  } catch {
    return null;
  }
}
