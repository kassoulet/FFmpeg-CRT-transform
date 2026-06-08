//! Separable Gaussian blur — the `gblur=sigma=..:sigmaV=..:steps=..` filter.
//!
//! ffmpeg's `gblur` uses a recursive IIR Gaussian whose `steps` parameter just
//! sharpens the approximation; a single truncated FIR Gaussian convolution is
//! perceptually equivalent for our purposes. Horizontal and vertical sigmas are
//! independent (the script often blurs more horizontally than vertically).

use crate::image_buf::ImgF32;
use rayon::prelude::*;

fn kernel(sigma: f64) -> Vec<f32> {
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

            // 1. Left edge: needs clamping
            for x in 0..left_end {
                let mut acc = [0.0f32; 4];
                for (j, &kw) in k.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                    let si = sx * 4;
                    for c in 0..4 {
                        acc[c] += src_row[si + c] * kw;
                    }
                }
                let di = x * 4;
                row_out[di..di + 4].copy_from_slice(&acc);
            }

            // 2. Middle: no clamping needed, sequential source access
            for x in left_end..right_start {
                let mut acc = [0.0f32; 4];
                let start_idx = (x - r_usize) * 4;
                let src_ptr = &src_row[start_idx..start_idx + k.len() * 4];
                for (j, &kw) in k.iter().enumerate() {
                    let si = j * 4;
                    for c in 0..4 {
                        acc[c] += src_ptr[si + c] * kw;
                    }
                }
                let di = x * 4;
                row_out[di..di + 4].copy_from_slice(&acc);
            }

            // 3. Right edge: needs clamping
            for x in right_start..w {
                let mut acc = [0.0f32; 4];
                for (j, &kw) in k.iter().enumerate() {
                    let sx = (x as i64 + j as i64 - r).clamp(0, w as i64 - 1) as usize;
                    let si = sx * 4;
                    for c in 0..4 {
                        acc[c] += src_row[si + c] * kw;
                    }
                }
                let di = x * 4;
                row_out[di..di + 4].copy_from_slice(&acc);
            }
        });
    out
}

fn blur_v(img: &ImgF32, k: &[f32]) -> ImgF32 {
    let r = (k.len() / 2) as i64;
    let mut out = ImgF32::new(img.w, img.h);
    let row_stride = img.w * 4;
    let h = img.h;
    let r_usize = r as usize;

    let top_edge = r_usize.min(h);
    let bottom_start = h.saturating_sub(r_usize).max(top_edge);

    out.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            if y < top_edge || y >= bottom_start {
                for (j, &w) in k.iter().enumerate() {
                    let sy = (y as i64 + j as i64 - r).clamp(0, h as i64 - 1) as usize;
                    let src_row = &img.data[sy * row_stride..(sy + 1) * row_stride];
                    for (px_out, px_in) in row_out.chunks_exact_mut(4).zip(src_row.chunks_exact(4))
                    {
                        for c in 0..4 {
                            px_out[c] += px_in[c] * w;
                        }
                    }
                }
            } else {
                for (j, &w) in k.iter().enumerate() {
                    let sy = y + j - r_usize;
                    let src_row = &img.data[sy * row_stride..(sy + 1) * row_stride];
                    for (px_out, px_in) in row_out.chunks_exact_mut(4).zip(src_row.chunks_exact(4))
                    {
                        for c in 0..4 {
                            px_out[c] += px_in[c] * w;
                        }
                    }
                }
            }
        });
    out
}

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
pub fn gblur_iso(img: &ImgF32, sigma: f64) -> ImgF32 {
    gblur(img, sigma, sigma)
}

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
}
