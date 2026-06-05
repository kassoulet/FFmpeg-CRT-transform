//! `curves=` evaluator — natural cubic spline through control points, per
//! channel, used for the monochrome `MONITOR_COLOR` tint maps.
//!
//! Control-point strings look like `0/0 .77/0 1/.45`. ffmpeg builds a natural
//! cubic spline through the points, clamps to 0..1, and bakes a LUT. We
//! evaluate the spline directly on the float input.

use crate::image_buf::ImgF32;

/// One channel's control points, as a natural cubic spline ready to evaluate.
pub struct Spline {
    xs: Vec<f64>,
    ys: Vec<f64>,
    /// second derivatives at the knots
    y2: Vec<f64>,
}

impl Spline {
    fn from_points(mut pts: Vec<(f64, f64)>) -> Spline {
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let n = pts.len();
        let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
        let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
        let mut y2 = vec![0.0; n];
        if n >= 3 {
            // Natural cubic spline (Numerical Recipes "spline"): y2[0]=y2[n-1]=0.
            let mut u = vec![0.0; n];
            for i in 1..n - 1 {
                let sig = (xs[i] - xs[i - 1]) / (xs[i + 1] - xs[i - 1]);
                let p = sig * y2[i - 1] + 2.0;
                y2[i] = (sig - 1.0) / p;
                let d = (ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i])
                    - (ys[i] - ys[i - 1]) / (xs[i] - xs[i - 1]);
                u[i] = (6.0 * d / (xs[i + 1] - xs[i - 1]) - sig * u[i - 1]) / p;
            }
            for k in (0..n - 1).rev() {
                y2[k] = y2[k] * y2[k + 1] + u[k];
            }
        }
        Spline { xs, ys, y2 }
    }

    fn eval(&self, x: f64) -> f64 {
        let n = self.xs.len();
        if n == 0 {
            return x;
        }
        if x <= self.xs[0] {
            return self.ys[0].clamp(0.0, 1.0);
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1].clamp(0.0, 1.0);
        }
        // locate interval
        let mut hi = n - 1;
        let mut lo = 0;
        while hi - lo > 1 {
            let mid = (hi + lo) / 2;
            if self.xs[mid] > x {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let h = self.xs[hi] - self.xs[lo];
        if h == 0.0 {
            return self.ys[lo].clamp(0.0, 1.0);
        }
        let a = (self.xs[hi] - x) / h;
        let b = (x - self.xs[lo]) / h;
        let y = a * self.ys[lo]
            + b * self.ys[hi]
            + ((a * a * a - a) * self.y2[lo] + (b * b * b - b) * self.y2[hi]) * (h * h) / 6.0;
        y.clamp(0.0, 1.0)
    }
}

/// The three per-channel splines parsed from a `curves=r='..':g='..':b='..'`
/// specification.
pub struct Curves {
    pub r: Spline,
    pub g: Spline,
    pub b: Spline,
}

fn parse_points(spec: &str) -> Vec<(f64, f64)> {
    spec.split_whitespace()
        .filter_map(|tok| {
            let (x, y) = tok.split_once('/')?;
            Some((x.parse().ok()?, y.parse().ok()?))
        })
        .collect()
}

impl Curves {
    /// `r_spec`/`g_spec`/`b_spec` are the bare point lists (no `r='...'` wrapper).
    pub fn new(r_spec: &str, g_spec: &str, b_spec: &str) -> Curves {
        Curves {
            r: Spline::from_points(parse_points(r_spec)),
            g: Spline::from_points(parse_points(g_spec)),
            b: Spline::from_points(parse_points(b_spec)),
        }
    }

    pub fn apply(&self, img: &mut ImgF32) {
        for px in img.data.chunks_exact_mut(4) {
            px[0] = self.r.eval(px[0] as f64) as f32;
            px[1] = self.g.eval(px[1] as f64) as f32;
            px[2] = self.b.eval(px[2] as f64) as f32;
        }
    }
}
