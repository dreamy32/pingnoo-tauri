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
  /// Default chart window (seconds); new tabs start with this and changing a
  /// tab's selector updates it.
  windowSecs = $state(300);
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
      if (typeof s.warnMs === "number" && s.warnMs > 0) this.warnMs = s.warnMs;
      if (typeof s.critMs === "number" && s.critMs > 0) this.critMs = s.critMs;
      if (typeof s.defaultIntervalMs === "number") this.defaultIntervalMs = s.defaultIntervalMs;
      if (s.defaultIpVersion === "v4" || s.defaultIpVersion === "v6")
        this.defaultIpVersion = s.defaultIpVersion;
      if (typeof s.windowSecs === "number" && s.windowSecs >= 60) this.windowSecs = s.windowSecs;
      if (typeof s.maskEnabled === "boolean") this.maskEnabled = s.maskEnabled;
      if (s.maskPublicIp === null || typeof s.maskPublicIp === "string")
        this.maskPublicIp = s.maskPublicIp;
      if (Array.isArray(s.maskRules))
        this.maskRules = s.maskRules.filter(
          (r: unknown): r is { pattern: string; replacement: string } =>
            !!r &&
            typeof (r as MaskRule).pattern === "string" &&
            typeof (r as MaskRule).replacement === "string",
        );
    } catch {
      /* ignore corrupt settings */
    }
  }

  /** Effective critical threshold — always above the warning threshold, so the
   *  colour bands stay coherent even if the user types warn >= crit. */
  get effectiveCritMs(): number {
    return Math.max(this.critMs, this.warnMs + 1);
  }

  /** Reads every field (so an $effect tracks them) and returns the JSON blob. */
  serialize(): string {
    return JSON.stringify({
      theme: this.theme,
      warnMs: this.warnMs,
      critMs: this.critMs,
      defaultIntervalMs: this.defaultIntervalMs,
      defaultIpVersion: this.defaultIpVersion,
      windowSecs: this.windowSecs,
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
