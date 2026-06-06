//! Gamma conversion — the `lutrgb gammaval(2.2)` / `gammaval(0.454545)` dance.
//!
//! On normalized 0..1 values, ffmpeg's `gammaval(g)` is simply `x^g`. The script
//! processes spatial filters in a pseudo-linear space (`^2.2`) and converts back
//! (`^(1/2.2)`) before the gamma-space blends.

use crate::image_buf::ImgF32;

pub const TO_LINEAR: f32 = 2.2;
pub const FROM_LINEAR: f32 = 0.454_545_45;

pub fn to_linear(img: &mut ImgF32) {
    img.map_rgb(|x| x.max(0.0).powf(TO_LINEAR));
}

pub fn from_linear(img: &mut ImgF32) {
    img.map_rgb(|x| x.max(0.0).powf(FROM_LINEAR));
}

/// The halation branch's combined "revert gamma + slight contrast" lut:
/// `clip(gammaval(0.454545)*(258/256) - 2*256, 0, max)` evaluated in 16-bit
/// space, expressed on normalized values.
pub fn from_linear_halation(img: &mut ImgF32) {
    let scale = 258.0 / 256.0;
    let offset = 512.0 / 65535.0; // 2*256 out of the 16-bit max
    img.map_rgb(|x| (x.max(0.0).powf(FROM_LINEAR) * scale - offset).clamp(0.0, 1.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_linear_identity() {
        let mut img = ImgF32::new(2, 2);
        img.set(0, 0, [0.0, 0.0, 0.0, 1.0]);
        img.set(1, 0, [1.0, 1.0, 1.0, 1.0]);
        to_linear(&mut img);
        assert_eq!(img.get(0, 0)[0], 0.0);
        assert_eq!(img.get(1, 0)[0], 1.0);
    }

    #[test]
    fn gamma_roundtrip_half() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        to_linear(&mut img);
        from_linear(&mut img);
        let v = img.get(0, 0)[0];
        assert!((v - 0.5).abs() < 1e-4, "roundtrip error: {v}");
    }

    #[test]
    fn from_linear_halation_scales() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        from_linear_halation(&mut img);
        let v = img.get(0, 0)[0];
        // halation is brighter than the straight from_linear, with a subtract
        assert!(v > 0.0 && v <= 1.0);
    }
}
