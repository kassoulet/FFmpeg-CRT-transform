# Plan: Convert FFmpeg-CRT-transform to a native Rust program

## Context

Today the project is a single large Windows batch script (`ffcrt.bat`) that drives
**FFmpeg** through ~9 sequential CLI invocations, each writing a `TMP*` intermediate
file. Config files (`.cfg`, `KEY value ; comment`) set environment variables that are
interpolated verbatim into FFmpeg filtergraph strings. The goal is a **native Rust
program** that reproduces the same visual pipeline **without any FFmpeg dependency** —
all image processing (DSP) implemented in Rust. We keep the existing `.cfg` format so
the 16 presets in `presets/` and the `test-suite/` configs keep working unchanged.
Scope is `ffcrt` only (the `sim-rgbi1bpp/` sidecar is a later effort).

Decisions locked with the user:
- **Native Rust DSP** (no ffmpeg orchestration, no libav bindings).
- **Keep `.cfg` format** (presets work as-is).
- **`ffcrt` only.**

### One unavoidable boundary: video codecs

`ffcrt` handles both still images and video (mp4/mkv/avi, h264). Pure-Rust H.264
encode/decode is not practical. The DSP (the actual ask) is fully native; only the
container/codec I/O is not. The plan therefore phases delivery:

- **Phase A — still images, 100% native Rust** (no external binary at all). Uses the
  `image` crate for PNG/JPG/TIF/BMP I/O. This is the primary deliverable and exercises
  every spatial filter in the pipeline.
- **Phase B — video.** All per-frame DSP stays native Rust; only raw-frame demux/encode
  needs a backend. Recommended: pipe `rawvideo` frames through the `ffmpeg` binary used
  purely as a dumb codec I/O layer (no filters), OR integrate a Rust decoder for limited
  formats. This is the only place the "no ffmpeg" goal must bend, and it's I/O only.
  Flagged here as a decision to revisit when Phase A lands.

The temporal effects (`tmix`, `lagfun`, latency, p7 decay/latency) are video-only and
belong to Phase B.

## Target architecture

New Rust binary crate `ffcrt` (Cargo) at the repo root, alongside the existing files.

```
Cargo.toml
src/
  main.rs            CLI args, input probe, dispatch to image/video pipeline
  config.rs          parse .cfg -> typed Config; derived values (PX, PY, SXINT, OX, RNG...)
  monitor.rs         MONITOR_COLOR tables: per-type curve control points + special cases
  image_buf.rs       ImgF32: interleaved f32 RGBA buffer (0..1), the universal work type
  pipeline.rs        orchestrates the stages in ffcrt.bat order; SKIP_OVL/SKIP_BRI shortcuts
  ops/
    gamma.rs         pow LUT: to-linear (2.2) / from-linear (0.4545); gammaval equivalent
    resample.rs      nearest, fast_bilinear, bilinear, bicubic, lanczos, gauss-scale
    blur.rs          separable gaussian (sigma + steps, matching gblur), box passes
    blend.rs         multiply, screen, lighten, vividlight, all_opacity, bloom expr
    curves.rs        cubic-spline curves= evaluator from control-point strings
    lens.rs          lenscorrection k1/k2 barrel distortion (bilinear resample)
    vignette.rs      ffmpeg vignette model (PI*power)
    generate.rs      scanline profile, pixel grid, rounded-corner mask, solid color
    noise.rs         seeded noise for paper / lcdgrain textures (visual approximation)
    crop.rs          cropdetect (non-black bounding box) + crop
  stages/
    bezel.rs         white canvas + rounded corners + bezel curvature  (TMPbezel)
    scanlines.rs     sin^(1/weight) profile, tile, blur, curvature       (TMPscanlines)
    overlay.rs       shadowmask tile+blur+curvature OR texture overlay   (TMPshadowmask/texture)
    grid.rs          flat-panel discrete pixel grid                       (TMPgrid)
    step01.rs        prescale(neighbor)+to-linear+aspect+grid+pixel blur
    step02.rs        halation + from-linear + blackpoint + curvature
    step03.rs        bloom + scanlines + shadowmask + bezel + brighten
    output.rs        crop + rescale(OFILTER) + monochrome + vignette + pad + texture + encode
tests/
  reference.rs       perceptual comparison vs ffcrt.bat reference outputs (tolerance)
```

