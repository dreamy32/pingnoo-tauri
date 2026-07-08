// Online GeoIP lookups (ip-api.com free tier — no key, ~45 req/min). Results are
// cached per IP for the session. A future phase can swap this for an offline
// MaxMind DB behind the same interface.

import type { GeoInfo } from "./types";

const cache = new Map<string, GeoInfo | null>();
const inflight = new Set<string>();

/** Skip private / reserved ranges — they never resolve to a location. */
function isPrivate(ip: string): boolean {
  if (ip.startsWith("10.") || ip.startsWith("192.168.") || ip.startsWith("127.")) return true;
  const m = ip.match(/^172\.(\d+)\./);
  if (m) {
    const n = Number(m[1]);
    if (n >= 16 && n <= 31) return true;
  }
  // TEST-NET and link-local
  if (ip.startsWith("192.0.2.") || ip.startsWith("169.254.")) return true;
  return false;
}

export function cachedGeo(ip: string): GeoInfo | null | undefined {
  return cache.get(ip);
}

/**
 * Ensures a lookup for `ip` has been started; calls `onResult` when it lands
 * (or immediately if cached). No-ops for private ranges and duplicates.
 */
export function ensureGeo(ip: string, onResult: (ip: string, geo: GeoInfo | null) => void) {
  if (cache.has(ip)) {
    onResult(ip, cache.get(ip)!);
    return;
  }
  if (inflight.has(ip)) return;
  if (isPrivate(ip)) {
    cache.set(ip, null);
    onResult(ip, null);
    return;
  }
  inflight.add(ip);
  void (async () => {
    let geo: GeoInfo | null = null;
    try {
      const res = await fetch(
        `https://ip-api.com/json/${ip}?fields=status,country,countryCode,city,isp`,
      );
      if (res.ok) {
        const j = await res.json();
        if (j.status === "success") {
          geo = {
            country: j.country ?? null,
            countryCode: j.countryCode ?? null,
            city: j.city ?? null,
            isp: j.isp ?? null,
          };
        }
      }
    } catch {
      /* offline / rate-limited — leave null */
    }
    cache.set(ip, geo);
    inflight.delete(ip);
    onResult(ip, geo);
  })();
}

/** Country code (e.g. "US") to a flag emoji. */
export function flag(cc: string | null | undefined): string {
  if (!cc || cc.length !== 2) return "";
  const base = 0x1f1e6;
  return String.fromCodePoint(
    base + (cc.charCodeAt(0) - 65),
    base + (cc.charCodeAt(1) - 65),
  );
}
