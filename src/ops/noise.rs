//! Seeded noise for the paper / lcdgrain substrate textures.
//!
//! ffmpeg's `noise` filter uses its own internal PRNG, so our textures will
//! differ in detail (the plan calls this out as expected). We use a small
//! deterministic hash so results are reproducible run-to-run without needing a
//! random source. `seed` matches the script's `all_seed=5150`.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

#[inline]
fn hash(mut x: u64) -> u64 {
    // splitmix64 finalizer
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Uniform value in 0..1 for a pixel.
#[inline]
fn rand01(seed: u64, x: usize, y: usize) -> f32 {
    let h = hash(seed ^ (x as u64).wrapping_mul(0x100000001B3) ^ (y as u64) << 1);
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Gray field centered on 0.5 with uniform noise of amplitude `strength`/255.
/// Returns an RGBA `ImgF32` (gray replicated across RGB, alpha 1).
pub fn gray_noise(w: usize, h: usize, seed: u64, strength: f64) -> ImgF32 {
    let amp = (strength / 255.0) as f32;
    let mut img = ImgF32::new(w, h);
    let row_stride = w * 4;
    img.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let n = (rand01(seed, x, y) - 0.5) * 2.0 * amp;
                let v = (0.5 + n).clamp(0.0, 1.0);
                let di = x * 4;
                row[di..di + 4].copy_from_slice(&[v, v, v, 1.0]);
            }
        });
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gray_noise_correct_dimensions() {
        let img = gray_noise(16, 9, 5150, 32.0);
        assert_eq!(img.w, 16);
        assert_eq!(img.h, 9);
        assert_eq!(img.data.len(), 16 * 9 * 4);
    }

    #[test]
    fn gray_noise_values_in_range() {
        let img = gray_noise(32, 32, 5150, 64.0);
        for y in 0..32 {
            for x in 0..32 {
                let p = img.get(x, y);
                assert!(p[0] >= 0.0 && p[0] <= 1.0, "pixel ({x},{y}) = {}", p[0]);
                assert_eq!(p[3], 1.0);
            }
        }
    }

    #[test]
    fn gray_noise_deterministic() {
        let a = gray_noise(8, 8, 5150, 32.0);
        let b = gray_noise(8, 8, 5150, 32.0);
        assert_eq!(a.data, b.data);
    }

    #[test]
    fn zero_strength_gray_field() {
        let img = gray_noise(4, 4, 5150, 0.0);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(img.get(x, y)[0], 0.5);
            }
        }
    }
}
