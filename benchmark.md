# Benchmark: Rust ffcrt vs FFmpeg ffcrt.sh

System: GNU bash 5.2.21, FFmpeg 6.1.1, Rust release build
CPU: AMD Ryzen 5 5600G (6 cores / 12 threads)
Run: 2026-06-05

## Still-image test cases

| Test | Input | Config | FFmpeg | Rust (single) | Rust (rayon) | Ratio (rayon vs ffmpeg) |
|------|-------|--------|--------|---------------|--------------|-------------------------|
| 06 | 640×480, PRESCALE_BY=10 | paperwhite, halation+bloom | 9.71s | 13.64s | **2.58s** | **3.8× faster** |
| 08 | 640×480, PRESCALE_BY=10 | color RGB triad | 10.57s | 3.08s | **0.92s** | **11.5× faster** |
| 09 | 800×416, PRESCALE_BY=6 | amber monochrome | 9.28s | 11.64s | **2.54s** | **3.7× faster** |

## Speedup from parallelization

| Test | Single-thread | Rayon | Speedup |
|------|--------------|-------|---------|
| 06 | 13.64s | 2.58s | **5.3×** |
| 08 | 3.08s | 0.92s | **3.3×** |
| 09 | 11.64s | 2.54s | **4.6×** |

## Output file sizes

| Test | FFmpeg | Rust |
|------|--------|------|
| 06 | 3,357,890 B | 2,947,073 B |
| 08 | 3,327,736 B | 3,412,492 B |
| 09 | 5,460,784 B | 3,189,843 B |

## Observations

- **08 (color CRT)** — Rust+rayon is 11.5× faster than FFmpeg. The shadowmask/blur/curvature pipeline benefits from both in-memory processing and per-row parallelism.
- **06 (paperwhite monochrome)** — Rust+rayon is 3.8× faster. Halation+bloom (Gaussian blurs on full-resolution f32 buffers) now scale well across cores.
- **09 (amber)** — Rust+rayon is 3.7× faster. Monochrome curve processing and curvature step parallelize efficiently.

Rayon adds per-row parallelism across all DSP primitives (resample, blur, lens, blend, vignette, curves, noise, crop, and the pixel-tiling loops). The 3–5× wall-clock speedup on a 6-core/12-thread CPU matches expectations for memory-bound image processing.

## Compatibility notes

The FFmpeg script (`ffcrt.sh`) required two workarounds for this environment:

1. **`PX_ASPECT=2/3` in bash arithmetic** — bash 5.2 evaluates bare variable references in `$(( ))` differently than older versions. `$((IX * PRESCALE_BY * PX_ASPECT))` with `PX_ASPECT=2/3` resolves the variable as a sub-expression where integer division `2/3 = 0`, producing width 0. Using `${PX_ASPECT}` instead does textual substitution first: `$((800 * 6 * 2/3)) = 3200`.

2. **`crop=` prefix from cropdetect** — `cropdetect` outputs `crop=W:H:X:Y`; the script passes this directly as `crop=${CROP_STR}`, producing `crop=crop=W:H:X:Y`. FFmpeg 6.1's crop filter has a boolean `crop` option that conflicts with this doubled syntax. Stripping the prefix (`CROP_STR="${CROP_STR#crop=}"`) restores standard positional arg parsing: `crop=W:H:X:Y`.

Neither issue affects the Rust port, which has its own config parser and doesn't rely on ffmpeg/cropdetect.
