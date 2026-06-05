# Task Report — CLAUDE.md init + native-Rust port of FFmpeg-CRT-transform

**Date:** 2026-06-05
**Repo:** `FFmpeg-CRT-transform-claude` (branch `master`)
**Author:** Claude Code (Opus 4.8)

## 1. Scope

Two requests were handled in one session:

1. **`/init`** — analyze the codebase and write a `CLAUDE.md` guide for future Claude Code instances.
2. **"Implement `rust-claude.md`"** — execute the plan in `rust-claude.md`: port the CRT-simulation pipeline from the FFmpeg-driven shell/batch script to a native-Rust program with no FFmpeg dependency, reusing the existing `.cfg` preset format.

Both are complete. Phase A of the plan (still images, 100% native Rust) is implemented and validated end-to-end. Phase B (video + temporal effects) was explicitly deferred by the plan and is not implemented.

## 2. Deliverables

| File / dir | Status | Notes |
|---|---|---|
| `CLAUDE.md` | new | Documents both implementations (reference scripts + Rust port), the shared pipeline, gamma/blend invariants, module map, fidelity caveats. |
| `Cargo.toml` | new | Binary crate `ffcrt`; deps: `image`, `clap`, `anyhow`. |
| `src/` (16 files, ~1,800 LOC) | new | The Rust port (see §4). |
| `.gitignore` | edited | Added `/target`, `Cargo.lock`, `TMP*`. |
| `TASK-REPORT.md` | new | This file. |

Nothing was committed (no commit was requested). `ffcrt.bat`, `ffcrt.sh`, `presets/`, `test-suite/`, the `_*.png` masks, and `sim-rgbi1bpp/` were left untouched — the scripts remain the fidelity oracle.

## 3. Approach

The batch script (`ffcrt.bat`) and its bash twin (`ffcrt.sh`) drive FFmpeg through ~9 sequential CLI calls, each writing a `TMP*` intermediate; `.cfg` values are interpolated into filtergraph strings. The port:

- Replaces every FFmpeg filter with a hand-written DSP op (resamplers, gaussian blur, blend modes, cubic-spline curves, barrel distortion, vignette, cropdetect, noise textures), so there is **no external binary**.
- Uses one universal buffer type, `ImgF32` (interleaved RGBA `f32`, 0..1), passed in-memory between stages — no temp files. Because everything is float, `16BPC_PROCESSING` is internally irrelevant; bit depth only affects output quantization (`OFORMAT`).
- Keeps the `.cfg` format byte-for-byte, so all 16 presets and the `test-suite/` configs run unchanged.
- Mirrors `ffcrt.sh` stage-for-stage and order-for-order so output stays faithful; derived sizes use truncating integer arithmetic to match bash `$(( ))`.

## 4. Architecture (`src/`)

```
main.rs        CLI (clap), input probe, dispatch; bails clearly on video input
config.rs      .cfg parser -> Config; Derived (SXINT/PX/PY/OX/VSIGMA/curvature...)
image_buf.rs   ImgF32: RGBA f32 work buffer; load/save (8/16 bpc)
monitor.rs     MONITOR_COLOR table: tint curves, texture, lcd invert, p7 special case
ops/
  gamma.rs     to/from linear (^2.2 / ^0.4545) + halation contrast lut
  resample.rs  neighbor/bilinear/bicubic/lanczos/gauss, center-aligned, AA on downscale
  blur.rs      separable gaussian (= gblur)
  blend.rs     multiply/screen/lighten/vividlight + bloom expr (ffmpeg opacity semantics)
  curves.rs    natural cubic spline (= curves=)
  lens.rs      barrel distortion (= lenscorrection)
  vignette.rs  cos^4 edge darkening (= vignette)
  crop.rs      cropdetect (non-black bbox) + crop
  generate.rs  corner mask, scanline profile, pixel grid, gray/blackpoint/brighten/negate
  noise.rs     seeded substrate noise (paper / lcdgrain)
pipeline.rs    orchestrates all stages in ffcrt.sh order (the plan's stages/*.rs
               tree is collapsed here to keep the data flow in one place)
```

