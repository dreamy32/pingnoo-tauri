# Pingnoo (Tauri + Rust) — Phase 0 prototype

A ground-up rewrite of [Pingnoo](../README.md) (the Qt/C++ traceroute + continuous
ping analyser) on **Tauri v2 + Rust + Svelte 5**. This directory is a
self-contained Cargo workspace; the legacy C++ tree alongside it is untouched.

## Why

The legacy app freezes because rendering happens on the GUI thread: one
`QCustomPlot` per hop calling `replot()` every tick, synchronous per-hop reverse
DNS, and whole-model table invalidation per sample. This rewrite makes
"never freezes" a **structural property**:

- **Rust backend** (on Tauri's tokio runtime) does all I/O + math and never
  touches the UI.
- **Svelte webview** does all rendering on a `requestAnimationFrame` loop that is
  decoupled from data arrival, and owns bounded chart history.
- Between them: **one back-pressured `ipc::Channel`** carrying latest-wins
  whole-trace snapshots. Coalescing dropped snapshots is always safe.

## Layout

```
crates/pingnoo-core   pure types + wire contract + stats accumulator (PingData port), unit-tested
crates/pingnoo-engine async engine — trippy on Unix, IP Helper API on Windows; bounded rounds, clean cancel
                      └ bin/trace-demo  headless soak harness (real ICMP, no GUI)
src-tauri             Tauri shell: start/stop/list_engines commands + coalescing forwarder
src/                  Svelte 5 frontend: ControlBar, HopTable, uPlot LatencyChart
```

## Run

Requires Rust, Node 18+, and (Linux) the Tauri system deps
(`libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, …).

```bash
npm install
npm run tauri dev          # launches the desktop app
```

Raw-socket ICMP needs privileges. On Linux either run as root or grant the dev
binary the capability:

```bash
sudo setcap cap_net_raw+ep target/debug/pingnoo-tauri
```

macOS uses the unprivileged ICMP datagram socket (no root needed).

### Headless engine check (no GUI)

```bash
cargo run -p pingnoo-engine --bin trace-demo -- 1.1.1.1 5 1000
cargo test --workspace
```

## Status

Phase 0 (prototype): single IPv4 target, live hop table + streaming latency
chart, clean start/stop. See [the roadmap](../ROADMAP.md) discussion and the plan
for Phases 1–5 (bespoke engine, IPv6, multi-target tabs, GeoIP + masking, map,
packaging).
