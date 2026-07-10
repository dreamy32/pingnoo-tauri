# Pingnoo (Tauri + Rust)

A ground-up rewrite of [Pingnoo](../README.md) (the Qt/C++ traceroute + continuous
ping analyser) on **Tauri v2 + Rust + Svelte 5**. This directory is a
self-contained Cargo workspace; the legacy C++ tree alongside it is untouched.

## Features

- **Live route analysis** — combined traceroute + continuous per-hop ping with
  render-ready stats (loss %, last/avg/min/max latency, jitter) computed in Rust.
- **Multi-target tabs** — trace several hosts concurrently, one stream per tab.
- **PingPlotter-style chart** — hover tooltip (per-hop latency + timeouts),
  nearest-series focus, red loss markers, wall-clock axis, and a viewport
  selector (1 min → 12 h, ~14 h history).
- **Favourites** — star targets and reopen them from the ★ menu.
- **Settings** — latency colour thresholds, default interval / IP version /
  chart window, dark & light theme (all persisted).
- **Host masking** — public-IP + regex redaction for safe screenshots; while
  enabled, hop IPs are also never sent to the GeoIP service.
- **GeoIP locality** — per-hop country/city/ISP via ipwho.is (HTTPS, keyless),
  cached, private/reserved ranges never leave the machine.
- **Reverse DNS** — background hop hostname resolution, negatively cached.
- **No admin required on Windows** — IP Helper (`IcmpSendEcho`) engine; raw
  sockets via trippy on Linux/macOS (unprivileged datagram where available).
- **Never freezes, by construction** — all I/O and math in Rust on tokio; one
  bounded, latest-wins coalescing `ipc::Channel` per session; the webview only
  renders. Sessions keep tracing while the window is minimized (the WebView2
  renderer is suspended to save power) and the UI reattaches on restore.

## Architecture

```
crates/pingnoo-core   pure types + wire contract + stats accumulator (PingData port), unit-tested
crates/pingnoo-engine async engine — trippy on Unix, IP Helper API on Windows; bounded rounds,
                      clean cancellation, per-session trace identifiers
                      └ bin/trace-demo  headless soak harness (real ICMP, no GUI)
src-tauri             Tauri shell: session registry (sessions outlive the webview),
                      coalescing forwarder, attach/reattach, minimize suspend (power.rs)
src/                  Svelte 5 frontend: tabs, hop table, uPlot chart, settings, masking
```

## Run (development)

Requires Rust, Node 18+, and (Linux) the Tauri system deps
(`libwebkit2gtk-4.1-dev`, `libsoup-3.0-dev`, …).

```bash
npm install
npm run tauri dev
```

ICMP privileges:
- **Windows** — none needed (IP Helper engine).
- **macOS** — none needed (unprivileged ICMP datagram socket).
- **Linux** — unprivileged datagram where `net.ipv4.ping_group_range` allows;
  otherwise grant the binary raw-socket capability:
  `sudo setcap cap_net_raw+ep target/debug/pingnoo-tauri`

## Build (production)

```bash
npm run tauri build
```

Optimized binary in `src-tauri/target/release/`, installers under
`src-tauri/target/release/bundle/` (msi/nsis on Windows, dmg on macOS,
deb/AppImage on Linux).

### Headless engine check (no GUI)

```bash
cargo run -p pingnoo-engine --bin trace-demo -- 1.1.1.1 5 1000
cargo test --workspace
```

## Known limitations

- Windows engine is IPv4-only (the IPv6 `Icmp6SendEcho2` path is planned); the
  IPv6 toggle is disabled there.
- GeoIP and public-IP detection need internet access and are rate-limited by
  the free services they use.
- After a very long minimize, WebView2 may reclaim the renderer; stats survive
  (they live in Rust) but the chart's visual history restarts.

## Roadmap

World map of hops (GeoIP lat/lon is already available), min/max latency bands,
copy-as-image export, i18n, session save/open, signed installers + updater.
