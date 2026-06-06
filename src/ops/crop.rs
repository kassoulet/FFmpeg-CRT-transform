//! `cropdetect` + `crop`.
//!
//! The script builds a white `PX x PY` canvas, applies the CRT curvature, and
//! runs `cropdetect` to find the bounding box of the non-black (curved) screen
//! area, then crops `TMPstep03` to it. With no curvature the white canvas fills
//! the frame and the crop is a no-op.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// Bounding box of pixels whose luma exceeds `limit` (0..1). Dimensions are
/// rounded down to a multiple of 2 (`cropdetect ... round=2`).
pub fn detect(reference: &ImgF32, limit: f32) -> Rect {
    let mut min_x = reference.w;
    let mut min_y = reference.h;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut found = false;
    for y in 0..reference.h {
        for x in 0..reference.w {
            let p = reference.get(x, y);
            if p[0] > limit || p[1] > limit || p[2] > limit {
                found = true;
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }
    if !found {
        return Rect {
            x: 0,
            y: 0,
            w: reference.w,
            h: reference.h,
        };
    }
    let mut w = max_x - min_x + 1;
    let mut h = max_y - min_y + 1;
    w -= w % 2;
    h -= h % 2;
    Rect {
        x: min_x,
        y: min_y,
        w: w.max(2),
        h: h.max(2),
    }
}

pub fn crop(img: &ImgF32, r: &Rect) -> ImgF32 {
    let w = r.w.min(img.w - r.x);
    let h = r.h.min(img.h - r.y);
    let mut out = ImgF32::new(w, h);
    let row_stride = w * 4;
    out.data
        .par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                let p = img.get(r.x + x, r.y + y);
                let di = x * 4;
                row[di..di + 4].copy_from_slice(&p);
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_all_white_full_rect() {
        let mut img = ImgF32::new(8, 6);
        for y in 0..6 {
            for x in 0..8 {
                img.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
        let r = detect(&img, 0.1);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
        assert_eq!(r.w, 8);
        assert_eq!(r.h, 6);
    }

    #[test]
    fn detect_all_black_returns_full() {
        let mut img = ImgF32::new(8, 6);
        for y in 0..6 {
            for x in 0..8 {
                img.set(x, y, [0.0, 0.0, 0.0, 1.0]);
            }
        }
        let r = detect(&img, 0.1);
        assert_eq!(r.x, 0);
        assert_eq!(r.y, 0);
        assert_eq!(r.w, 8);
        assert_eq!(r.h, 6);
    }

    #[test]
    fn detect_single_white_pixel() {
        let mut img = ImgF32::new(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                img.set(x, y, [0.0, 0.0, 0.0, 1.0]);
            }
        }
        img.set(5, 3, [1.0, 1.0, 1.0, 1.0]);
        let r = detect(&img, 0.5);
        // White pixel at (5,3) → rect from (5,3) to (5,3), rounded to even
        assert!(r.x <= 5);
        assert!(r.y <= 3);
        assert!(r.x + r.w > 5);
        assert!(r.y + r.h > 3);
        assert_eq!(r.w % 2, 0);
        assert_eq!(r.h % 2, 0);
    }

    #[test]
    fn crop_sub_region() {
        let mut img = ImgF32::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
        for y in 2..6 {
            for x in 2..6 {
                img.set(x, y, [0.5, 0.5, 0.5, 1.0]);
            }
        }
        let r = Rect {
            x: 2,
            y: 2,
            w: 4,
            h: 4,
        };
        let cropped = crop(&img, &r);
        assert_eq!(cropped.w, 4);
        assert_eq!(cropped.h, 4);
        assert_eq!(cropped.get(0, 0)[0], 0.5);
    }
}
