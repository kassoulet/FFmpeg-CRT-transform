//! Blend modes, matching ffmpeg's `blend` filter semantics.
//!
//! In ffmpeg the FIRST input is the "bottom" (B) and the SECOND is the "top"
//! (A). `all_opacity` mixes between the bottom and the blended result:
//!   out = (1-opacity)*bottom + opacity*mode(top, bottom)
//! so opacity=0 leaves the bottom input untouched.

use crate::image_buf::ImgF32;
use rayon::prelude::*;

#[inline]
fn multiply(top: f32, bot: f32) -> f32 {
    top * bot
}
#[inline]
fn screen(top: f32, bot: f32) -> f32 {
    1.0 - (1.0 - top) * (1.0 - bot)
}
#[inline]
fn lighten(top: f32, bot: f32) -> f32 {
    top.max(bot)
}
#[inline]
fn color_burn(a: f32, b: f32) -> f32 {
    // ffmpeg BURN(a,b): a==0 ? 0 : 1 - min(1,(1-b)/a)
    if a <= 0.0 { 0.0 } else { 1.0 - (1.0 - b).min(a) / a.max(1e-6) }
}
#[inline]
fn color_dodge(a: f32, b: f32) -> f32 {
    // ffmpeg DODGE(a,b): a==1 ? 1 : min(1, b/(1-a))
    if a >= 1.0 { 1.0 } else { (b / (1.0 - a)).min(1.0) }
}
#[inline]
fn vividlight(top: f32, bot: f32) -> f32 {
    // ffmpeg VIVIDLIGHT: A=top selects burn/dodge applied to B=bottom.
    if top < 0.5 {
        color_burn(2.0 * top, bot)
    } else {
        color_dodge(2.0 * (top - 0.5), bot)
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    Multiply,
    Screen,
    Lighten,
    VividLight,
}

impl Mode {
    #[inline]
    fn apply(&self, top: f32, bot: f32) -> f32 {
        match self {
            Mode::Multiply => multiply(top, bot),
            Mode::Screen => screen(top, bot),
            Mode::Lighten => lighten(top, bot),
            Mode::VividLight => vividlight(top, bot),
        }
    }
}

/// `bottom` is modified in place: out = (1-op)*bottom + op*mode(top, bottom).
/// `top` may be a different size; pixels beyond its extent repeat the edge
/// (mimics `eof_action=repeat` / equal-size inputs in the script).
pub fn blend(bottom: &mut ImgF32, top: &ImgF32, mode: Mode, opacity: f32) {
    let row_stride = bottom.w * 4;
    bottom.data.par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let ty = y.min(top.h - 1);
            for x in 0..bottom.w {
                let tx = x.min(top.w - 1);
                let di = x * 4;
                let b = [row[di], row[di + 1], row[di + 2], row[di + 3]];
                let t = top.get(tx, ty);
                for c in 0..3 {
                    let blended = mode.apply(t[c], b[c]);
                    row[di + c] = (1.0 - opacity) * b[c] + opacity * blended;
                }
            }
        });
}

/// Scanline bloom: `blend=all_expr='if(gte(A,RNG/2), B+(1-B)*power*(A-.5)/.5, B)'`.
/// `bottom` (B) = desaturated image, `top` (A) = scanline luminance.
pub fn bloom_expr(bottom: &mut ImgF32, top: &ImgF32, power: f32) {
    let row_stride = bottom.w * 4;
    bottom.data.par_chunks_exact_mut(row_stride)
        .enumerate()
        .for_each(|(y, row)| {
            let ty = y.min(top.h - 1);
            for x in 0..bottom.w {
                let tx = x.min(top.w - 1);
                let di = x * 4;
                let b = [row[di], row[di + 1], row[di + 2], row[di + 3]];
                let t = top.get(tx, ty);
                for c in 0..3 {
                    let a = t[c];
                    row[di + c] = if a >= 0.5 {
                        b[c] + (1.0 - b[c]) * power * (a - 0.5) / 0.5
                    } else {
                        b[c]
                    };
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(w: usize, h: usize, val: f32) -> ImgF32 {
        let mut img = ImgF32::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.set(x, y, [val, val, val, 1.0]);
            }
        }
        img
    }

    #[test]
    fn multiply_zero() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 0.0);
        blend(&mut bot, &top, Mode::Multiply, 1.0);
        assert_eq!(bot.get(0, 0)[0], 0.0);
    }

    #[test]
    fn multiply_one() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 1.0);
        blend(&mut bot, &top, Mode::Multiply, 1.0);
        assert_eq!(bot.get(0, 0)[0], 0.3);
    }

    #[test]
    fn multiply_half() {
        let mut bot = constant(1, 1, 0.5);
        let top = constant(1, 1, 0.5);
        blend(&mut bot, &top, Mode::Multiply, 1.0);
        assert_eq!(bot.get(0, 0)[0], 0.25);
    }

    #[test]
    fn screen_zero() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 0.0);
        blend(&mut bot, &top, Mode::Screen, 1.0);
        assert_eq!(bot.get(0, 0)[0], 0.3);
    }

    #[test]
    fn screen_one() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 1.0);
        blend(&mut bot, &top, Mode::Screen, 1.0);
        assert_eq!(bot.get(0, 0)[0], 1.0);
    }

    #[test]
    fn opacity_zero_no_change() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 1.0);
        blend(&mut bot, &top, Mode::Multiply, 0.0);
        assert_eq!(bot.get(0, 0)[0], 0.3);
    }

    #[test]
    fn bloom_low_leaf_unchanged() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 0.4);
        bloom_expr(&mut bot, &top, 1.0);
        assert_eq!(bot.get(0, 0)[0], 0.3);
    }

    #[test]
    fn bloom_high_brightens() {
        let mut bot = constant(1, 1, 0.3);
        let top = constant(1, 1, 1.0);
        bloom_expr(&mut bot, &top, 1.0);
        assert!((bot.get(0, 0)[0] - 1.0).abs() < 1e-6);
    }
}
