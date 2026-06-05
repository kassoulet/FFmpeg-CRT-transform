//! `MONITOR_COLOR` handling — the big `case` block in `ffcrt.sh`.
//!
//! `rgb` is the color path (shadowmask enabled). Every other value is a
//! monochrome phosphor/panel: the image is desaturated to gray at the output
//! stage and tinted through a per-type `curves` spline. A few types carry extra
//! behaviour: `paperwhite` (paper texture), `lcd*` (inverted pixel grid + grain),
//! and `p7` (latency/decay split — its still-image form is the simpler split).

use crate::ops::curves::Curves;

#[derive(Clone, Copy, PartialEq)]
pub enum Texture {
    None,
    Paper,
    LcdGrain,
}

pub struct Monitor {
    pub is_color: bool,
    /// gray-tint curves for monochrome types (None for `rgb`/`white`/`paperwhite`)
    pub curves: Option<Curves>,
    pub texture: Texture,
    /// lcd types invert the pixel grid (gap bright, pixel dark)
    pub pxgrid_invert: bool,
    pub is_p7: bool,
    /// p7 still-image latency/decay tint curves (applied to the split copies)
    pub p7_lat: Option<Curves>,
    pub p7_dec: Option<Curves>,
}

impl Monitor {
    pub fn resolve(monitor_color: &str, lcd_grain: i64) -> Monitor {
        let mc = monitor_color.to_ascii_lowercase();
        let mut m = Monitor {
            is_color: mc == "rgb",
            curves: None,
            texture: Texture::None,
            pxgrid_invert: false,
            is_p7: mc == "p7",
            p7_lat: None,
            p7_dec: None,
        };

        match mc.as_str() {
            "rgb" | "white" => {}
            "paperwhite" => m.texture = Texture::Paper,
            "green1" => {
                m.curves = Some(Curves::new(
                    "0/0 .77/0 1/.45",
                    "0/0 .77/1 1/1",
                    "0/0 .77/.17 1/.73",
                ))
            }
            "green2" => {
                m.curves = Some(Curves::new(
                    "0/0 .43/.16 .72/.30 1/.56",
                    "0/0 .51/.53 .82/1 1/1",
                    "0/0 .43/.16 .72/.30 1/.56",
                ))
            }
            "bw-tv" => {
                m.curves = Some(Curves::new(
                    "0/0 .5/.49 1/1",
                    "0/0 .5/.49 1/1",
                    "0/0 .5/.62 1/1",
                ))
            }
            "amber" => {
                m.curves = Some(Curves::new(
                    "0/0 .25/.45 .8/1 1/1",
                    "0/0 .25/.14 .8/.55 1/.8",
                    "0/0 .8/0 1/.29",
                ))
            }
            "plasma" => {
                m.curves = Some(Curves::new(
                    "0/0 .13/.27 .52/.83 .8/1 1/1",
                    "0/0 .13/0 .52/.14 .8/.35 1/.54",
                    "0/0 1/0",
                ))
            }
            "eld" => {
                m.curves = Some(Curves::new(
                    "0/0 .46/.49 1/1",
                    "0/0 .46/.37 1/.94",
                    "0/0 .46/0 1/.29",
                ))
            }
            "lcd" => {
                m.curves = Some(Curves::new("0/.09 1/.48", "0/.11 1/.56", "0/.20 1/.35"));
                m.pxgrid_invert = true;
            }
            "lcd-lite" => {
                m.curves = Some(Curves::new("0/.06 1/.64", "0/.15 1/.77", "0/.35 1/.65"));
                m.pxgrid_invert = true;
            }
            "lcd-lwhite" => {
                m.curves = Some(Curves::new("0/.09 1/.82", "0/.18 1/.89", "0/.29 1/.93"));
                m.pxgrid_invert = true;
            }
            "lcd-lblue" => {
                m.curves = Some(Curves::new("0/.00 1/.62", "0/.22 1/.75", "0/.73 1/.68"));
                m.pxgrid_invert = true;
            }
            "p7" => {
                m.p7_lat = Some(Curves::new(
                    "0/0 .6/.31 1/.75",
                    "0/0 .25/.16 .75/.83 1/.94",
                    "0/0 .5/.76 1/.97",
                ));
                m.p7_dec = Some(Curves::new(
                    "0/0 .5/.36 1/.86",
                    "0/0 .5/.52 1/.89",
                    "0/0 .5/.08 1/.13",
                ));
            }
            _ => {}
        }

        // lcd grain texture only for lcd* types with LCD_GRAIN > 0
        if mc.starts_with("lcd") && lcd_grain > 0 {
            m.texture = Texture::LcdGrain;
        }

        m
    }
}
