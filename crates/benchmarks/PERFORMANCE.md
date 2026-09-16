# P1-7 performance baseline

The `p1_hot_paths` Criterion target measures only deterministic, in-memory work. Production desktop-frame code and the benchmark both call `at-pc-desktop-core`; the benchmark is not a copied implementation.

## Benchmarks

- `frame/rgba_to_rgb/1920x1080_copy`
- `frame/rgba_to_rgb/3840x2160_half_scale`
- `frame/jpeg_encode/{1280x720/q55,1920x1080/q60,3840x2160/q85}`
- `frame/change_detection/block_hash_1920x1080_rgba`
- `frame/change_detection/{decision_changed,decision_static_skip,decision_static_keepalive}`
- `registry/list/{empty,active_100,active_1000,mixed_500_active_500_offline}`

Registry setup and synthetic frame generation happen outside timed iterations. Registry cases use `TerminalRegistry::new()` and its in-memory metadata store, so no filesystem or network I/O is measured.

P1-3 changes `fast_rgba_to_rgb` geometry: default `scale = 1.0` now caps output at 1280 px wide instead of the `>1920 → half` cliff. The `1920x1080_copy` and `3840x2160_half_scale` bench names are historical; both now measure the continuous target-width convert. JPEG benches still encode the listed source sizes (they do not go through `stream_output_size`). Dirty detection benchmarks now target `compute_block_hashes` (the 64×64 block hash used in production).

## Save the P1-7 baseline

Run on an idle machine with a fixed power profile:

```bash
cargo bench -p at-pc-benchmarks --bench p1_hot_paths --locked -- --save-baseline p1-7
```

Record CPU model, OS, Rust version, power profile, and the Criterion estimates printed by the command. Criterion stores the machine-local baseline below `target/criterion`; that generated data is intentionally not source-controlled.

## Compare P1-3

On the same host and power profile, after P1-3:

```bash
cargo bench -p at-pc-benchmarks --bench p1_hot_paths --locked -- --baseline p1-7
```

Repeat the comparison at least three times. Treat a median regression over 10% as requiring investigation, and a median improvement of at least 5% as meaningful. Decision-only nanosecond benchmarks are noise indicators rather than hard gates; use the 20% threshold in `thresholds.toml`. Shared CI wall-clock results are informational only.

Convert benches are expected to improve because 1920×1080 and 3840×2160 now emit ~1280×720 instead of 1920×1080 / 1920×1080. That is a geometry change, not a like-for-like micro-optimization of the same pixel count.

## Hardware encode evaluation (P1-3, evaluate only)

This slice keeps software JPEG via the existing `image` crate. No new workspace crates and no `Cargo.toml` / `Cargo.lock` edits — those remain P2-4.

| Option | What it actually encodes | Integration cost | Protocol / player impact | Verdict for this slice |
| --- | --- | --- | --- | --- |
| **`image` JPEG (current)** | Pure-Rust baseline JPEG | Already in `at-pc-desktop-core` | Unchanged `DFRM` + JPEG bytes | **Keep.** Matches the dashboard poller and binary frame header. |
| **turbojpeg / libjpeg-turbo** | SIMD software JPEG, typically 2–6× faster than `image` at similar quality | New native dep (nasm/cmake), `turbojpeg` crate, Windows/macOS/Linux bindgen or vendoring | None if we keep JPEG-in-`DFRM` | Best *software* upgrade. Blocked here because it needs a workspace dependency, which P2-4 owns. Revisit once the lockfile owner can add the crate. |
| **Windows Media Foundation** | Hardware H.264/HEVC (MFT). Hardware *JPEG* sinks are rare and vendor-specific | `windows` crate COM features, IMFSinkWriter, adapter/session lifetime, fallback when no MFT | Would replace JPEG with Annex-B or AVCC; dashboard `<img>` and `/frame.jpg` cannot decode H.264 | Not a drop-in JPEG accelerator. Worth a later P2 if we introduce a real video track + MSE/WebCodecs player. True-machine fps belongs on a Windows box with a GPU MFT. |
| **VideoToolbox** | Hardware H.264/HEVC (`VTCompressionSession`) on Apple Silicon / T2 | `core-media` / `video-toolbox` bindings, session reset on resolution change, fallback to software | Same container change as MF | Same as MF: great for a future H.264 stream, not for the current JPEG poller. True-machine fps is leftover work on a real Mac display session. |

**Recommendation:** stay on `image` JPEG for the current binary protocol. The idle-path win in this slice is *skipping* JPEG (block-hash dirty check + 5 s header-only keepalive), which dominates turbojpeg/HW for office desktops. Next encode upgrade, in order: (1) turbojpeg behind the existing JPEG payload once P2-4 can take the dep, (2) optional H.264 via VideoToolbox/MF only together with a player that is not `image/jpeg`.

Real Windows/macOS hardware encode and true-machine fps are leftover, not this slice.

## Deliberately excluded

Real screen capture, monitor enumeration/handle refresh, frame pacing/sleep behavior, WebSocket backpressure, and end-to-end desktop latency require a graphical session, capture permission, stable display hardware, and a receiver. They cannot be measured reliably in a headless environment. JPEG and frame preparation remain measurable without a screen.
