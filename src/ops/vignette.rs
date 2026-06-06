//! Vignette — the `vignette=PI*power` filter (edge darkening).
//!
//! ffmpeg's default vignette is a cos^4 falloff parameterized by an angle. We
//! reproduce it with `factor = cos(angle * r/rmax)^4`, where `angle = PI*power`
//! and `rmax` is the half-diagonal, then multiply the RGB by that factor.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

pub fn vignette(img: &mut ImgF32, power: f64) {
    let angle = std::f64::consts::PI * power;
    let cx = img.w as f64 / 2.0;
    let cy = img.h as f64 / 2.0;
    let rmax = (cx * cx + cy * cy).sqrt();
    let row_stride = img.w * 4;
    img.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let dy = y as f64 + 0.5 - cy;
            for x in 0..img.w {
                let dx = x as f64 + 0.5 - cx;
                let r = (dx * dx + dy * dy).sqrt();
                let theta = angle * (r / rmax);
                let c = theta.cos().max(0.0);
                let factor = (c * c * c * c) as f32;
                let di = x * 4;
                row[di] *= factor;
                row[di + 1] *= factor;
                row[di + 2] *= factor;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_zero_nop() {
        let mut img = ImgF32::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.set(x, y, [0.5, 0.5, 0.5, 1.0]);
            }
        }
        let copy = img.clone();
        vignette(&mut img, 0.0);
        for y in 0..8 {
            for x in 0..8 {
                let a = copy.get(x, y);
                let b = img.get(x, y);
                for c in 0..4 {
                    assert!(
                        (a[c] - b[c]).abs() < 1e-6,
                        "pixel ({x},{y})[{c}] differs: {} vs {}",
                        a[c],
                        b[c]
                    );
                }
            }
        }
    }

    #[test]
    fn corners_darker_than_center() {
        let mut img = ImgF32::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                img.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
        vignette(&mut img, 0.3);
        let center = img.get(7, 7)[0];
        let corner = img.get(0, 0)[0];
        assert!(center > corner, "center={center} <= corner={corner}");
    }
}
