//! `cropdetect` + `crop`.
//!
//! The script builds a white `PX x PY` canvas, applies the CRT curvature, and
//! runs `cropdetect` to find the bounding box of the non-black (curved) screen
//! area, then crops `TMPstep03` to it. With no curvature the white canvas fills
//! the frame and the crop is a no-op.

use crate::image_buf::ImgF32;

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
                if x < min_x { min_x = x; }
                if y < min_y { min_y = y; }
                if x > max_x { max_x = x; }
                if y > max_y { max_y = y; }
            }
        }
    }
    if !found {
        return Rect { x: 0, y: 0, w: reference.w, h: reference.h };
    }
    let mut w = max_x - min_x + 1;
    let mut h = max_y - min_y + 1;
    w -= w % 2;
    h -= h % 2;
    Rect { x: min_x, y: min_y, w: w.max(2), h: h.max(2) }
}

pub fn crop(img: &ImgF32, r: &Rect) -> ImgF32 {
    let w = r.w.min(img.w - r.x);
    let h = r.h.min(img.h - r.y);
    let mut out = ImgF32::new(w, h);
    for y in 0..h {
        for x in 0..w {
            out.set(x, y, img.get(r.x + x, r.y + y));
        }
    }
    out
}
