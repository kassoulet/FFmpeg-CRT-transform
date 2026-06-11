//! Video framing interface and temporal mixing.
//!
//! [`FfmpegFrameSource`] and [`FfmpegFrameSink`] pipe raw RGBA frames through
//! an ffmpeg subprocess (B-1).  [`TemporalMixer`] implements LATENCY (tmix)
//! and P_DECAY (lagfun) on top of those frames.
//!
//! - **LATENCY / tmix**: temporal average of the last N frames, blended back
//!   into the current frame at `LATENCY_ALPHA`.
//!   Formula: `output = (1 − α) · mean(ring) + α · current`
//!
//! - **P_DECAY / lagfun**: exponential phosphor-persistence trail — each
//!   pixel's state decays by `P_DECAY_FACTOR` per frame; brighter new pixels
//!   override the trail.  Blended into the output at `P_DECAY_ALPHA`.
//!   Formula: `state[t] = max(current, factor · state[t-1])`;
//!   `output = (1 − α) · current + α · state`
//!
//! Both effects are disabled when their respective factor/alpha are zero and
//! compose additively (latency is applied first, then decay).

use crate::image_buf::ImgF32;
use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

// ---------------------------------------------------------------------------
// Traits (B-1 will provide concrete implementations)
// ---------------------------------------------------------------------------

/// Yields decoded video frames one at a time, in display order.
pub trait FrameSource: Iterator<Item = Result<ImgF32>> {}

/// Accepts processed frames for encoding or writing.
pub trait FrameSink {
    fn write(&mut self, frame: &ImgF32) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Video metadata
// ---------------------------------------------------------------------------

/// Basic metadata returned by [`probe_video`].
#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub w: usize,
    pub h: usize,
    /// Numerator of the frame-rate rational.
    pub fps_num: u32,
    /// Denominator of the frame-rate rational.
    pub fps_den: u32,
}

impl VideoInfo {
    /// Frame rate as an `f64`.
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den as f64
    }
}

/// Query width, height, and frame-rate from a video file using `ffprobe`.
pub fn probe_video(path: &Path) -> Result<VideoInfo> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate",
            "-of",
            "csv=p=0",
            path.to_str().context("non-UTF-8 path")?,
        ])
        .output()
        .context("ffprobe not found — is ffmpeg installed?")?;

    anyhow::ensure!(
        out.status.success(),
        "ffprobe failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = String::from_utf8(out.stdout).context("ffprobe output not UTF-8")?;
    let text = text.trim();
    // Expected format: "WIDTH,HEIGHT,NUM/DEN"
    let mut parts = text.splitn(3, ',');
    let w: usize = parts.next().context("missing width")?.parse()?;
    let h: usize = parts.next().context("missing height")?.parse()?;
    let fps_str = parts.next().context("missing frame rate")?;
    let mut fps_parts = fps_str.splitn(2, '/');
    let fps_num: u32 = fps_parts.next().context("missing fps numerator")?.parse()?;
    let fps_den: u32 = fps_parts.next().unwrap_or("1").trim().parse().unwrap_or(1);

    Ok(VideoInfo {
        w,
        h,
        fps_num,
        fps_den,
    })
}

// ---------------------------------------------------------------------------
// FfmpegFrameSource
// ---------------------------------------------------------------------------

/// Decodes a video file frame-by-frame, yielding raw RGBA [`ImgF32`] frames.
///
/// Spawns `ffmpeg -i <path> -f rawvideo -pix_fmt rgba -` and reads frames
/// from stdout.  Call [`probe_video`] first to inspect metadata without
/// opening the pipe.
pub struct FfmpegFrameSource {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    w: usize,
    h: usize,
    frame_bytes: Vec<u8>,
    done: bool,
}

impl FfmpegFrameSource {
    /// Open `path` for sequential frame decoding.
    pub fn open(path: &Path) -> Result<Self> {
        let info = probe_video(path)?;
        Self::open_with_info(path, &info, None)
    }

    /// Open with pre-probed [`VideoInfo`] (avoids a second ffprobe call).
    ///
    /// `duration_secs`: if `Some`, stop decoding after that many seconds (`-t`).
    pub fn open_with_info(
        path: &Path,
        info: &VideoInfo,
        duration_secs: Option<f64>,
    ) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        if let Some(dur) = duration_secs {
            cmd.arg("-t").arg(dur.to_string());
        }
        cmd.arg("-i")
            .arg(path.to_str().context("non-UTF-8 path")?)
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("ffmpeg not found — is ffmpeg installed?")?;