Recommended crates: `image` (I/O), `clap` (args), `rayon` (per-row parallelism),
`anyhow`/`thiserror` (errors). Resamplers/blur/lens are hand-written for fidelity control
rather than pulling a resize crate, since matching ffmpeg's results matters.

### Universal working representation

A single `ImgF32` type: interleaved `Vec<f32>` RGBA, channels normalized 0..1, with
width/height. This subsumes the batch script's `RNG`/`RGBFMT` 8-bit-vs-16-bit branching —
f32 makes `16BPC_PROCESSING` largely irrelevant internally (we always have headroom);
it only affects the **output** quantization (8 vs 16 bpc) selected by `OFORMAT`. Gamma
ops mirror the script's repeated `lutrgb gammaval(2.2)` ... `gammaval(0.454545)` dance:
process spatial filters in the "linear-ish" space, convert back before blends that the
script does in gamma space.

## Stage-by-stage mapping (mirror `ffcrt.bat`)

Each Rust stage reproduces one `::+++` section of the batch. Intermediates become
in-memory `ImgF32` values passed between stages (no temp files), but stage boundaries and
order match the script exactly so output stays faithful.

1. **Config + derived vars** (`config.rs`): parse `.cfg`; compute `SXINT=IX*PRESCALE_BY`,
   `PX=IX*PRESCALE_BY*PX_ASPECT`, `PY=IY*PRESCALE_BY`, `OX=round(OY*OASPECT)`, `VSIGMA`,
   `SL_COUNT`/`SCAN_FACTOR` (single/double/half), curvature clamp
   (`BEZEL_CURVATURE = max(BEZEL,CRT)`), and `FLAT_PANEL` overrides (disables scanlines,
   curvature, overlay).
2. **Monitor color** (`monitor.rs`): `rgb` = color path (shadowmask on). Non-rgb =
   monochrome: convert to gray then apply per-type `curves`. Special cases: `paperwhite`
   (paper texture), `lcd*` (inverted pixel grid + optional grain, `PXGRID_INVERT`),
   `p7` (extra latency/decay curve maps — Phase B for video; still path is the simpler split).
3. **Bezel** (`stages/bezel.rs`): solid white `PX×PY`; if `CORNER_RADIUS>0` build a
   quarter-circle mask and stamp 4 corners; apply bezel `lenscorrection` if set.
4. **Scanlines** (`stages/scanlines.rs`): 1px column, `lum = sin(Y*PI/period)^(1/SL_WEIGHT)`
   over `period=PRESCALE_BY/SCAN_FACTOR`, neighbor-scale to `PX` wide, tile `SL_COUNT` rows,
   gaussian + CRT curvature.
5. **Overlay** (`stages/overlay.rs`): if `OVL_ALPHA>0` and rgb, load `_{OVL_TYPE}.png`,
   to-linear, lanczos-scale by `OVL_SCALE`, tile to cover, blur + curvature, from-linear.
   Else build texture overlay for `paperwhite`/`lcdgrain`, or a transparent canvas.
6. **Pixel grid** (`stages/grid.rs`, flat-panel only): gap pattern via
   `mod(X,GX)>=GX-PX_X_GAP || mod(Y,GY)>=...`, gamma, scale to `PX`, gamma back.
   `GX=PRESCALE_BY/PX_FACTOR_X`, `GY=PRESCALE_BY/PX_FACTOR_Y`; `LUM_GAP/LUM_PX` invert for lcd.
