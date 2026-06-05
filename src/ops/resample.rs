//! Separable resampling with selectable kernels.
//!
//! Hand-written (rather than pulling a resize crate) so we can match the
//! specific ffmpeg `scale` flags the script uses: `neighbor`, `fast_bilinear`,
//! `bilinear`, `bicubic`, `lanczos`, `gauss`. Mapping is center-aligned
//! (`src = (dst+0.5)/scale - 0.5`); on downscale the kernel widens by `1/scale`
//! for anti-aliasing, as swscale does.

use crate::image_buf::ImgF32;

#[derive(Clone, Copy, PartialEq)]
pub enum Filter {
    Neighbor,
    FastBilinear,
    Bilinear,
    Bicubic,
    Lanczos,
    Gauss,
}

impl Filter {
    /// Parse an `OFILTER` string; unknown values fall back to bicubic (a safe
    /// general-purpose default, matching swscale's behaviour for odd names).
    pub fn parse(s: &str) -> Filter {
        match s.to_ascii_lowercase().as_str() {
            "neighbor" => Filter::Neighbor,
            "fast_bilinear" => Filter::FastBilinear,
            "bilinear" => Filter::Bilinear,
            "lanczos" => Filter::Lanczos,
            "gauss" => Filter::Gauss,
            _ => Filter::Bicubic,
        }
    }

    fn support(&self) -> f64 {
        match self {
            Filter::Neighbor => 0.5,
            Filter::FastBilinear | Filter::Bilinear => 1.0,
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
            Filter::Gauss => 2.0,
        }
    }

    fn weight(&self, t: f64) -> f64 {
        let t = t.abs();
        match self {
            Filter::Neighbor => {
                if t < 0.5 { 1.0 } else { 0.0 }
            }
            Filter::FastBilinear | Filter::Bilinear => {
                if t < 1.0 { 1.0 - t } else { 0.0 }
            }
            Filter::Bicubic => cubic(t, -0.5),
            Filter::Lanczos => lanczos(t, 3.0),
            Filter::Gauss => {
                // ffmpeg's gauss scaler is a gaussian; exp(-2 t^2) over support 2.
                if t < 2.0 { (-2.0 * t * t).exp() } else { 0.0 }
            }
        }
    }
}

fn cubic(t: f64, a: f64) -> f64 {
    // Keys cubic (a=-0.5 ≈ Catmull-Rom)
    if t < 1.0 {
        (a + 2.0) * t * t * t - (a + 3.0) * t * t + 1.0
    } else if t < 2.0 {
        a * t * t * t - 5.0 * a * t * t + 8.0 * a * t - 4.0 * a
    } else {
        0.0
    }
}

fn lanczos(t: f64, a: f64) -> f64 {
    if t == 0.0 {
        1.0
    } else if t < a {
        let pt = std::f64::consts::PI * t;
        a * (pt.sin() * (pt / a).sin()) / (pt * pt)
    } else {
        0.0
    }
}

struct Contrib {
    start: usize,
    weights: Vec<f32>,
}

fn build_contribs(src_size: usize, dst_size: usize, filter: Filter) -> Vec<Contrib> {
    let scale = dst_size as f64 / src_size as f64;
    let filter_scale = if scale < 1.0 { 1.0 / scale } else { 1.0 };
    let support = filter.support() * filter_scale;
    let mut out = Vec::with_capacity(dst_size);
    for d in 0..dst_size {
        let center = (d as f64 + 0.5) / scale - 0.5;
        let left = (center - support).ceil() as i64;
        let right = (center + support).floor() as i64;
        let mut weights = Vec::new();
        let mut sum = 0.0f64;
        let start = left.max(0) as usize;
        for s in left..=right {
            let sc = s.clamp(0, src_size as i64 - 1) as usize;
            // accumulate weight at the clamped sample (edge clamp)
            let w = filter.weight((s as f64 - center) / filter_scale);
            if sc as i64 == s {
                weights.push(w as f32);
            } else {
                // out-of-range: fold weight onto nearest edge sample we already
                // emitted (or will emit) by clamping; simplest is to clamp index.
                weights.push(w as f32);
            }
            sum += w;
        }
        if sum != 0.0 {
            for w in &mut weights {
                *w = (*w as f64 / sum) as f32;
            }
        }
        // Re-clamp the start so all indices are valid; we clamp per-sample below.
        out.push(Contrib { start: start.min(src_size.saturating_sub(1)), weights });
        // store the true left for indexing
        out.last_mut().unwrap().start = if left < 0 { 0 } else { left as usize };
    }
    out
}

