# crt-transform — Development Plan

## Current state (Phase A + B + CLI complete)

Still-image and video pipelines are complete.  The binary handles single
images, batch directories, video files (via ffmpeg subprocess), config
validation, and stage dumping.  Performance is 3–11× faster than the
ffcrt.sh reference on a 6-core machine.
88 unit tests · 13 integration tests · pipeline benchmark: **306 ms** (640×480, color CRT preset).

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
| C3  | Config::to_map() + Derived::report()  | 0bce60f  |

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

| ID  | Item | Files | Status |
|-----|------|-------|--------|
| ~~B-0~~ | ~~FrameSource/FrameSink traits + TemporalMixer~~ | `src/video.rs` | Done — aaaf068 |
| ~~B-1~~ | ~~FfmpegFrameSource + FfmpegFrameSink~~ | `src/video.rs` | Done — 973790a (round-trip test: 8 frames, luma error < 1%) |
| ~~B-2~~ | ~~Wire LATENCY into pipeline~~ | `src/pipeline.rs` | Done — bundled with B-3 |
| ~~B-3~~ | ~~Wire P_DECAY_FACTOR (p7 phosphor decay)~~ | `src/pipeline.rs` | Done — `run_video_inner()` applies `TemporalMixer.mix()` per frame |
| ~~B-4~~ | ~~End-to-end video test~~ | `tests/` | Done — a65a084 (`video_p_decay_trail_end_to_end`: step video, decay trail YAVG > 5 verified via ffmpeg signalstats, ~12 s) |

---

---

## Phase C — Polish / remaining known gaps

| ID | Item | Notes |
|----|------|-------|
| C5 | p7 video temporal path | ffcrt.sh uses two lagfun passes + separate MONOCURVES_LAT / MONOCURVES_DEC per-frame; the still-image path is correct but the video version diverges from the reference |
| C6 | Video frame-pipeline parallelism | Decode/process/encode stages run serially; pipelining would improve throughput on long clips |
| C7 | Video fidelity PSNR test | Measure frame-level PSNR vs ffcrt.sh on a known short clip (target ≥ 25 dB; known divergences are documented in CLAUDE.md) |

---

## Out of scope (won't implement)

- Native H.264 encode/decode (use ffmpeg as I/O codec instead)
- Additional blend modes beyond the four used by the pipeline
- Real-time preview / GUI
- WebAssembly target (no rayon; would need single-threaded rewrite)
