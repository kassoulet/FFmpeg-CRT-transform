# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

A configurable simulation of CRT monitors (and some older flat-panel displays) applied to an input image or video. There are **two implementations of the same pipeline**:

1. **Reference scripts** — `ffcrt.bat` (Windows) and `ffcrt.sh` (bash). These drive **FFmpeg** through ~9 sequential CLI invocations, each writing a `TMP*` intermediate file. This is the original, authoritative behaviour and handles both images and video. Requires a git-master FFmpeg build from 2021-01-27 or newer.
2. **Native Rust port** — the `ffcrt` Cargo crate (`Cargo.toml` + `src/`). Reimplements the same visual pipeline as pure-Rust DSP **with no FFmpeg dependency**. Phase A (still images) is complete; video (Phase B) is not yet implemented. The port is the subject of `rust-claude.md` (the plan) and `rust-gemini.md`.

Both consume the **same `.cfg` format**, so the 16 presets in `presets/` and the `test-suite/` configs work with either. The scripts remain the fidelity oracle for the Rust port.

`sim-rgbi1bpp/` is a separate sidecar tool (1-bit display shading) and is out of scope for the Rust port.

## Commands

### Rust port (still images)
```bash
cargo build --release
# NOTE: this environment sets CARGO_TARGET_DIR=/home/gautier/target,
# so the binary is at $CARGO_TARGET_DIR/release/ffcrt, not ./target/.
ffcrt <config.cfg> <input_image> [output_image]    # output defaults to (input)_(config).(ext)
ffcrt presets/color-PAL-TV.cfg test-suite/08.png /tmp/out.png
ffcrt --dump-stages /tmp/stages presets/color-PAL-TV.cfg test-suite/08.png /tmp/out.png  # debug each stage
cargo build && cargo clippy
```
`--dump-stages <dir>` writes each pipeline boundary (`bezel`, `scanlines`, `shadowmask`, `grid`, `step01..03`) as a PNG — use it to bisect a divergence against the script's `TMP*` files.

### Reference scripts
```bash
./ffcrt.sh <config.cfg> <input_image_or_video> [output]   # needs ffmpeg + ffprobe on PATH
./test-suite/run-tests.sh                                  # runs the sample inputs/configs
```
To capture golden images from the reference for comparison, run `ffcrt.sh` over `presets/` + `test-suite/` inputs and diff against the Rust output (perceptual, not bit-exact — see fidelity notes).

## How the pipeline works (shared mental model)

The transform is a fixed sequence of stages. The Rust functions in `src/pipeline.rs` map **1:1** onto the `# ---` comment sections of `ffcrt.sh` — read the two side by side. Order (still-image path):

1. **Config + derived vars** — parse `.cfg`; compute `SXINT = IX*PRESCALE_BY`, `PX = IX*PRESCALE_BY*PX_ASPECT`, `PY = IY*PRESCALE_BY`, `OX = round(OY*OASPECT)`, `VSIGMA`, scan period/count, and `BEZEL_CURVATURE = max(BEZEL, CRT)`. `FLAT_PANEL=yes` forces scanlines/CRT-curvature/overlay **off**; non-`rgb` `MONITOR_COLOR` forces the shadowmask off. These derived integers must use **truncating integer arithmetic** to match bash `$(( ))` so canvas sizes line up.
2. **Bezel** — white `PX×PY` canvas, optional rounded corners, optional bezel curvature.
3. **Scanlines** — `sin^(1/SL_WEIGHT)` luminance profile, period `PRESCALE_BY/SCAN_FACTOR`, tiled to `PX×PY`, blurred + CRT-curved.
4. **Shadowmask overlay** — only for `rgb`: load `_<OVL_TYPE>.png`, linearize, scale by `OVL_SCALE`, tile, blur + curve. (`_triad.png`/`_slot.png`/`_grille.png` live at the repo root and must be in the working dir.)
5. **Pixel grid** — flat-panel only: discrete-pixel gap pattern, scaled to `PX` wide.
6. **Step01** — neighbor prescale ×`PRESCALE_BY`, to-linear, aspect scale, grid blend, separable pixel blur (`H_PX_BLUR`/`VSIGMA`). **Works in linear light.**
7. **Step02** — optional halation, back to gamma space, blackpoint lift, CRT curvature.
8. **Step03** — optional bloom, multiply scanlines @`SL_ALPHA`, multiply shadowmask @`OVL_ALPHA`, multiply bezel, brighten. Skipped wholesale by a redundancy guard (no scanlines + equal curvatures + no corners + no overlay + brighten==1).
9. **Output** — cropdetect the curved-white bounding box, crop, to-linear, monochrome gray+tint curves if applicable, scale to `OY*OASPECT − margins` preserving aspect with `OFILTER`, vignette, pad/center to `OX×OY`, paper/lcdgrain texture, quantize to 8/16bpc per `OFORMAT`.

