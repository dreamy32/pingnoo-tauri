// Global, persisted user settings (theme, latency thresholds, defaults, host
// masking). Mirrors the legacy Latency / Host Masker configuration.

import type { IpVersion } from "./types";

export interface MaskRule {
  pattern: string;
  replacement: string;
}

const KEY = "pingnoo-settings";

class Settings {
  theme = $state<"dark" | "light">("dark");
  // Latency colour thresholds (ms): < warn = ideal, < crit = warning, else critical.
  warnMs = $state(100);
  critMs = $state(200);
  defaultIntervalMs = $state(1000);
  defaultIpVersion = $state<IpVersion>("v4");
  // Host masking (privacy redaction), applied in the UI before display.
  maskEnabled = $state(false);
  maskPublicIp = $state<string | null>(null);
  maskRules = $state<MaskRule[]>([]);

  load() {
    try {
      const raw = localStorage.getItem(KEY);
      if (!raw) return;
      const s = JSON.parse(raw);
      if (s.theme) this.theme = s.theme;
      if (typeof s.warnMs === "number") this.warnMs = s.warnMs;
      if (typeof s.critMs === "number") this.critMs = s.critMs;
      if (typeof s.defaultIntervalMs === "number") this.defaultIntervalMs = s.defaultIntervalMs;
      if (s.defaultIpVersion) this.defaultIpVersion = s.defaultIpVersion;
      if (typeof s.maskEnabled === "boolean") this.maskEnabled = s.maskEnabled;
      if (s.maskPublicIp !== undefined) this.maskPublicIp = s.maskPublicIp;
      if (Array.isArray(s.maskRules)) this.maskRules = s.maskRules;
    } catch {
      /* ignore corrupt settings */
    }
  }

  /** Reads every field (so an $effect tracks them) and returns the JSON blob. */
  serialize(): string {
    return JSON.stringify({
      theme: this.theme,
      warnMs: this.warnMs,
      critMs: this.critMs,
      defaultIntervalMs: this.defaultIntervalMs,
      defaultIpVersion: this.defaultIpVersion,
      maskEnabled: this.maskEnabled,
      maskPublicIp: this.maskPublicIp,
      maskRules: this.maskRules,
    });
  }

  persist() {
    try {
      localStorage.setItem(KEY, this.serialize());
    } catch {
      /* ignore */
    }
  }
}

export const settings = new Settings();
