// Application state: the set of open sessions (tabs), the active one, saved
// favourites, engine info and the shared GeoIP cache.

import { invoke } from "@tauri-apps/api/core";
import type { EngineInfo, Favourite, GeoInfo, IpVersion } from "./types";
import { Session } from "./session.svelte";
import { settings } from "./settings.svelte";
import { ensureGeo as ensureGeoLookup } from "./geoip";

const FAV_KEY = "pingnoo-favourites";

class AppState {
  sessions = $state<Session[]>([]);
  activeId = $state<number | null>(null);
  engine = $state<EngineInfo | null>(null);
  favourites = $state<Favourite[]>([]);
  geo = $state<Record<string, GeoInfo | null>>({});
  showSettings = $state(false);
  #nextId = 1;

  get active(): Session | null {
    return this.sessions.find((s) => s.id === this.activeId) ?? null;
  }

  async init() {
    this.#loadFavourites();
    try {
      const engines = await invoke<EngineInfo[]>("list_engines");
      this.engine = engines[0] ?? null;
    } catch {
      /* engine list unavailable */
    }
  }

  newSession(
    host: string,
    ipVersion?: IpVersion,
    intervalMs?: number,
    autostart = true,
  ): Session {
    const s = new Session(
      this.#nextId++,
      host.trim(),
      ipVersion ?? settings.defaultIpVersion,
      intervalMs ?? settings.defaultIntervalMs,
    );
    this.sessions = [...this.sessions, s];
    this.activeId = s.id;
    if (autostart && s.target) void s.start();
    return s;
  }

  async closeSession(id: number) {
    const idx = this.sessions.findIndex((x) => x.id === id);
    const s = this.sessions[idx];
    if (s) await s.dispose();
    this.sessions = this.sessions.filter((x) => x.id !== id);
    if (this.activeId === id) {
      this.activeId = this.sessions[Math.max(0, idx - 1)]?.id ?? null;
    }
  }

  setActive(id: number) {
    this.activeId = id;
  }

  ensureGeo(ip: string) {
    // Guard against re-lookups: once an IP has a result (even null), never touch
    // `geo` for it again. Without this, an $effect that reads `geo` and calls
    // this would loop forever (every call minted a new `geo` object).
    if (Object.prototype.hasOwnProperty.call(this.geo, ip)) return;
    ensureGeoLookup(ip, (rip, g) => {
      if (Object.prototype.hasOwnProperty.call(this.geo, rip)) return;
      this.geo = { ...this.geo, [rip]: g };
    });
  }

  // --- favourites -----------------------------------------------------------
  #loadFavourites() {
    try {
      const raw = localStorage.getItem(FAV_KEY);
      if (raw) this.favourites = JSON.parse(raw);
    } catch {
      /* ignore */
    }
  }

  #saveFavourites() {
    try {
      localStorage.setItem(FAV_KEY, JSON.stringify(this.favourites));
    } catch {
      /* ignore */
    }
  }

  addFavourite(fav: Favourite) {
    this.favourites = [fav, ...this.favourites.filter((f) => f.host !== fav.host)];
    this.#saveFavourites();
  }

  removeFavourite(host: string) {
    this.favourites = this.favourites.filter((f) => f.host !== host);
    this.#saveFavourites();
  }

  isFavourite(host: string): boolean {
    return this.favourites.some((f) => f.host === host);
  }
}

export const app = new AppState();