fn resample_axis(
    src: &[f32],
    src_w: usize,
    src_h: usize,
    dst_w: usize,
    horizontal: bool,
    contribs: &[Contrib],
) -> Vec<f32> {
    let (dst_h, out_w, out_h) = if horizontal {
        (src_h, dst_w, src_h)
    } else {
        (dst_w, src_w, dst_w) // when vertical, dst_w is the new height
    };
    let mut out = vec![0.0f32; out_w * out_h * 4];
    if horizontal {
        for y in 0..src_h {
            for x in 0..dst_w {
                let c = &contribs[x];
                let mut acc = [0.0f32; 4];
                for (i, &w) in c.weights.iter().enumerate() {
                    let sx = (c.start + i).min(src_w - 1);
                    let si = (y * src_w + sx) * 4;
                    acc[0] += src[si] * w;
                    acc[1] += src[si + 1] * w;
                    acc[2] += src[si + 2] * w;
                    acc[3] += src[si + 3] * w;
                }
                let di = (y * dst_w + x) * 4;
                out[di..di + 4].copy_from_slice(&acc);
            }
        }
    } else {
        let new_h = dst_w; // contribs indexed by destination row
        let _ = dst_h;
        for y in 0..new_h {
            let c = &contribs[y];
            for x in 0..src_w {
                let mut acc = [0.0f32; 4];
                for (i, &w) in c.weights.iter().enumerate() {
                    let sy = (c.start + i).min(src_h - 1);
                    let si = (sy * src_w + x) * 4;
                    acc[0] += src[si] * w;
                    acc[1] += src[si + 1] * w;
                    acc[2] += src[si + 2] * w;
                    acc[3] += src[si + 3] * w;
                }
                let di = (y * src_w + x) * 4;
                out[di..di + 4].copy_from_slice(&acc);
            }
        }
    }
    out
}

/// Resize to exactly `new_w` x `new_h` using `filter`. Sizes of 0 are clamped
/// to 1. A no-op size returns a clone.
pub fn resize(img: &ImgF32, new_w: usize, new_h: usize, filter: Filter) -> ImgF32 {
    let new_w = new_w.max(1);
    let new_h = new_h.max(1);
    if new_w == img.w && new_h == img.h {
        return img.clone();
    }
    // horizontal pass
    let hcontribs = build_contribs(img.w, new_w, filter);
    let tmp = resample_axis(&img.data, img.w, img.h, new_w, true, &hcontribs);
    // vertical pass on the width-resized buffer
    let vcontribs = build_contribs(img.h, new_h, filter);
    let out = resample_axis(&tmp, new_w, img.h, new_h, false, &vcontribs);
    ImgF32 { w: new_w, h: new_h, data: out }
}

/// Integer nearest-neighbor upscale by independent x/y factors (the `neighbor`
/// prescale passes). Exact pixel replication, no interpolation.
pub fn nearest_scale(img: &ImgF32, fx: usize, fy: usize) -> ImgF32 {
    if fx == 1 && fy == 1 {
        return img.clone();
    }
    let nw = img.w * fx;
    let nh = img.h * fy;
    let mut out = ImgF32::new(nw, nh);
    for y in 0..nh {
        let sy = y / fy;
        for x in 0..nw {
            let sx = x / fx;
            let p = img.get(sx, sy);
            out.set(x, y, p);
        }
    }
    out
}
