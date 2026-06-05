//! Vignette — the `vignette=PI*power` filter (edge darkening).
//!
//! ffmpeg's default vignette is a cos^4 falloff parameterized by an angle. We
//! reproduce it with `factor = cos(angle * r/rmax)^4`, where `angle = PI*power`
//! and `rmax` is the half-diagonal, then multiply the RGB by that factor.

use crate::image_buf::ImgF32;

pub fn vignette(img: &mut ImgF32, power: f64) {
    let angle = std::f64::consts::PI * power;
    let cx = img.w as f64 / 2.0;
    let cy = img.h as f64 / 2.0;
    let rmax = (cx * cx + cy * cy).sqrt();
    for y in 0..img.h {
        let dy = y as f64 + 0.5 - cy;
        for x in 0..img.w {
            let dx = x as f64 + 0.5 - cx;
            let r = (dx * dx + dy * dy).sqrt();
            let theta = angle * (r / rmax);
            let c = theta.cos().max(0.0);
            let factor = (c * c * c * c) as f32;
            let i = img.idx(x, y);
            img.data[i] *= factor;
            img.data[i + 1] *= factor;
            img.data[i + 2] *= factor;
        }
    }
}
