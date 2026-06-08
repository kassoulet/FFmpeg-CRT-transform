# crt-transform — Development Plan

## Current state (Phase A complete)

Still-image pipeline is complete and passing all tests.  The binary produces
perceptually correct output for all 16 presets.  Performance is 3–11× faster
than the ffcrt.sh reference on a 6-core machine.

---

## Completed items

| ID | Item | Commit |
|----|------|--------|
| A1 | Gamma LUT (4096-entry, ~10× speedup) | ee03c47 |
| A2 | Gaussian kernel caching in `gblur_iso` | 814b9f0 |
| A3 | Eliminate `step02.clone()` in `step03` | ee03c47 |
| A4 | `ImgF32::for_each_row_mut` helper | ee03c47 |
| A5 | Blur edge-loop consolidation + dynamic gather buffer | 1246755 / fc6e54f |
| B1 | `Config::validate()` with range warnings | 5b4ca50 |
| B2 | Early overlay-file existence check | ee03c47 |
| B3 | Progress callback in `run()` | ee03c47 |
| B4 | Monitor profile unit tests | ee03c47 |
| B5 | Expanded bench coverage (gamma, blend, vignette) | ee03c47 |
| SIMD | Investigated manual SSE2/FMA intrinsics — **reverted** | fc6e54f |

> SIMD note: hand-written 128-bit SSE2 was 40% slower than the scalar loops
> that LLVM auto-vectorizes to AVX2+FMA (256-bit) via `target-cpu=native`.
> The scalar `accum_rgba` + `saxpy` structure was kept; the compiler wins.

---

## Remaining work

### Performance

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| (none) | All profitable perf work done | — | — | rayon + AVX2 auto-vec + gamma LUT is the ceiling without data-layout changes |

### Code quality / DX

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| B6 | Expand integration tests | `tests/basic.rs` | M | One test per preset family (color / mono / p7 / flat-panel / lcd); verify output file size and that no stage panics |
| B7 | `bench_full_pipeline` | `benches/ops.rs` | S | End-to-end benchmark on a synthetic 640×480 input through the color-PAL-TV preset; provides a single regression number |

### New CLI features

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| C1 | `--validate` flag | `main.rs`, `config.rs` | S | Parse config, print derived variables, check overlay file existence, then exit without loading the image.  Useful for iterating on `.cfg` files |
| C2 | Batch mode | `main.rs` | M | `--batch <cfg> <input_dir/> <output_dir/>` — glob input files, process in parallel with rayon, reuse parsed `Config` |
| C4 | 16-bit output correctness | `image_buf.rs` | S | `OFORMAT=1` is wired but the 16-bit quantisation path needs a test confirming values use the full 0–65535 range |

---

## Phase B — Video

Video support requires:

1. **Codec I/O layer** — pipe rawvideo frames through `ffmpeg -f rawvideo` as
   stdin/stdout; avoids a native H.264 decoder dependency.

2. **Frame-pipeline interface** — traits for streaming frames in and out:
   ```rust
   pub trait FrameSource: Iterator<Item = Result<ImgF32>> {}
   pub trait FrameSink { fn write(&mut self, frame: &ImgF32) -> Result<()>; }
   ```

3. **Temporal mixer** — holds the previous N frames in a ring buffer to
   implement `LATENCY` (frame mixing via `tmix`) and `P_DECAY_FACTOR` (p7
   phosphor decay via `lagfun`).

4. **Per-frame pipeline** — the existing `step01`–`step03` + `output` stages
   are already stateless and can be called per frame once the I/O layer is
   wired up.

### Phase B milestones

| Milestone | Deliverable |
|-----------|-------------|
| B-0 | Add `src/video.rs` with `FrameSource`/`FrameSink` trait definitions and `TemporalMixer` struct; unit-test the mixing logic without codec I/O |
| B-1 | Implement `FfmpegFrameSource` / `FfmpegFrameSink` using `std::process::Command` pipes; validate round-trip on a short clip |
| B-2 | Wire `LATENCY`/`LATENCY_ALPHA` through `TemporalMixer` in the pipeline |
| B-3 | Wire `P_DECAY_FACTOR`/`P_DECAY_ALPHA` for the p7 phosphor-decay path |
| B-4 | End-to-end comparison against `ffcrt.sh` output on test clips |

---

## Out of scope (won't implement)

- Native H.264 encode/decode (use ffmpeg as I/O codec instead)
- Additional blend modes beyond the four used by the pipeline
- Real-time preview / GUI
- WebAssembly target (no rayon; would need single-threaded rewrite)
