//! Separable resampling with selectable kernels.
//!
//! Hand-written (rather than pulling a resize crate) so we can match the
//! specific ffmpeg `scale` flags the script uses: `neighbor`, `fast_bilinear`,
//! `bilinear`, `bicubic`, `lanczos`, `gauss`. Mapping is center-aligned
//! (`src = (dst+0.5)/scale - 0.5`); on downscale the kernel widens by `1/scale`
//! for anti-aliasing, as swscale does.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

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
    let (out_w, out_h) = if horizontal {
        (dst_w, src_h)
    } else {
        (src_w, dst_w)
    };
    let mut out = vec![0.0f32; out_w * out_h * 4];
    if horizontal {
        let dst_row_stride = dst_w * 4;
        let src_row_stride = src_w * 4;
        out.par_chunks_exact_mut(dst_row_stride)
            .enumerate()
            .for_each(|(y, row_out)| {
                let src_row = &src[y * src_row_stride..(y + 1) * src_row_stride];
                for x in 0..dst_w {
                    let c = &contribs[x];
                    let mut acc = [0.0f32; 4];
                    for (i, &w) in c.weights.iter().enumerate() {
                        let sx = (c.start + i).min(src_w - 1);
                        let si = sx * 4;
                        acc[0] += src_row[si] * w;
                        acc[1] += src_row[si + 1] * w;
                        acc[2] += src_row[si + 2] * w;
                        acc[3] += src_row[si + 3] * w;
                    }
                    let di = x * 4;
                    row_out[di..di + 4].copy_from_slice(&acc);
                }
            });
    } else {
        let row_stride = src_w * 4;
        // Optimized vertical resample: process destination rows in parallel.
        // For each destination row, we accumulate contributions from all source
        // rows that fall within the filter kernel. This ensures we process each
        // row linearly, significantly improving cache locality.
        out.par_chunks_exact_mut(row_stride)
            .enumerate()
            .for_each(|(dy, row_out)| {
                let c = &contribs[dy];
                for (i, &w) in c.weights.iter().enumerate() {
                    let sy = (c.start + i).min(src_h - 1);
                    let src_row = &src[sy * row_stride..(sy + 1) * row_stride];
                    for (o, &s) in row_out.iter_mut().zip(src_row.iter()) {
                        *o += s * w;
                    }
                }
            });
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
    let row_stride = nw * 4;
    out.data.par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let sy = y / fy;
            for x in 0..nw {
                let sx = x / fx;
                let p = img.get(sx, sy);
                let di = x * 4;
                row[di..di + 4].copy_from_slice(&p);
            }
        });
    out
}