        let stdout = child.stdout.take().context("no stdout from ffmpeg")?;
        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            w: info.w,
            h: info.h,
            frame_bytes: vec![0u8; info.w * info.h * 4],
            done: false,
        })
    }
}

impl Iterator for FfmpegFrameSource {
    type Item = Result<ImgF32>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match self.reader.read_exact(&mut self.frame_bytes) {
            Ok(()) => {
                let mut img = ImgF32::new(self.w, self.h);
                for (dst, src) in img
                    .data
                    .chunks_exact_mut(4)
                    .zip(self.frame_bytes.chunks_exact(4))
                {
                    dst[0] = src[0] as f32 / 255.0;
                    dst[1] = src[1] as f32 / 255.0;
                    dst[2] = src[2] as f32 / 255.0;
                    dst[3] = src[3] as f32 / 255.0;
                }
                Some(Ok(img))
            }
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e.into()))
            }
        }
    }
}

impl FrameSource for FfmpegFrameSource {}

impl Drop for FfmpegFrameSource {
    fn drop(&mut self) {
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// FfmpegFrameSink
// ---------------------------------------------------------------------------

/// Encodes RGBA [`ImgF32`] frames into a video file via ffmpeg.
///
/// Spawns `ffmpeg -f rawvideo -pix_fmt rgba -video_size WxH -framerate R -i
/// pipe:0 <output>` and writes frames to stdin.  Call [`finish`] after the
/// last frame to flush and wait for ffmpeg to exit cleanly.
///
/// [`finish`]: FfmpegFrameSink::finish
pub struct FfmpegFrameSink {
    child: Child,
    stdin: Option<ChildStdin>,
    w: usize,
    h: usize,
    row_buf: Vec<u8>,
}

impl FfmpegFrameSink {
    /// Create `path` as a video file.  Frame dimensions and frame rate must
    /// match every frame passed to [`write`].
    ///
    /// `crf`: libx264 Constant Rate Factor (0 = lossless, 14 = perceptually
    /// lossless, 23 = default).  Output is always `yuv444p` (no chroma
    /// subsampling) with the `high444` profile.
    ///
    /// [`write`]: FrameSink::write
    pub fn create(
        path: &Path,
        w: usize,
        h: usize,
        fps_num: u32,
        fps_den: u32,
        crf: u32,
    ) -> Result<Self> {
        let fps_str = if fps_den == 1 {
            fps_num.to_string()
        } else {
            format!("{fps_num}/{fps_den}")
        };
        let size_str = format!("{w}x{h}");
        let crf_str = crf.to_string();

        let mut child = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-video_size",
                &size_str,
                "-framerate",
                &fps_str,
                "-i",
                "pipe:0",
                "-c:v",
                "libx264",
                "-profile:v",
                "high444",
                "-crf",
                &crf_str,
                "-preset",
                "slow",
                "-pix_fmt",
                "yuv444p",
                path.to_str().context("non-UTF-8 path")?,
            ])
            .stdin(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("ffmpeg not found — is ffmpeg installed?")?;

        let stdin = child.stdin.take().context("no stdin for ffmpeg")?;
        Ok(Self {
            child,
            stdin: Some(stdin),
            w,
            h,
            row_buf: vec![0u8; w * h * 4],
        })
    }

    /// Flush stdin, wait for ffmpeg to exit, and return an error if it failed.
    /// Must be called after the last [`write`] to ensure the file is finalised.
    ///
    /// [`write`]: FrameSink::write
    pub fn finish(mut self) -> Result<()> {
        // Drop stdin so ffmpeg sees EOF on its input pipe, then wait.
        self.stdin.take();
        let status = self.child.wait().context("waiting for ffmpeg")?;
        anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
        Ok(())
    }
}

impl FrameSink for FfmpegFrameSink {
    fn write(&mut self, frame: &ImgF32) -> Result<()> {
        anyhow::ensure!(
            frame.w == self.w && frame.h == self.h,
            "frame size {}x{} != expected {}x{}",
            frame.w,
            frame.h,
            self.w,
            self.h
        );
        for (dst, src) in self
            .row_buf
            .chunks_exact_mut(4)
            .zip(frame.data.chunks_exact(4))
        {
            dst[0] = (src[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            dst[1] = (src[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            dst[2] = (src[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            dst[3] = (src[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        if let Some(ref mut stdin) = self.stdin {
            stdin
                .write_all(&self.row_buf)
                .context("writing frame to ffmpeg")?;
        }
        Ok(())
    }
}

impl Drop for FfmpegFrameSink {
    fn drop(&mut self) {
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// TemporalMixer
// ---------------------------------------------------------------------------

/// Applies LATENCY (tmix) and P_DECAY (lagfun) temporal effects per frame.
///
/// Construct with [`TemporalMixer::new`], then call [`mix`] once per decoded
/// frame in display order.  The mixer is stateful; resetting it loses the
/// accumulated ring buffer and decay trail.
pub struct TemporalMixer {
    latency: usize,
    latency_alpha: f32,
    ring: VecDeque<ImgF32>,
    decay_factor: f32,
    decay_alpha: f32,
    decay_state: Option<ImgF32>,
}

impl TemporalMixer {
    /// Construct a new mixer.
    ///
    /// - `latency`: number of frames to average (0 = disabled).
    /// - `latency_alpha`: blend weight for tmix result (0–1).
    /// - `decay_factor`: per-frame phosphor-decay multiplier (0–1, 0 = disabled).
    /// - `decay_alpha`: blend weight for the lagfun trail (0–1).
    pub fn new(latency: usize, latency_alpha: f32, decay_factor: f32, decay_alpha: f32) -> Self {
        Self {
            latency,
            latency_alpha: latency_alpha.clamp(0.0, 1.0),
            ring: VecDeque::with_capacity(latency.max(1)),
            decay_factor: decay_factor.clamp(0.0, 1.0),
            decay_alpha: decay_alpha.clamp(0.0, 1.0),
            decay_state: None,
        }
    }

    /// Apply temporal effects to `frame` and return the mixed output.
    ///
    /// Latency (tmix) is applied before decay (lagfun), matching ffcrt.sh's
    /// filter graph order.
    pub fn mix(&mut self, frame: ImgF32) -> ImgF32 {
        let after_latency = self.apply_latency(frame);
        self.apply_decay(after_latency)
    }

    /// Reset internal state (ring buffer + decay trail).  Use when seeking.
    pub fn reset(&mut self) {
        self.ring.clear();
        self.decay_state = None;
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// tmix: blend current frame with the temporal mean of the last N frames.
    ///
    /// ffcrt.sh: `[2lat] tmix=N → [lat][orig] blend=all_opacity=LATENCY_ALPHA`
    /// = `(1 − alpha) * mean(ring) + alpha * current`
    fn apply_latency(&mut self, frame: ImgF32) -> ImgF32 {
        if self.latency == 0 || self.latency_alpha >= 1.0 {
            return frame;
        }

        // Maintain ring of the last `latency` frames.
        self.ring.push_back(frame.clone());
        if self.ring.len() > self.latency {
            self.ring.pop_front();
        }

        // Not enough history yet — pass through.
        if self.ring.len() < 2 {
            return frame;
        }

        // Per-pixel mean of all ring frames.
        let inv_n = 1.0 / self.ring.len() as f32;
        let mut mean_data = vec![0.0f32; frame.data.len()];
        for rf in &self.ring {
            for (m, &s) in mean_data.iter_mut().zip(rf.data.iter()) {
                *m += s * inv_n;
            }
        }

        // Blend: (1 - alpha)*mean + alpha*current
        let a = self.latency_alpha;
        let mut out = ImgF32::new(frame.w, frame.h);
        for (o, (&m, &c)) in out
            .data
            .iter_mut()
            .zip(mean_data.iter().zip(frame.data.iter()))
        {
            *o = (1.0 - a) * m + a * c;
        }
        out
    }

    /// lagfun: exponential phosphor-persistence trail.
    ///
    /// ffcrt.sh: `lagfun=P_DECAY_FACTOR` → `[orig][lag] blend=lighten:P_DECAY_ALPHA`
    /// State: `state[t] = max(current, decay * state[t-1])`
    /// Output: `(1 − alpha) * current + alpha * state`
    /// (state ≥ current by construction, so this is a lighten blend.)
    fn apply_decay(&mut self, frame: ImgF32) -> ImgF32 {
        if self.decay_factor <= 0.0 || self.decay_alpha <= 0.0 {
            return frame;
        }

        let decay = self.decay_factor;
        let new_state = match &self.decay_state {
            None => frame.clone(),
            Some(prev) => {
                let mut s = ImgF32::new(frame.w, frame.h);
                for (sv, (&fv, &pv)) in s
                    .data
                    .iter_mut()
                    .zip(frame.data.iter().zip(prev.data.iter()))
                {
                    *sv = fv.max(pv * decay);
                }
                s
            }
        };

        let a = self.decay_alpha;
        let mut out = ImgF32::new(frame.w, frame.h);
        for (o, (&c, &s)) in out
            .data
            .iter_mut()
            .zip(frame.data.iter().zip(new_state.data.iter()))
        {
            *o = (1.0 - a) * c + a * s;
        }

        self.decay_state = Some(new_state);
        out
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_frame(w: usize, h: usize, v: f32) -> ImgF32 {
        ImgF32::filled(w, h, [v, v, v, 1.0])
    }

    fn mean_luma(img: &ImgF32) -> f32 {
        let sum: f32 = img.data.chunks_exact(4).map(|px| px[0]).sum();
        sum / (img.w * img.h) as f32
    }

    // --- identity when disabled ---

    #[test]
    fn disabled_mixer_is_identity() {
        let mut m = TemporalMixer::new(0, 0.0, 0.0, 0.0);
        let frame = gray_frame(4, 4, 0.7);
        let out = m.mix(frame);
        assert!((mean_luma(&out) - 0.7).abs() < 1e-5);
    }

    #[test]
    fn latency_alpha_one_is_identity() {
        // alpha=1.0 → output = current frame regardless of history
        let mut m = TemporalMixer::new(5, 1.0, 0.0, 0.0);
        for _ in 0..3 {
            m.mix(gray_frame(4, 4, 0.2));
        }
        let out = m.mix(gray_frame(4, 4, 0.8));
        assert!((mean_luma(&out) - 0.8).abs() < 1e-5);
    }

    // --- latency (tmix) ---

    #[test]
    fn latency_ring_fills_gradually() {
        // With only 1 frame in the ring, output equals the current frame.
        let mut m = TemporalMixer::new(4, 0.3, 0.0, 0.0);
        let out = m.mix(gray_frame(4, 4, 0.5));
        assert!((mean_luma(&out) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn latency_converges_on_constant_input() {
        // After N identical frames the mean equals the frame value,
        // so output = (1-alpha)*v + alpha*v = v.
        let mut m = TemporalMixer::new(4, 0.3, 0.0, 0.0);
        let mut out = gray_frame(1, 1, 0.0);
        for _ in 0..8 {
            out = m.mix(gray_frame(4, 4, 0.6));
        }
        assert!(
            (mean_luma(&out) - 0.6).abs() < 1e-4,
            "luma={}",
            mean_luma(&out)
        );
    }

    #[test]
    fn latency_blurs_a_step_change() {
        // Feed 4 dark frames, then one bright frame; the output should be
        // between the two extremes (temporal smoothing visible).
        let mut m = TemporalMixer::new(4, 0.5, 0.0, 0.0);
        for _ in 0..4 {
            m.mix(gray_frame(4, 4, 0.0));
        }
        let out = m.mix(gray_frame(4, 4, 1.0));
        let luma = mean_luma(&out);
        assert!(luma > 0.0 && luma < 1.0, "expected blur, got {luma}");
    }

    // --- decay (lagfun) ---

    #[test]
    fn decay_persists_after_bright_flash() {
        // One bright frame, then dark frames; the trail should persist.
        let mut m = TemporalMixer::new(0, 0.0, 0.9, 0.5);
        m.mix(gray_frame(4, 4, 1.0)); // bright flash
        let out = m.mix(gray_frame(4, 4, 0.0)); // dark follow-up
                                                // state[1] = max(0, 0.9 * 1.0) = 0.9
                                                // output  = (1-0.5)*0 + 0.5*0.9 = 0.45
        assert!(
            (mean_luma(&out) - 0.45).abs() < 1e-4,
            "luma={}",
            mean_luma(&out)
        );
    }

    #[test]
    fn decay_trail_shrinks_geometrically() {
        // After a bright flash, each subsequent dark frame should decay by factor.
        let factor = 0.8f32;
        let alpha = 1.0f32; // full trail for easy math
        let mut m = TemporalMixer::new(0, 0.0, factor, alpha);
        m.mix(gray_frame(4, 4, 1.0)); // flash
        let mut prev = 1.0f32;
        for _ in 0..5 {
            let out = m.mix(gray_frame(4, 4, 0.0));
            let luma = mean_luma(&out);
            let expected = prev * factor;
            assert!(
                (luma - expected).abs() < 1e-4,
                "expected {expected}, got {luma}"
            );
            prev = luma;
        }
    }

    #[test]
    fn decay_bright_frame_overrides_trail() {
        // A frame brighter than the decayed trail should win.
        let mut m = TemporalMixer::new(0, 0.0, 0.9, 1.0);
        m.mix(gray_frame(4, 4, 0.5)); // moderate frame
                                      // state = 0.5; next frame = 1.0 > 0.9*0.5 = 0.45
        let out = m.mix(gray_frame(4, 4, 1.0));
        assert!((mean_luma(&out) - 1.0).abs() < 1e-4);
    }

    // --- reset ---

    #[test]
    fn reset_clears_decay_state() {
        let mut m = TemporalMixer::new(0, 0.0, 0.9, 0.5);
        m.mix(gray_frame(4, 4, 1.0)); // build up state
        m.reset();
        // After reset, first frame after a flash — but there's no previous state,
        // so state = current = 0.0; output = 0.0.
        let out = m.mix(gray_frame(4, 4, 0.0));
        assert!((mean_luma(&out) - 0.0).abs() < 1e-5);
    }

    #[test]
    fn reset_clears_latency_ring() {
        let mut m = TemporalMixer::new(4, 0.3, 0.0, 0.0);
        for _ in 0..4 {
            m.mix(gray_frame(4, 4, 1.0)); // fill ring with bright frames
        }
        m.reset();
        // After reset, ring is empty → output = current (pass-through on first frame)
        let out = m.mix(gray_frame(4, 4, 0.2));
        assert!((mean_luma(&out) - 0.2).abs() < 1e-5);
    }

    // --- composition ---

    #[test]
    fn both_effects_compose() {
        // Smoke test: both effects active, no panic, output in [0,1].
        let mut m = TemporalMixer::new(3, 0.4, 0.8, 0.3);
        for i in 0..10 {
            let v = (i as f32 / 9.0).clamp(0.0, 1.0);
            let out = m.mix(gray_frame(4, 4, v));
            let luma = mean_luma(&out);
            assert!(
                (0.0..=1.0).contains(&luma),
                "frame {i}: luma {luma} out of range"
            );
        }
    }

    // --- codec round-trip ---

    #[test]
    fn ffmpeg_roundtrip_preserves_frames() {
        // Write 8 synthetic frames to a .mkv via FfmpegFrameSink, read them
        // back with FfmpegFrameSource, and verify per-channel mean is within
        // the 8-bit quantisation error (≤ 0.004 ≈ 1/255).
        let tmp_path = std::env::temp_dir().join("crt-video-roundtrip-test.mkv");
        let _ = std::fs::remove_file(&tmp_path);

        let w = 8;
        let h = 8;
        let n_frames = 8;
        let fps_num = 25;
        let fps_den = 1;

        // Build test frames with distinct luma values.
        let frames: Vec<ImgF32> = (0..n_frames)
            .map(|i| gray_frame(w, h, i as f32 / (n_frames - 1) as f32))
            .collect();

        // Write.
        {
            let mut sink =
                crate::video::FfmpegFrameSink::create(&tmp_path, w, h, fps_num, fps_den, 0)
                    .expect("create sink");
            for f in &frames {
                sink.write(f).expect("write frame");
            }
            sink.finish().expect("finish sink");
        }

        assert!(tmp_path.exists(), "output video not created");

        // Read back.
        let source = crate::video::FfmpegFrameSource::open(&tmp_path).expect("open source");
        let decoded: Vec<ImgF32> = source.map(|r| r.expect("decode frame")).collect();

        assert_eq!(decoded.len(), n_frames, "frame count mismatch");

        for (i, (orig, dec)) in frames.iter().zip(decoded.iter()).enumerate() {
            let orig_luma = mean_luma(orig);
            let dec_luma = mean_luma(dec);
            assert!(
                (orig_luma - dec_luma).abs() < 0.01,
                "frame {i}: orig={orig_luma:.4} decoded={dec_luma:.4}"
            );
        }

        let _ = std::fs::remove_file(&tmp_path);
    }
}
