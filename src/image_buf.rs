//! `ImgF32` — the universal working buffer for the whole pipeline.
//!
//! Interleaved RGBA, one `f32` per channel, normalized to 0..1 (values may
//! transiently exceed that range mid-computation; callers clamp where it
//! matters). This single representation subsumes the batch script's
//! 8-bit-vs-16-bit (`RNG`/`RGBFMT`) branching: we always have float headroom,
//! so `16BPC_PROCESSING` only influences the *output* quantization (`OFORMAT`).

use anyhow::{Context, Result};
use std::path::Path;

#[derive(Clone)]
pub struct ImgF32 {
    pub w: usize,
    pub h: usize,
    /// length == w * h * 4, channel order R,G,B,A
    pub data: Vec<f32>,
}

impl ImgF32 {
    pub fn new(w: usize, h: usize) -> Self {
        ImgF32 {
            w,
            h,
            data: vec![0.0; w * h * 4],
        }
    }

    /// Solid fill (alpha included).
    pub fn filled(w: usize, h: usize, rgba: [f32; 4]) -> Self {
        let mut img = ImgF32::new(w, h);
        for px in img.data.chunks_exact_mut(4) {
            px.copy_from_slice(&rgba);
        }
        img
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        (y * self.w + x) * 4
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [f32; 4] {
        let i = self.idx(x, y);
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, rgba: [f32; 4]) {
        let i = self.idx(x, y);
        self.data[i..i + 4].copy_from_slice(&rgba);
    }

    /// Apply a per-channel function to R,G,B (alpha untouched).
    pub fn map_rgb<F: Fn(f32) -> f32 + Send + Sync>(&mut self, f: F) {
        use rayon::prelude::*;
        self.data.par_chunks_exact_mut(4).for_each(|px| {
            px[0] = f(px[0]);
            px[1] = f(px[1]);
            px[2] = f(px[2]);
        });
    }

    /// Load an image file as RGBA f32 (0..1). 8- and 16-bit sources are both
    /// normalized; alpha defaults to opaque when the source has none.
    pub fn load(path: &Path) -> Result<Self> {
        let dyn_img =
            image::open(path).with_context(|| format!("Couldn't read image {}", path.display()))?;
        let rgba = dyn_img.to_rgba32f(); // image crate already linearizes nothing; raw 0..1
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        Ok(ImgF32 {
            w,
            h,
            data: rgba.into_raw(),
        })
    }

    /// Quantize and write. `bpc` is 8 or 16; alpha is dropped (RGB output).
    pub fn save(&self, path: &Path, bpc: u8) -> Result<()> {
        match bpc {
            16 => {
                let mut buf: Vec<u16> = Vec::with_capacity(self.w * self.h * 3);
                for px in self.data.chunks_exact(4) {
                    for c in &px[..3] {
                        buf.push((c.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16);
                    }
                }
                let img = image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(
                    self.w as u32,
                    self.h as u32,
                    buf,
                )
                .context("rgb48 buffer size mismatch")?;
                img.save(path)
                    .with_context(|| format!("write {}", path.display()))?;
            }
            _ => {
                let mut buf: Vec<u8> = Vec::with_capacity(self.w * self.h * 3);
                for px in self.data.chunks_exact(4) {
                    for c in &px[..3] {
                        buf.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                    }
                }
                let img = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(
                    self.w as u32,
                    self.h as u32,
                    buf,
                )
                .context("rgb24 buffer size mismatch")?;
                img.save(path)
                    .with_context(|| format!("write {}", path.display()))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_correct_size() {
        let img = ImgF32::new(10, 20);
        assert_eq!(img.w, 10);
        assert_eq!(img.h, 20);
        assert_eq!(img.data.len(), 10 * 20 * 4);
        // all zero
        assert!(img.data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn filled_sets_all_pixels() {
        let img = ImgF32::filled(3, 4, [0.2, 0.4, 0.6, 1.0]);
        for y in 0..4 {
            for x in 0..3 {
                assert_eq!(img.get(x, y), [0.2, 0.4, 0.6, 1.0]);
            }
        }
    }

    #[test]
    fn get_set_roundtrip() {
        let mut img = ImgF32::new(5, 5);
        img.set(2, 3, [0.1, 0.2, 0.3, 0.5]);
        let p = img.get(2, 3);
        assert_eq!(p[0], 0.1);
        assert_eq!(p[1], 0.2);
        assert_eq!(p[2], 0.3);
        assert_eq!(p[3], 0.5);
    }

    #[test]
    fn map_rgb_applies_to_rgb_only() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.5, 0.5, 0.5, 0.25]);
        img.map_rgb(|x| x * 2.0);
        let p = img.get(0, 0);
        assert_eq!(p[0], 1.0);
        assert_eq!(p[1], 1.0);
        assert_eq!(p[2], 1.0);
        assert_eq!(p[3], 0.25); // alpha unchanged
    }

    #[test]
    fn clone_is_independent() {
        let mut img = ImgF32::new(2, 2);
        img.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        let copy = img.clone();
        img.set(0, 0, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(copy.get(0, 0)[0], 0.5);
    }
}
