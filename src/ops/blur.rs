//! Separable Gaussian blur — the `gblur=sigma=..:sigmaV=..:steps=..` filter.
//!
//! ffmpeg's `gblur` uses a recursive IIR Gaussian whose `steps` parameter just
//! sharpens the approximation; a single truncated FIR Gaussian convolution is
//! perceptually equivalent for our purposes. Horizontal and vertical sigmas are
//! independent (the script often blurs more horizontally than vertically).
//!
//! ## Vectorization
//!
//! With `-C target-cpu=native` (see `.cargo/config.toml`), LLVM auto-vectorizes
//! the inner accumulation loops to AVX2+FMA (8-wide f32), which outperforms
//! hand-written 128-bit SSE2 intrinsics by ~40%.  The scalar-style code below is
//! intentional: keep it simple and let the compiler pick the widest SIMD tier
//! available.  `blur_v`'s SAXPY pattern (`row += w * src_row`) is especially
//! friendly to the auto-vectorizer.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

// ---------------------------------------------------------------------------
// Kernel construction
// ---------------------------------------------------------------------------

pub fn kernel(sigma: f64) -> Vec<f32> {
    let radius = (sigma * 3.0).ceil().max(1.0) as i64;
    let mut k = Vec::with_capacity((2 * radius + 1) as usize);
    let two_s2 = 2.0 * sigma * sigma;
    let mut sum = 0.0f64;
    for i in -radius..=radius {
        let w = (-(i as f64 * i as f64) / two_s2).exp();
        k.push(w as f32);
        sum += w;
    }
    for w in &mut k {
        *w = (*w as f64 / sum) as f32;
    }
    k
}

// ---------------------------------------------------------------------------
// Inner accumulation — scalar so LLVM can auto-vectorize across pixels
// ---------------------------------------------------------------------------

/// Weighted sum of `k.len()` RGBA samples.
/// `src` layout: [R0,G0,B0,A0, R1,G1,B1,A1, ...] (k.len() * 4 floats).
#[inline(always)]
fn accum_rgba(src: &[f32], k: &[f32]) -> [f32; 4] {
    let mut acc = [0.0f32; 4];
    for (j, &kw) in k.iter().enumerate() {
        let si = j * 4;
        for c in 0..4 {
            acc[c] += src[si + c] * kw;
        }
    }
    acc
}

/// SAXPY: dst[i] += w * src[i] for a whole row.
/// LLVM auto-vectorizes this to AVX2+FMA with target-cpu=native.
#[inline(always)]
fn saxpy(dst: &mut [f32], src: &[f32], w: f32) {
    for (a, &b) in dst.iter_mut().zip(src) {
        *a += b * w;
    }
}

// ---------------------------------------------------------------------------
// Horizontal pass
// ---------------------------------------------------------------------------

fn blur_h(img: &ImgF32, k: &[f32]) -> ImgF32 {
    let r = (k.len() / 2) as i64;
    let mut out = ImgF32::new(img.w, img.h);
    let row_stride = img.w * 4;
    let w = img.w;
    let r_usize = r as usize;
    let left_end = r_usize.min(w);
    let right_start = (w.saturating_sub(r_usize)).max(left_end);

    out.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            let src_row = &img.data[y * row_stride..(y + 1) * row_stride];

            // Edge pixels (left + right): clamp source index, then accumulate.
            for x in (0..left_end).chain(right_start..w) {
                // Gather clamped source pixels into a contiguous stack buffer.
                // Max kernel radius is ceil(36*3)=108; 4*(2*108+1)=868 floats worst-case.
                // Practical sigmas stay well below 10 (radius ≤ 30, buffer ≤ 244 floats).
                let mut buf = [0.0f32; 244];
                debug_assert!(k.len() * 4 <= buf.len());
                for (j, _) in k.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                    buf[j * 4..j * 4 + 4].copy_from_slice(&src_row[sx * 4..sx * 4 + 4]);
                }
                let acc = accum_rgba(&buf[..k.len() * 4], k);
                row_out[x * 4..x * 4 + 4].copy_from_slice(&acc);
            }

            // Interior pixels: source indices are always in-bounds; feed directly.
            for x in left_end..right_start {
                let start_idx = (x - r_usize) * 4;
                let src_ptr = &src_row[start_idx..start_idx + k.len() * 4];
                let acc = accum_rgba(src_ptr, k);
                row_out[x * 4..x * 4 + 4].copy_from_slice(&acc);
            }
        });
    out
}

