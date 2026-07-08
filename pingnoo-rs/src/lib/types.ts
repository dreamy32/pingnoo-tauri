// Mirror of the Rust wire contract in `pingnoo-core` (serde camelCase).

export type ResultCode = "ok" | "noReply" | "timeExceeded";
export type IpVersion = "v4" | "v6";

export interface HopStats {
  sent: number;
  received: number;
  lossPct: number;
  lastMs: number | null;
  avgMs: number | null;
  minMs: number | null;
  maxMs: number | null;
  jitterMs: number | null;
}

export interface Hop {
  ttl: number;
  sampleNumber: number;
  addr: string | null;
  host: string | null;
  code: ResultCode;
  currentMs: number | null;
  stats: HopStats;
}

export interface TraceUpdate {
  sessionId: number;
  seq: number;
  target: string;
  resolvedAddr: string | null;
  ipVersion: IpVersion;
  hops: Hop[];
  totalHops: number;
  maxHops: number;
  completed: boolean;
  intervalMs: number;
  error: string | null;
}

export interface EngineInfo {
  id: string;
  description: string;
  priority: number;
  available: boolean;
}

/** A saved target (favourite / recent). */
export interface Favourite {
  host: string;
  name: string;
  ipVersion: IpVersion;
  intervalMs: number;
}

/** Geolocation for a hop IP (from an online lookup). */
export interface GeoInfo {
  country: string | null;
  countryCode: string | null;
  city: string | null;
  isp: string | null;
}
