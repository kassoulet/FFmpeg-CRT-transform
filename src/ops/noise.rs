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
    img.data.par_chunks_exact_mut(row_stride)
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
