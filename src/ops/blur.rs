//! Separable Gaussian blur — the `gblur=sigma=..:sigmaV=..:steps=..` filter.
//!
//! ffmpeg's `gblur` uses a recursive IIR Gaussian whose `steps` parameter just
//! sharpens the approximation; a single truncated FIR Gaussian convolution is
//! perceptually equivalent for our purposes. Horizontal and vertical sigmas are
//! independent (the script often blurs more horizontally than vertically).

use crate::image_buf::ImgF32;

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
    for y in 0..img.h {
        for x in 0..img.w {
            let mut acc = [0.0f32; 4];
            for (j, &w) in k.iter().enumerate() {
                let sx = (x as i64 + j as i64 - r).clamp(0, img.w as i64 - 1) as usize;
                let p = img.get(sx, y);
                for c in 0..4 {
                    acc[c] += p[c] * w;
                }
            }
            out.set(x, y, acc);
        }
    }
    out
}

fn blur_v(img: &ImgF32, k: &[f32]) -> ImgF32 {
    let r = (k.len() / 2) as i64;
    let mut out = ImgF32::new(img.w, img.h);
    for y in 0..img.h {
        for x in 0..img.w {
            let mut acc = [0.0f32; 4];
            for (j, &w) in k.iter().enumerate() {
                let sy = (y as i64 + j as i64 - r).clamp(0, img.h as i64 - 1) as usize;
                let p = img.get(x, sy);
                for c in 0..4 {
                    acc[c] += p[c] * w;
                }
            }
            out.set(x, y, acc);
        }
    }
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