7. **Step01** (`stages/step01.rs`): horizontal neighbor prescale ×`PRESCALE_BY`, to-linear,
   aspect scale (`fast_bilinear`) by `PX_ASPECT`, vertical neighbor ×`PRESCALE_BY`, optional
   grid blend (multiply, or screen if inverted), separable gaussian blur
   (`sigmaH=H_PX_BLUR/100*PRESCALE_BY*PX_ASPECT`, `sigmaV=VSIGMA`, steps=3).
8. **Step02** (`stages/step02.rs`): optional halation (gaussian + `lighten`@`HALATION_ALPHA`),
   from-linear, blackpoint lift `val + BLACKPOINT/255*(max-val)`, CRT curvature.
9. **Step03** (`stages/step03.rs`): optional bloom (desaturate scanlines, expr blend with
   `BLOOM_POWER`), multiply scanlines @`SL_ALPHA`, multiply shadowmask @`OVL_ALPHA`,
   multiply bezel, brighten `clip(val*BRIGHTEN)`. Skipped wholesale when the script's
   redundancy guard applies (no scanlines, equal curvatures, no corners, `SKIP_OVL`, `SKIP_BRI`).
10. **Output** (`stages/output.rs`): cropdetect bounding box from a curved white canvas
    over the bezel; crop; to-linear; monochrome gray+curves if set; scale to
    `OX-2*OMARGIN × OY-2*OMARGIN` preserving aspect with `OFILTER`; from-linear; vignette;
    pad/center to `OX×OY`; texture blend (paper `multiply` / lcdgrain `vividlight`+`lighten`);
    quantize to 8 or 16 bpc per `OFORMAT`; write via `image` crate.

## Fidelity expectations

Output will be **perceptually equivalent, not bit-identical**. Sources of small
divergence to call out: ffmpeg's exact lanczos/bicubic/gauss resampler kernels, `gblur`'s
IIR approximation vs our kernel, the `noise` filter's internal PRNG (textures will differ
in detail), and `lenscorrection` interpolation. We match formulas where they're documented
and tune kernels against reference images.

## Verification

- **Reference set**: run the existing `ffcrt.bat` (on a Windows/Wine box, or reuse any
  committed sample outputs) over `presets/` + `test-suite/` images to capture golden PNGs.
- **Comparison harness** (`tests/reference.rs`): run the Rust binary on the same
  `.cfg`+input pairs and compare to goldens with SSIM / mean-absolute-error under a
  per-stage tolerance; fail on regressions. Mirrors the spirit of `test-suite/readme.txt`'s
  before/after `-out`/`-OLD` scheme.
- **Stage dumps**: a `--dump-stages` flag writing each `ImgF32` boundary to PNG, so a
  diverging final image can be bisected to the offending stage against the batch's `TMP*`.
- **Smoke**: `cargo run -- presets/color-PAL-TV.cfg test-suite/06.png out.png` and eyeball;
  repeat for one preset per family (color / mono / p7 / flat-panel / lcd).

## Suggested milestones

1. Scaffold crate; `config.rs` parser + derived vars; `image_buf.rs`; gamma ops; CLI + image probe.
2. Core ops: resamplers, gaussian blur, blend modes, curves, lens, vignette, crop, generators.
3. Still-image color path (rgb): bezel → scanlines → overlay → step01–03 → output. Validate `color-*` presets.
4. Monochrome + flat-panel + textures (mono/lcd/p7-still/plasma/eld/paperwhite). Validate `mono-*`/`fpanel-*`.
5. Comparison harness + tune kernels to reference; `--dump-stages`.
6. (Phase B, later) Video: frame-I/O backend decision + temporal effects (tmix/lagfun/latency/p7 video).

## Files

- **New**: `Cargo.toml`, `src/**` (above), `tests/reference.rs`, a Rust-project `CLAUDE.md`.
- **Unchanged / reused**: `presets/*.cfg`, `test-suite/*`, `_triad.png`/`_slot.png`/`_grille.png`.
- **Untouched**: `ffcrt.bat` (kept as reference oracle during the port), `sim-rgbi1bpp/` (out of scope).
