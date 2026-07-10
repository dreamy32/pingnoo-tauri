// Online GeoIP lookups via ipwho.is (free tier works over HTTPS with no key —
// unlike ip-api.com, whose free tier is HTTP-only and fails from a secure
// webview). Results are cached per IP; transient failures are retried after a
// cooldown instead of being cached as permanent misses. A future phase can swap
// this for an offline MaxMind DB behind the same interface.

import type { GeoInfo } from "./types";

const RETRY_AFTER_MS = 5 * 60 * 1000;

const cache = new Map<string, GeoInfo | null>();
const inflight = new Set<string>();
const failedAt = new Map<string, number>();

/** Skip private / reserved / special-use ranges — they never geolocate, and
 *  sending them to a third party is a pointless privacy leak. */
function isPrivate(ip: string): boolean {
  // IPv6: loopback, link-local, unique-local, multicast, unspecified.
  if (ip.includes(":")) {
    const v6 = ip.toLowerCase();
    return (
      v6 === "::" ||
      v6 === "::1" ||
      v6.startsWith("fe8") ||
      v6.startsWith("fe9") ||
      v6.startsWith("fea") ||
      v6.startsWith("feb") ||
      v6.startsWith("fc") ||
      v6.startsWith("fd") ||
      v6.startsWith("ff")
    );
  }
  const parts = ip.split(".").map(Number);
  if (parts.length !== 4 || parts.some((n) => Number.isNaN(n))) return true;
  const [a, b] = parts;
  if (a === 0 || a === 10 || a === 127) return true; // this-net, private, loopback
  if (a === 100 && b >= 64 && b <= 127) return true; // CGNAT 100.64/10
  if (a === 169 && b === 254) return true; // link-local
  if (a === 172 && b >= 16 && b <= 31) return true; // private 172.16/12
  if (a === 192 && b === 168) return true; // private
  if (a === 192 && parts[1] === 0 && (parts[2] === 0 || parts[2] === 2)) return true; // IETF, TEST-NET-1
  if (a === 198 && (b === 18 || b === 19)) return true; // benchmark 198.18/15
  if (a === 198 && b === 51 && parts[2] === 100) return true; // TEST-NET-2
  if (a === 203 && b === 0 && parts[2] === 113) return true; // TEST-NET-3
  if (a >= 224) return true; // multicast + reserved
  return false;
}

export function cachedGeo(ip: string): GeoInfo | null | undefined {
  return cache.get(ip);
}

/**
 * Ensures a lookup for `ip` has been started; calls `onResult` when it lands
 * (or immediately if cached). No-ops for private ranges, duplicates, and
 * recently-failed lookups (retried after a cooldown).
 */
export function ensureGeo(ip: string, onResult: (ip: string, geo: GeoInfo | null) => void) {
  if (cache.has(ip)) {
    onResult(ip, cache.get(ip)!);
    return;
  }
  if (inflight.has(ip)) return;
  const lastFail = failedAt.get(ip);
  if (lastFail !== undefined && Date.now() - lastFail < RETRY_AFTER_MS) return;
  if (isPrivate(ip)) {
    cache.set(ip, null);
    onResult(ip, null);
    return;
  }
  inflight.add(ip);
  void (async () => {
    try {
      const res = await fetch(`https://ipwho.is/${encodeURIComponent(ip)}`);
      if (!res.ok) throw new Error(`http ${res.status}`);
      const j = await res.json();
      if (j.success === true) {
        const geo: GeoInfo = {
          country: j.country ?? null,
          countryCode: j.country_code ?? null,
          city: j.city ?? null,
          isp: j.connection?.isp ?? null,
        };
        cache.set(ip, geo);
        onResult(ip, geo);
      } else {
        // Definitive "no location" (e.g. reserved range): cache permanently.
        cache.set(ip, null);
        onResult(ip, null);
      }
      failedAt.delete(ip);
    } catch {
      // Transient (offline / rate limit): retry after the cooldown.
      failedAt.set(ip, Date.now());
    } finally {
      inflight.delete(ip);
    }
  })();
}

/** Country code (e.g. "US") to a flag emoji. */
export function flag(cc: string | null | undefined): string {
  if (!cc || cc.length !== 2) return "";
  const base = 0x1f1e6;
  return String.fromCodePoint(base + (cc.charCodeAt(0) - 65), base + (cc.charCodeAt(1) - 65));
}
