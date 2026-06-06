//! Barrel distortion — the `lenscorrection=k1=..:k2=..:i=bilinear` filter,
//! used for both CRT surface curvature and the (equal-or-greater) bezel
//! curvature.
//!
//! For each destination pixel we offset from the image center, normalize the
//! radius by the half-diagonal (corner -> 1.0), scale the offset by
//! `1 + k1*r^2 + k2*r^4`, and bilinearly sample the source. Out-of-range
//! samples are black, matching the script's "pad black, distort, crop" trick.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

fn sample_bilinear(img: &ImgF32, fx: f64, fy: f64) -> [f32; 4] {
    if fx < 0.0 || fy < 0.0 || fx > (img.w - 1) as f64 || fy > (img.h - 1) as f64 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let x1 = (x0 + 1).min(img.w - 1);
    let y1 = (y0 + 1).min(img.h - 1);
    let tx = (fx - x0 as f64) as f32;
    let ty = (fy - y0 as f64) as f32;
    let p00 = img.get(x0, y0);
    let p10 = img.get(x1, y0);
    let p01 = img.get(x0, y1);
    let p11 = img.get(x1, y1);
    let mut out = [0.0f32; 4];
    for c in 0..4 {
        let top = p00[c] * (1.0 - tx) + p10[c] * tx;
        let bot = p01[c] * (1.0 - tx) + p11[c] * tx;
        out[c] = top * (1.0 - ty) + bot * ty;
    }
    out
}

/// Apply barrel distortion in place-returning a new image of the same size.
/// `k` of 0 returns a clone (no distortion).
pub fn lenscorrection(img: &ImgF32, k1: f64, k2: f64) -> ImgF32 {
    if k1 == 0.0 && k2 == 0.0 {
        return img.clone();
    }
    let w = img.w as f64;
    let h = img.h as f64;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let half_diag = (cx * cx + cy * cy).sqrt();
    let mut out = ImgF32::new(img.w, img.h);
    let row_stride = img.w * 4;
    out.data.par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row_out)| {
            let dy = y as f64 + 0.5 - cy;
            for x in 0..img.w {
                let dx = x as f64 + 0.5 - cx;
                let dn = (dx * dx + dy * dy).sqrt() / half_diag;
                let r2 = dn * dn;
                let mult = 1.0 + k1 * r2 + k2 * r2 * r2;
                let sx = cx + dx * mult - 0.5;
                let sy = cy + dy * mult - 0.5;
                let p = sample_bilinear(img, sx, sy);
                let di = x * 4;
                row_out[di..di + 4].copy_from_slice(&p);
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_zero_returns_clone() {
        let mut img = ImgF32::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let v = (x + y) as f32 / 30.0;
                img.set(x, y, [v, v, v, 1.0]);
            }
        }
        let out = lenscorrection(&img, 0.0, 0.0);
        for y in 0..16 {
            for x in 0..16 {
                let a = img.get(x, y);
                let b = out.get(x, y);
                for c in 0..4 {
                    assert!((a[c] - b[c]).abs() < 1e-6,
                        "pixel ({x},{y})[{c}] differs: {} vs {}", a[c], b[c]);
                }
            }
        }
    }

    #[test]
    fn white_center_stays_white() {
        let mut img = ImgF32::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
        let out = lenscorrection(&img, -0.2, 0.0);
        let c = out.get(3, 3); // near center
        assert!(c[0] > 0.99, "center red = {}", c[0]);
    }
}
