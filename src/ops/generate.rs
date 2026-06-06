//! Procedural generators + simple pointwise image adjustments.
//!
//! Covers the bits the script synthesizes with `geq`/`lutrgb`: rounded-corner
//! masks, the scanline luminance column, the flat-panel pixel grid, plus the
//! grayscale / blackpoint / brighten / negate point ops.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

/// Stamp black rounded corners onto a (white) bezel canvas: a quarter circle of
/// `radius` in each corner; pixels outside the circle become black. Mirrors the
/// script's `geq lum=if((X-W)^2+(Y-H)^2 <= r^2 ...)` corner build.
pub fn round_corners(img: &mut ImgF32, radius: usize) {
    if radius == 0 || radius * 2 > img.w || radius * 2 > img.h {
        return;
    }
    let r = radius as f64;
    let r2 = r * r;
    // four corner centers (cx, cy) are the inner points of each rounded corner
    let corners = [
        (r, r),                                  // top-left
        (img.w as f64 - r, r),                   // top-right
        (r, img.h as f64 - r),                   // bottom-left
        (img.w as f64 - r, img.h as f64 - r),    // bottom-right
    ];
    for (ci, &(cx, cy)) in corners.iter().enumerate() {
        let (x0, x1, y0, y1) = match ci {
            0 => (0, radius, 0, radius),
            1 => (img.w - radius, img.w, 0, radius),
            2 => (0, radius, img.h - radius, img.h),
            _ => (img.w - radius, img.w, img.h - radius, img.h),
        };
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f64 + 0.5 - cx;
                let dy = y as f64 + 0.5 - cy;
                if dx * dx + dy * dy > r2 {
                    img.set(x, y, [0.0, 0.0, 0.0, 1.0]);
                }
            }
        }
    }
}

/// One-column scanline luminance profile, `period` pixels tall:
/// `lum = pow(sin(Y*PI/period), 1/weight)` for the first `period` rows.
pub fn scanline_column(period: usize, weight: f64) -> ImgF32 {
    let mut img = ImgF32::new(1, period.max(1));
    for y in 0..period {
        let s = (y as f64 * std::f64::consts::PI / period as f64).sin().max(0.0);
        let lum = s.powf(1.0 / weight) as f32;
        img.set(0, y, [lum, lum, lum, 1.0]);
    }
    img
}

/// Flat-panel pixel grid at native (SXINT x PY) resolution. Gap cells get
/// `lum_gap`, pixel cells get `lum_px` (both 0..1). `gx`/`gy` are the cell
/// pitch; `gap_x`/`gap_y` the gap width within each cell.
pub fn pixel_grid(
    w: usize,
    h: usize,
    gx: usize,
    gy: usize,
    gap_x: usize,
    gap_y: usize,
    lum_gap: f32,
    lum_px: f32,
) -> ImgF32 {
    let mut img = ImgF32::new(w, h);
    let gx = gx.max(1);
    let gy = gy.max(1);
    let row_stride = w * 4;
    img.data.par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let in_gap_y = gy >= gap_y && (y % gy) >= gx_sub(gy, gap_y);
            for x in 0..w {
                let in_gap_x = gx >= gap_x && (x % gx) >= gx_sub(gx, gap_x);
                let lum = if in_gap_x || in_gap_y { lum_gap } else { lum_px };
                let di = x * 4;
                row[di..di + 4].copy_from_slice(&[lum, lum, lum, 1.0]);
            }
        });
    img
}

#[inline]
fn gx_sub(a: usize, b: usize) -> usize {
    a.saturating_sub(b)
}

/// Rec.601 luma, written back to all three channels (the `format=gray` step).
pub fn to_gray(img: &mut ImgF32) {
    img.data.par_chunks_exact_mut(4).for_each(|px| {
        let l = 0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2];
        px[0] = l;
        px[1] = l;
        px[2] = l;
    });
}

/// Blackpoint lift: `val + (bp*256/65535)*(1-val)`, bp in 0..255.
pub fn blackpoint(img: &mut ImgF32, bp: f64) {
    let k = (bp * 256.0 / 65535.0) as f32;
    if k == 0.0 {
        return;
    }
    img.map_rgb(|v| v + k * (1.0 - v));
}

/// Brightness multiply with clamp to 0..1 (`clip(val*brighten, 0, max)`).
pub fn brighten(img: &mut ImgF32, mult: f64) {
    let m = mult as f32;
    img.map_rgb(|v| (v * m).clamp(0.0, 1.0));
}

/// Invert RGB (`negate`).
pub fn negate(img: &mut ImgF32) {
    img.map_rgb(|v| 1.0 - v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_corners_zero_radius_is_nop() {
        let mut img = ImgF32::filled(10, 10, [1.0, 1.0, 1.0, 1.0]);
        round_corners(&mut img, 0);
        for y in 0..10 {
            for x in 0..10 {
                assert_eq!(img.get(x, y)[0], 1.0);
            }
        }
    }

    #[test]
    fn round_corners_cuts_corners() {
        let mut img = ImgF32::filled(6, 6, [1.0, 1.0, 1.0, 1.0]);
        round_corners(&mut img, 2);
        // corner pixel should be black (outside radius)
        assert_eq!(img.get(0, 0)[0], 0.0);
        // center pixel stays white
        assert_eq!(img.get(2, 2)[0], 1.0);
    }

    #[test]
    fn scanline_column_range() {
        let col = scanline_column(4, 1.0);
        assert_eq!(col.w, 1);
        assert_eq!(col.h, 4);
        // at y=0, sin(0) = 0
        assert_eq!(col.get(0, 0)[0], 0.0);
        // at mid-point, sin(pi/2) = 1
        assert_eq!(col.get(0, 2)[0], 1.0);
    }

    #[test]
    fn pixel_grid_dimensions() {
        let grid = pixel_grid(20, 10, 4, 4, 1, 1, 0.0, 1.0);
        assert_eq!(grid.w, 20);
        assert_eq!(grid.h, 10);
    }

    #[test]
    fn grayscale_rec601() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [1.0, 0.0, 0.0, 1.0]);
        to_gray(&mut img);
        let v = img.get(0, 0)[0];
        assert!((v - 0.299).abs() < 1e-6);
    }

    #[test]
    fn blackpoint_zero_is_nop() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        blackpoint(&mut img, 0.0);
        assert_eq!(img.get(0, 0)[0], 0.5);
    }

    #[test]
    fn brighten_mult_one_is_nop() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.5, 0.5, 0.5, 1.0]);
        brighten(&mut img, 1.0);
        assert_eq!(img.get(0, 0)[0], 0.5);
    }

    #[test]
    fn negate_inverts() {
        let mut img = ImgF32::new(1, 1);
        img.set(0, 0, [0.3, 0.6, 0.9, 1.0]);
        negate(&mut img);
        let p = img.get(0, 0);
        assert!((p[0] - 0.7).abs() < 1e-6);
        assert!((p[1] - 0.4).abs() < 1e-6);
        assert!((p[2] - 0.1).abs() < 1e-6);
    }
}
