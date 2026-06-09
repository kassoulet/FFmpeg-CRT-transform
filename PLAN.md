# crt-transform — Development Plan

## Current state (Phase A + CLI complete)

Still-image pipeline is complete and all tests pass.  The binary handles
single images, batch directories, config validation, and stage dumping.
Performance is 3–11× faster than the ffcrt.sh reference on a 6-core machine.
74 unit tests · 12 integration tests · pipeline benchmark: **306 ms** (640×480, color CRT preset).

---

## Completed items

| ID  | Item                                  | Commit   |
|-----|---------------------------------------|----------|
| A1  | Gamma LUT (4096-entry, ~10× speedup)  | ee03c47  |
| A2  | Gaussian kernel caching               | 814b9f0  |
| A3  | Eliminate step02.clone() in step03    | ee03c47  |
| A4  | ImgF32::for_each_row_mut helper       | ee03c47  |
| A5  | Blur edge-loop consolidation + dynamic gather buffer | 1246755 / fc6e54f |
| B1  | Config::validate() with range warnings | 5b4ca50 |
| B2  | Early overlay-file existence check    | ee03c47  |
| B3  | Progress callback in run()            | ee03c47  |
| B4  | Monitor profile unit tests            | ee03c47  |
| B5  | Expanded bench coverage               | ee03c47  |
| B6  | Integration tests — one per preset family | cdfd5a4 |
| B7  | bench_full_pipeline (306 ms baseline) | 7c812e7  |
| C1  | --validate flag                       | a742e94  |
| C2  | --batch mode (parallel, rayon)        | d691b56  |
| C4  | 16-bit output correctness test        | adacbd5  |

> **SIMD note:** hand-written 128-bit SSE2 intrinsics were 40% *slower* than
> the scalar loops LLVM auto-vectorizes to AVX2+FMA (256-bit) via
> `target-cpu=native`.  Scalar `accum_rgba` + `saxpy` was kept; the compiler
> wins.  See `fc6e54f`.

---

## Next tasks

### D — Developer experience

| ID | Item | Files | Effort | Notes |
|----|------|-------|--------|-------|
| ~~D1~~ | ~~Gate slow integration tests~~ | — | Done — 8ba770b (18 min → ~98 s debug / ~15 s release) |
| ~~D2~~ | ~~Fast color/mono/amber configs~~ | — | Done — 8ba770b |

### Phase B — Video

Phase B requires three independent components that can be built in order:

#### B-0 · Video framing interface (no codec I/O)

Add `src/video.rs` with the types needed for temporal mixing:

```rust
pub trait FrameSource: Iterator<Item = Result<ImgF32>> {}
pub trait FrameSink { fn write(&mut self, frame: &ImgF32) -> Result<()>; }

pub struct TemporalMixer {
    latency: usize,          // LATENCY: ring-buffer of N past frames
    decay_factor: f32,       // P_DECAY_FACTOR: exponential phosphor trail
    decay_alpha: f32,        // P_DECAY_ALPHA: blend weight
    ring: VecDeque<ImgF32>,  // held frames
}
impl TemporalMixer {
    pub fn mix(&mut self, frame: ImgF32) -> ImgF32 { ... }
}
```

Unit-test the mixing logic (latency blending, phosphor decay convergence)
without touching any codec.  This unblocks B-2 and B-3 immediately.

| ID  | Item | Files | Effort | Notes |
|-----|------|-------|--------|-------|
| B-0 | FrameSource/FrameSink traits + TemporalMixer | `src/video.rs` | M | Pure Rust, no subprocess. Unit tests for latency ring-buffer and decay convergence. No CLI wiring yet. |
| B-1 | FfmpegFrameSource + FfmpegFrameSink | `src/video.rs` | M | Spawn `ffmpeg -f rawvideo` as a child process; pipe raw RGBA frames in/out. Validate round-trip on a 10-frame synthetic clip. |
| B-2 | Wire LATENCY into pipeline | `src/pipeline.rs`, `src/video.rs` | S | Thread `TemporalMixer` through the per-frame pipeline loop; LATENCY frames held in ring buffer. |
| B-3 | Wire P_DECAY_FACTOR (p7 phosphor decay) | `src/pipeline.rs` | S | Apply exponential decay trail from TemporalMixer on the p7 path. Matches the `lagfun` filter in ffcrt.sh. |
| B-4 | End-to-end video test | `tests/` | M | Run on a short test clip; compare frame-level output against `ffcrt.sh` reference (PSNR ≥ 35 dB). |

---

## Out of scope (won't implement)

- Native H.264 encode/decode (use ffmpeg as I/O codec instead)
- Additional blend modes beyond the four used by the pipeline
- Real-time preview / GUI
- WebAssembly target (no rayon; would need single-threaded rewrite)
