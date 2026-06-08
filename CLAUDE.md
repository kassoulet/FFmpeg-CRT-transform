# CLAUDE.md

Guidance for Claude Code when working with this repository.

## What this project is

A configurable simulation of CRT monitors (and some older flat-panel displays)
applied to an input image, implemented as a native Rust crate (`crt-transform`). The
original FFmpeg shell/batch scripts (`ffcrt.sh` / `ffcrt.bat`) are kept as a
behavioural specification but the **Rust implementation is primary**.

The library crate (`src/lib.rs`) exposes the full pipeline via `crt_transform::run`.
The binary (`src/main.rs`) is a thin CLI wrapper. Still images (Phase A) are
complete. Video (Phase B) is not yet implemented.

## Commands

```bash
cargo build --release
# NOTE: this environment sets CARGO_TARGET_DIR=/home/gautier/target
# so the binary is at $CARGO_TARGET_DIR/release/crt-transform
crt-transform <config.cfg> <input_image> [output_image]
crt-transform --dump-stages /tmp/stages presets/color-PAL-TV.cfg test-suite/08.png /tmp/out.png

cargo test                                  # tests/basic.rs drives the built CLI binary
cargo test --test basic cli_accepts_png     # run a single integration test by name
cargo run --example run_preset -- presets/color-PAL-TV.cfg test-suite/08.png  # uses crt_transform::run
cargo bench                                 # criterion DSP micro-benchmarks → target/criterion/report
cargo doc --open

# CI / pre-commit gates (run before committing — see .pre-commit-config.yaml, .github/)
cargo fmt -- --check
cargo clippy -- -D warnings
```

Library entry point is `crt_transform::run(&config_path, &input_path, &output_path, dump_stages_dir)`
(see `examples/run_preset.rs`). Profiling is documented in `PROFILING.md` (the
`profiling` crate + `profile-with-puffin` feature).

## Pipeline stages (still-image path)

Functions in `src/pipeline.rs` map 1:1 onto `ffcrt.sh` sections:

1. **Config + derived vars** — `config.rs` + `Derived::compute`; truncating integer math matches bash `$(( ))`.
2. **Bezel** — white `PX×PY` canvas, optional rounded corners + curvature.
3. **Scanlines** — `sin^(1/SL_WEIGHT)` profile → tile → blur → CRT-curve.
4. **Shadowmask** — `_<OVL_TYPE>.png` → linearize → scale → tile → blur → curve → from-linear.
5. **Pixel grid** — flat-panel only: gap pattern, gamma round-trip.
6. **Step01** — neighbor prescale → to-linear → aspect scale → grid blend → separable pixel blur.
7. **Step02** — optional halation → from-linear → blackpoint → CRT curvature.
8. **Step03** — bloom → multiply scanlines/shadowmask/bezel → brighten. Redundancy guard skips when no-op.
9. **Output** — cropdetect → crop → to-linear → gray+tint → resize → from-linear → vignette → pad/center → texture.

### Gamma discipline
`x^2.2` for to-linear, `x^(1/2.2)` for from-linear. Spatial ops (resample, blur, lens) in linear space; multiply/screen/lighten blends in gamma space. See `src/ops/gamma.rs`.

### Blend semantics
ffmpeg convention: **first input = bottom (B), second = top (A)**. `out = (1-op)*B + op*mode(A, B)`. See `src/ops/blend.rs`.

## Module map

| Module | Purpose |
|--------|---------|
| `lib.rs` | Public API re-exports (`crt_transform::run`, `crt_transform::ImgF32`, `crt_transform::Config`) |
| `main.rs` | CLI binary (clap) — thin wrapper |
| `config.rs` | `.cfg` parser → `Config`; `Derived` for integer-truncated derived vars |
| `image_buf.rs` | `ImgF32` — interleaved RGBA `Vec<f32>`, the universal work buffer |
| `monitor.rs` | `MONITOR_COLOR` table: tint curves, texture type, grid inversion |
| `pipeline.rs` | Orchestrator — all 9 stages in order, no temp files |
| `ops/` | DSP primitives: blur, blend, resample, lens, curves, gamma, crop, vignette, generate, noise |

## Fidelity expectations

Output is **perceptually equivalent, not bit-identical** to the FFmpeg reference.
Known divergences by design:
- Hand-written resample/blur kernels differ from ffmpeg's internals.
- Curvature step skips the ×3 supersample (done at native res to bound memory).
- Noise-based textures use a different PRNG.
- `lenscorrection`/`vignette` use standard models, not ffmpeg's formulas.

Validate with `--dump-stages` against the script's `TMP*` files, testing one
preset per family (color / mono / p7 / flat-panel / lcd).

## Performance

All DSP ops use per-row rayon parallelism. On a 6-core / 12-thread CPU the
Rust port is 3–11× faster than the FFmpeg reference. See `benchmark.md`.

## Not yet implemented (Phase B)

Video and temporal effects (`tmix`, `lagfun`, `LATENCY`, p7 decay for video).
Planned approach: pipe rawvideo frames through the `ffmpeg` binary as a codec
I/O layer while keeping per-frame DSP native.