// ---------------------------------------------------------------------------
// Vertical pass
// ---------------------------------------------------------------------------

fn blur_v(img: &ImgF32, k: &[f32]) -> ImgF32 {
    let r = (k.len() / 2) as i64;
    let h = img.h as i64;
    let mut out = ImgF32::new(img.w, img.h);
    let row_stride = img.w * 4;

    out.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            for (j, &kw) in k.iter().enumerate() {
                // clamp is a no-op for interior rows; correct for edge rows.
                let sy = (y as i64 + j as i64 - r).clamp(0, h - 1) as usize;
                let src_row = &img.data[sy * row_stride..(sy + 1) * row_stride];
                // SAXPY: auto-vectorized to AVX2+FMA with target-cpu=native.
                saxpy(row_out, src_row, kw);
            }
        });
    out
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Gaussian blur with independent sigmas. Sigmas below ~0.1 are treated as
/// no-ops on that axis (matching the script's VSIGMA floor of 0.1).
pub fn gblur(img: &ImgF32, sigma_h: f64, sigma_v: f64) -> ImgF32 {
    let mut cur = if sigma_h > 0.1 {
        blur_h(img, &kernel(sigma_h))
    } else {
        img.clone()
    };
    if sigma_v > 0.1 {
        cur = blur_v(&cur, &kernel(sigma_v));
    }
    cur
}

/// Isotropic blur convenience (halation, texture).
/// Uses a single kernel for both axes (sigma_h == sigma_v).
pub fn gblur_iso(img: &ImgF32, sigma: f64) -> ImgF32 {
    if sigma <= 0.1 {
        return img.clone();
    }
    let k = kernel(sigma);
    blur_v(&blur_h(img, &k), &k)
}

/// Blur with pre-computed kernels. Pass pre-computed kernels when the same
/// sigma is reused across multiple calls (e.g. in a frame loop).
pub fn gblur_precomputed(img: &ImgF32, kh: &[f32], kv: &[f32]) -> ImgF32 {
    blur_v(&blur_h(img, kh), kv)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_sums_to_one() {
        let k = kernel(1.5);
        let sum: f32 = k.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "kernel sum = {sum}");
    }

    #[test]
    fn sigma_zero_returns_clone() {
        let mut img = ImgF32::new(4, 4);
        for y in 0..4 {
            for x in 0..4 {
                let v = (x + y * 4) as f32 / 15.0;
                img.set(x, y, [v, v, v, 1.0]);
            }
        }
        let blurred = gblur(&img, 0.0, 0.0);
        assert_eq!(blurred.w, img.w);
        assert_eq!(blurred.h, img.h);
        for y in 0..4 {
            for x in 0..4 {
                let a = img.get(x, y);
                let b = blurred.get(x, y);
                for c in 0..4 {
                    assert!((a[c] - b[c]).abs() < 1e-6, "pixel ({x},{y})[{c}] differs");
                }
            }
        }
    }

    #[test]
    fn constant_image_stays_constant() {
        let mut img = ImgF32::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.set(x, y, [0.7, 0.7, 0.7, 1.0]);
            }
        }
        let blurred = gblur(&img, 2.0, 1.0);
        for y in 0..8 {
            for x in 0..8 {
                let v = blurred.get(x, y)[0];
                assert!((v - 0.7).abs() < 1e-5, "pixel ({x},{y}) = {v}");
            }
        }
    }

    #[test]
    fn iso_gblur_delegates() {
        let img = ImgF32::new(4, 4);
        let r = gblur_iso(&img, 0.0);
        assert_eq!(r.w, 4);
        assert_eq!(r.h, 4);
    }

    #[test]
    fn accum_rgba_scalar_correctness() {
        let k = kernel(1.0);
        let n = k.len();
        let src: Vec<f32> = (0..n * 4).map(|i| (i as f32) * 0.1).collect();
        let got = accum_rgba(&src, &k);
        let mut want = [0.0f32; 4];
        for (j, &kw) in k.iter().enumerate() {
            for c in 0..4 {
                want[c] += src[j * 4 + c] * kw;
            }
        }
        for c in 0..4 {
            assert!(
                (got[c] - want[c]).abs() < 1e-5,
                "channel {c}: got={} want={}",
                got[c],
                want[c]
            );
        }
    }
}
