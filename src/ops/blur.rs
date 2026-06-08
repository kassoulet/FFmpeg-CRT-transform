//! Separable Gaussian blur — the `gblur=sigma=..:sigmaV=..:steps=..` filter.
//!
//! ffmpeg's `gblur` uses a recursive IIR Gaussian whose `steps` parameter just
//! sharpens the approximation; a single truncated FIR Gaussian convolution is
//! perceptually equivalent for our purposes. Horizontal and vertical sigmas are
//! independent (the script often blurs more horizontally than vertically).
//!
//! ## SIMD strategy
//!
//! Each output pixel is 4 × f32 (RGBA). The hot path in `blur_h` accumulates
//! a weighted sum of k kernel taps over 4 channels — exactly one 128-bit SSE
//! operation per tap. `blur_v` is a SAXPY (row += weight * src_row) which the
//! compiler auto-vectorizes to AVX2+FMA when `target-cpu=native` is set.
//!
//! Compile-time dispatch (no runtime overhead):
//!   - x86_64 + FMA  → `accum_rgba_fma`  (1 `_mm_fmadd_ps` per tap)
//!   - x86_64        → `accum_rgba_sse2` (mul + add, SSE2 baseline)
//!   - other arches  → scalar fallback (auto-vectorized by LLVM)

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
// SIMD helpers
// ---------------------------------------------------------------------------

/// Accumulate `k.len()` weighted RGBA samples into a single [f32; 4].
/// `src` layout: [R0,G0,B0,A0, R1,G1,B1,A1, ...] (k.len() * 4 floats).
#[inline(always)]
fn accum_rgba(src: &[f32], k: &[f32]) -> [f32; 4] {
    // Compile-time dispatch: best available ISA wins, zero runtime overhead.
    #[cfg(all(target_arch = "x86_64", target_feature = "fma"))]
    // SAFETY: target_feature = "fma" is a compile-time guarantee.
    return unsafe { accum_rgba_fma(src.as_ptr(), k) };

    #[cfg(all(target_arch = "x86_64", not(target_feature = "fma")))]
    // SAFETY: SSE2 is guaranteed on x86_64.
    return unsafe { accum_rgba_sse2(src.as_ptr(), k) };

    #[cfg(not(target_arch = "x86_64"))]
    {
        let mut acc = [0.0f32; 4];
        for (j, &kw) in k.iter().enumerate() {
            let si = j * 4;
            for c in 0..4 {
                acc[c] += src[si + c] * kw;
            }
        }
        acc
    }
}

/// SSE2 path: 1 mul + 1 add per kernel tap (128-bit, 4×f32).
/// Only compiled when FMA is absent (otherwise `accum_rgba_fma` is used).
#[cfg(all(target_arch = "x86_64", not(target_feature = "fma")))]
#[target_feature(enable = "sse2")]
unsafe fn accum_rgba_sse2(src: *const f32, k: &[f32]) -> [f32; 4] {
    use std::arch::x86_64::*;
    let mut acc = _mm_setzero_ps();
    for (j, &kw) in k.iter().enumerate() {
        let s = _mm_loadu_ps(src.add(j * 4));
        acc = _mm_add_ps(acc, _mm_mul_ps(_mm_set1_ps(kw), s));
    }
    let mut out = [0.0f32; 4];
    _mm_storeu_ps(out.as_mut_ptr(), acc);
    out
}

/// FMA path: 1 fused multiply-add per kernel tap — halves instruction count.
#[cfg(all(target_arch = "x86_64", target_feature = "fma"))]
#[target_feature(enable = "fma")]
unsafe fn accum_rgba_fma(src: *const f32, k: &[f32]) -> [f32; 4] {
    use std::arch::x86_64::*;
    let mut acc = _mm_setzero_ps();
    for (j, &kw) in k.iter().enumerate() {
        let s = _mm_loadu_ps(src.add(j * 4));
        // fmadd_ps(a, b, c) = a*b + c
        acc = _mm_fmadd_ps(_mm_set1_ps(kw), s, acc);
    }
    let mut out = [0.0f32; 4];
    _mm_storeu_ps(out.as_mut_ptr(), acc);
    out
}

/// SAXPY: dst[i] += w * src[i] for a whole row.
/// Written as a plain iterator loop so LLVM auto-vectorizes it to AVX2+FMA
/// when target-cpu=native is set (see .cargo/config.toml).
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
                // Build a temporary aligned slice of the clamped source pixels.
                // Stack-allocate to avoid heap churn; kernel radius <= 36 taps.
                let mut buf = [0.0f32; 148]; // 4 * (2*36+1) = 292, but max sigma=36 → 37*2+1=75 taps
                debug_assert!(k.len() * 4 <= buf.len());
                for (j, _) in k.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                    buf[j * 4..j * 4 + 4].copy_from_slice(&src_row[sx * 4..sx * 4 + 4]);
                }
                let acc = accum_rgba(&buf[..k.len() * 4], k);
                row_out[x * 4..x * 4 + 4].copy_from_slice(&acc);
            }

            // Middle pixels: source indices always in range; feed directly.
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
                // clamp is a no-op for rows in the interior; correct for edge rows.
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
    fn accum_rgba_matches_scalar() {
        // Verify SIMD path produces the same result as the scalar reference.
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