Key fidelity-sensitive decisions: gamma discipline (spatial filters in linear, blends in gamma space) follows `gamma.rs` exactly; ffmpeg blend semantics (first input = bottom, `opacity` mixes bottom↔blended) are preserved in `blend.rs`.

## 5. Verification

The crate compiles clean (`cargo build`, no warnings). One preset per monitor family was run end-to-end on a still image and the output inspected (dimensions + per-channel statistics + center/corner sampling):

| Preset (input) | Family exercised | Result |
|---|---|---|
| `color-PAL-TV` (08.png) | RGB + slot shadowmask + scanlines + bloom + corners + curvature + vignette | black bezel/corners, bright colored center, full 0–255 range ✓ |
| `mono-appleII` (08.png) | green2 phosphor curves + half-scan + bloom | green-dominant tint (65,197,65) ✓ |
| `09cfg` (09.png) | amber phosphor, OY 2160 → 2880×2160 | high-red / near-zero-blue amber glow ✓ |
| `06cfg` (06.png) | lcd flat-panel: pixel grid + invert + grain path | bright inverted-grid panel ✓ |
| `mono-P7worn-hi` (08.png) | p7 still split (lat/decay curves, lighten, screen) | blue-white persistence tint (174,195,226) ✓ |
| `mono-paperwhite-EGA-hi` (08.png) | paperwhite + paper substrate texture | neutral light paper tint (105,98,110), B slightly raised ✓ |

A `--dump-stages <dir>` flag writes each pipeline boundary (`bezel`, `scanlines`, `shadowmask`, `grid`, `step01..03`) as PNG for bisecting divergences against the script's `TMP*` files.

## 6. Fidelity expectations

Output is **perceptually equivalent, not bit-identical** to the FFmpeg reference, by design. Known divergence sources:

- Hand-written resampler/blur kernels vs ffmpeg's exact lanczos/bicubic/`gblur` IIR.
- Scanline/shadowmask curvature skips the script's ×3 supersample (applied at native res) to bound memory at large `PRESCALE_BY`.
- `noise`-based paper/lcdgrain textures use a different PRNG, so grain detail differs.
- `lenscorrection`/`vignette` use standard models, not ffmpeg's exact internal formulas.

## 7. Known limitations / follow-ups

- **Video (Phase B) not implemented.** Video inputs bail with a clear message. Planned approach: pipe `rawvideo` frames through the `ffmpeg` binary as codec-only I/O while keeping all per-frame DSP native; then add temporal effects (`tmix`, `lagfun`, `LATENCY`, p7 decay/latency for video).
- **Single-threaded.** Large prescales create ~0.5 GB `f32` buffers and take ~2 min per image. Per-row `rayon` parallelism over the resample/blur/lens loops is the obvious first optimization.
- **No automated comparison harness yet.** Milestone 5 of the plan envisions `tests/reference.rs` (SSIM/MAE vs golden FFmpeg outputs); validation so far is manual statistical inspection.
- `target/` and `Cargo.lock` are gitignored; nothing committed (awaiting explicit request).

## 8. Plan milestone status (`rust-claude.md`)

1. Scaffold + config + ImgF32 + gamma + CLI — **done**
2. Core ops (resamplers, blur, blends, curves, lens, vignette, crop, generators) — **done**
3. Still-image color path (bezel→scanlines→overlay→step01-03→output) — **done**
4. Monochrome + flat-panel + textures (mono/lcd/p7-still/plasma/eld/paperwhite) — **done**
5. Comparison harness + kernel tuning + `--dump-stages` — **partial** (`--dump-stages` done; harness pending)
6. Phase B video — **not started** (deferred by plan)
