//! Video framing interface and temporal mixing.
//!
//! Phase B skeleton — codec I/O (B-1) is not yet implemented.
//! [`TemporalMixer`] implements the two temporal effects from ffcrt.sh:
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
use anyhow::Result;
use std::collections::VecDeque;

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
}
