# ffcrt — native-Rust CRT / flat-panel monitor simulation

A fast, configurable simulation of CRT monitors and older flat-panel displays,
applied to still images. Written in pure Rust with **no external dependencies**
— no FFmpeg required.

```
cargo run --release -- presets/color-PAL-TV.cfg input.png output.png
```

## Features

- **Color CRTs** — shadow mask (triad / slot / grille), scanlines, bloom, halation, CRT curvature
- **Monochrome CRTs** — amber, green, white, P7 phosphor, paperwhite, and more, with tint curves
- **Flat-panel displays** — LCD, plasma, ELD with pixel grid, substrate grain
- **16 configurable presets** in `presets/`, with commented parameters
- **Per-row parallelism** via rayon — 3–11× faster than the original FFmpeg pipeline
- **Debug output** via `--dump-stages <dir>` to inspect each pipeline step

## Quick start

```bash
# Build (release)
cargo build --release

# Run on a test image with a color CRT preset
./target/release/ffcrt presets/color-PAL-TV.cfg test-suite/08.png /tmp/out.png

# Debug each pipeline stage
./target/release/ffcrt --dump-stages /tmp/stages presets/color-PAL-TV.cfg test-suite/08.png /tmp/out.png
```

## Usage

```
ffcrt <config.cfg> <input_image> [output_image]
```

- `<config.cfg>` — a configuration file (see `presets/` for examples).
- `<input_image>` — a still image (PNG, JPG, TIF, BMP).
- `[output_image]` — optional; defaults to `(input)_(config).(ext)`.

**Video input** is not yet supported (Phase B, planned).

## Configuration

All parameters are documented inline in the sample `.cfg` files under `presets/`.
Key settings:

| Parameter | Effect |
|-----------|--------|
| `PRESCALE_BY` | Integer prescale factor (higher = sharper but slower) |
| `MONITOR_COLOR` | `rgb` for color, or a monochrome type (amber, green1, p7, ...) |
| `OVL_TYPE` | Shadow mask shape: `triad`, `slot`, or `grille` |
| `SCANLINES_ON` / `SL_WEIGHT` | Scanline effect thickness and intensity |
| `HALATION_ON` / `HALATION_RADIUS` | Glow around bright areas |
| `CRT_CURVATURE` | Barrel distortion for curved CRT surfaces |
| `FLAT_PANEL` / `PXGRID_ALPHA` | Flat-panel grid overlay |
| `OFORMAT` | Output depth: `0` = 8-bit RGB, `1` = 16-bit RGB |

## Project structure

```
Cargo.toml           # Crate metadata + dependencies
src/
  lib.rs             # Public API: ffcrt::run, ffcrt::ImgF32, ffcrt::Config
  main.rs            # CLI binary (thin wrapper over the library)
  config.rs          # .cfg parser
  image_buf.rs       # ImgF32 — universal f32 work buffer
  monitor.rs         # MONITOR_COLOR table + tint curves
  pipeline.rs        # Pipeline orchestrator (9 stages)
  ops/               # DSP primitives (blur, blend, resample, lens, ...)
presets/              # 16 sample configuration files
test-suite/           # Test inputs + configs for regression checking
tests/                # Integration tests
benches/              # Benchmarks (requires nightly)
examples/             # Library usage examples
```

## Performance

On a Ryzen 5 5600G (6 cores / 12 threads):

| Test | FFmpeg (reference) | Rust (single-thread) | Rust (rayon) | vs FFmpeg |
|------|-------------------|---------------------|-------------|-----------|
| Color CRT (640×480) | 10.6 s | 3.1 s | **0.92 s** | **11.5× faster** |
| Paperwhite (640×480) | 9.7 s | 13.6 s | **2.58 s** | **3.8× faster** |
| Amber (800×416) | 9.3 s | 11.6 s | **2.54 s** | **3.7× faster** |

See `benchmark.md` for detailed results.

## Background

This is a native-Rust port of [VileR's FFmpeg-CRT-transform](https://github.com/viler-int10h/FFmpeg-CRT-transform/)
scripts. The original shell/batch scripts drive FFmpeg through ~9 sequential
invocations with intermediate temp files. The Rust port reimplements the same
pipeline as pure-Rust DSP — same `.cfg` presets, perceptually equivalent output,
3–11× faster.

### Reference scripts

The original `ffcrt.sh` and `ffcrt.bat` are kept in the repository as a
behavioural specification. They require a git-master FFmpeg build from
2021-01-27 or newer.

### Blog write-ups (by VileR)

1. [Simulating CRT Monitors (color)](https://int10h.org/blog/2021/01/simulating-crt-monitors-ffmpeg-pt-1-color/)
2. [Simulating CRT Monitors (monochrome)](https://int10h.org/blog/2021/02/simulating-crt-monitors-ffmpeg-pt-2-monochrome/)
3. [Simulating Flat-Panel Displays](https://int10h.org/blog/2021/03/simulating-non-crt-monitors-ffmpeg-flat-panels/)

## License

MIT OR Apache-2.0