### Gamma discipline (important and easy to get wrong)
The pipeline repeatedly converts to "linear" (`x^2.2`) for spatial filtering and back (`x^(1/2.2)`) before gamma-space blends. Spatial ops (resample, blur, lens) happen in linear; the multiply/screen/lighten blends in step03 happen in gamma space. Match the script's conversions exactly — `src/ops/gamma.rs` is the single source.

### Blend semantics
ffmpeg `blend`: **first input = bottom (B), second = top (A)**, and `all_opacity` mixes between the bottom and the blended result: `out = (1-op)*bottom + op*mode(top, bottom)` (so opacity 0 leaves the bottom untouched). `src/ops/blend.rs` follows this; preserve it when adding modes.

## Rust module map (`src/`)

- `main.rs` — CLI (clap), input probe, dispatch. Bails clearly on video input.
- `config.rs` — `.cfg` parser (`KEY value ; comment`) → `Config`; `Derived` computes the integer-truncated derived vars. `Frac` handles `num/den` values (`PX_ASPECT`, `OASPECT`).
- `image_buf.rs` — `ImgF32`: interleaved RGBA `Vec<f32>` (0..1), the **universal work buffer**. Because everything is f32, `16BPC_PROCESSING` is internally irrelevant; bit depth only affects output quantization (`OFORMAT`).
- `monitor.rs` — `MONITOR_COLOR` table: per-type tint `curves`, texture type, lcd grid inversion, p7 special case (the `case` block from `ffcrt.sh`).
- `ops/` — DSP primitives, each shaped like the ffmpeg filter it replaces: `gamma`, `resample` (neighbor/bilinear/bicubic/lanczos/gauss, center-aligned, downscale anti-aliasing), `blur` (separable gaussian = `gblur`), `blend`, `curves` (natural cubic spline = `curves=`), `lens` (barrel distortion = `lenscorrection`), `vignette`, `crop` (`cropdetect`+`crop`), `generate` (corner mask / scanline profile / pixel grid / gray / blackpoint / brighten / negate), `noise` (seeded substrate texture).
- `pipeline.rs` — orchestrates all stages in script order, passing in-memory `ImgF32` between them (no temp files). The plan's `src/stages/*.rs` tree is intentionally collapsed into the stage functions here.

## Fidelity expectations

Output is **perceptually equivalent, not bit-identical** to the FFmpeg reference. Known sources of divergence, by design:
- Our resampler/blur kernels are hand-written; ffmpeg's exact lanczos/bicubic/`gblur` IIR differ slightly.
- The scanline/shadowmask curvature step skips the script's ×3 supersample (done at native res) to bound memory at large `PRESCALE_BY`.
- `noise`-based textures (paper/lcdgrain) use a different PRNG, so grain detail differs.
- `lenscorrection`/`vignette` use standard models, not ffmpeg's exact internal formulas.

When changing DSP, validate against script output with `--dump-stages` and eyeball one preset per family (color / mono / p7 / flat-panel / lcd).

## Performance note

The Rust port is single-threaded and works on full `PX×PY` f32 buffers (e.g. `color-PAL-TV` on 640×480 → 6400×4800 ≈ 0.5 GB/buffer, ~2 min). Per-row parallelism (rayon) over the resample/blur/lens loops is the obvious optimization and is the first thing to reach for if speed matters.

## Not yet implemented (Phase B)

Video and all temporal effects (`tmix`, `lagfun`, `LATENCY`, p7 decay/latency for video). Pure-Rust H.264 is impractical; the planned approach pipes `rawvideo` frames through the `ffmpeg` binary as a dumb codec I/O layer while keeping all per-frame DSP native. See `rust-claude.md`.
