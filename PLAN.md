# crt-transform — Development Plan

## Current state (Phase A complete)

Still-image pipeline is complete and passing all tests.  The binary produces
perceptually correct output for all 16 presets.  Performance is 3–11× faster
than the ffcrt.sh reference on a 6-core machine.

---

## Remaining Phase A work

### Performance

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| A2 | Cache Gaussian kernels | `pipeline.rs`, `blur.rs` | S | `kernel(sigma)` is called 3–4× per run with the same sigma; compute once at pipeline start |
| A4 | `ImgF32::for_each_row_mut` helper | `image_buf.rs`, all `ops/` | M | Extract the repeated `par_chunks_exact_mut` idiom; ~10 call sites to clean up |
| A5 | Blur edge-loop consolidation | `ops/blur.rs` | S | `blur_h`/`blur_v` have three nearly-identical loops; unify with a clamped-index helper |

### Code quality / DX

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| B1 | Config range validation | `config.rs` | S | Add `Config::validate() -> Vec<Warning>`; check `PRESCALE_BY ≥ 1`, alpha ranges 0..1, known `OFILTER` values; emit warnings, not hard errors |
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
