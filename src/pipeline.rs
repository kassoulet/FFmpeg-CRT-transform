//! Still-image and video pipeline orchestration.
//!
//! Each function below reproduces one `::+++` section of `ffcrt.sh`, in the same
//! order, but passes in-memory `ImgF32` values between stages instead of writing
//! `TMP*` files. The stage boundaries match the script so output stays faithful.
//! (The plan's `stages/*.rs` tree is collapsed into these functions here — see
//! CLAUDE.md — to keep the data flow in one place.)
//!
//! [`run`] handles still images. [`run_video`] handles video files via
//! [`crate::video::FfmpegFrameSource`] / [`crate::video::FfmpegFrameSink`] with
//! optional LATENCY (tmix) and P_DECAY (lagfun) temporal effects.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::config::{Config, Derived};
use crate::image_buf::ImgF32;
use crate::monitor::{Monitor, Texture};
use crate::ops::{
    blend::{self, Mode},
    blur, crop, gamma, generate, lens,
    resample::{self, Filter},
    vignette,
};
use rayon::prelude::*;

/// Bundle of everything the stages read, resolved once.
struct Ctx<'p> {
    cfg: Config,
    d: Derived,
    mon: Monitor,
    scanlines_on: bool,
    ovl_alpha: f64,
    dump: Option<PathBuf>,
    progress: Option<&'p dyn Fn(&str)>,
}

impl Ctx<'_> {
    fn dump(&self, name: &str, img: &ImgF32) {
        if let Some(dir) = &self.dump {
            let _ = std::fs::create_dir_all(dir);
            let p = dir.join(format!("{name}.png"));
            let _ = img.save(&p, 8);
            eprintln!("  [dump] {}", p.display());
        }
    }

    fn progress(&self, stage: &str) {
        if let Some(f) = self.progress {
            f(stage);
        }
    }
}

/// Pre-built static layers shared across all frames of a run.
struct Layers {
    bezel: ImgF32,
    scanlines: Option<ImgF32>,
    shadowmask: ImgF32,
    grid: Option<ImgF32>,
}

/// Build context from config + first-frame dimensions.
fn make_ctx<'p>(
    cfg: Config,
    ix: i64,
    iy: i64,
    dump: Option<PathBuf>,
    progress: Option<&'p dyn Fn(&str)>,
) -> Result<Ctx<'p>> {
    let d = Derived::compute(&cfg, ix, iy)?;
    let lcd_grain = cfg.i64_or("LCD_GRAIN", 0);
    let mon = Monitor::resolve(&cfg.str_or("MONITOR_COLOR", "rgb"), lcd_grain);
    let scanlines_on = cfg.yes("SCANLINES_ON") && !d.flat_panel;
    let ovl_alpha = if d.flat_panel || !mon.is_color {
        0.0
    } else {
        cfg.f64_or("OVL_ALPHA", 0.0)
    };
    if ovl_alpha > 0.0 {
        let ovl_type = cfg.str_or("OVL_TYPE", "triad");
        let mask_path = PathBuf::from(format!("_{ovl_type}.png"));
        if !mask_path.exists() {
            bail!(
                "Shadow mask overlay '_{ovl_type}.png' not found in the working directory. \
                 Place the overlay file next to the input or set OVL_ALPHA=0 to disable it."
            );
        }
    }
    Ok(Ctx {
        cfg,
        d,
        mon,
        scanlines_on,
        ovl_alpha,
        dump,
        progress,
    })
}

/// Build the four static layers (bezel, scanlines, shadowmask, grid).
fn build_layers(ctx: &Ctx) -> Result<Layers> {
    ctx.progress("bezel");
    let bezel = build_bezel(ctx);
    ctx.dump("bezel", &bezel);

    let scanlines = if ctx.scanlines_on {
        ctx.progress("scanlines");
        let s = build_scanlines(ctx);
        ctx.dump("scanlines", &s);
        Some(s)
    } else {
        None
    };

    ctx.progress("shadowmask");
    let shadowmask = build_shadowmask(ctx)?;
    ctx.dump("shadowmask", &shadowmask);

    let grid = if ctx.d.flat_panel {
        ctx.progress("grid");
        let g = build_grid(ctx);
        ctx.dump("grid", &g);
        Some(g)
    } else {
        None
    };

    Ok(Layers {
        bezel,
        scanlines,
        shadowmask,
        grid,
    })
}

/// Process one frame through steps 01–03 + output (the CRT effect chain).
fn process_one_frame(ctx: &Ctx, img: ImgF32, layers: &Layers) -> Result<ImgF32> {
    ctx.progress("step01");
    let s01 = step01(ctx, &img, layers.grid.as_ref());
    ctx.dump("step01", &s01);

    ctx.progress("step02");
    let s02 = step02(ctx, s01);
    ctx.dump("step02", &s02);

    ctx.progress("step03");
    let s03 = step03(
        ctx,
        s02,
        layers.scanlines.as_ref(),
        &layers.shadowmask,
        &layers.bezel,
    );
    ctx.dump("step03", &s03);

    ctx.progress("output");
    output(ctx, s03)
}

/// Run the full still-image pipeline.
///
/// `progress` is an optional callback called with a short stage name before
/// each major stage begins (e.g. `"bezel"`, `"step01"`, `"output"`). Pass
/// `None` for no-op, or `Some(&|s| eprintln!("[{s}]"))` for simple logging.
pub fn run(
    cfg_path: &Path,
    input_path: &Path,
    output_path: &Path,
    dump: Option<PathBuf>,
    progress: Option<&dyn Fn(&str)>,
) -> Result<()> {
    let cfg = Config::load(cfg_path)?;
    for warning in cfg.validate() {
        eprintln!("crt-transform: warning: {warning}");
    }

    let ext = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v") {
        return run_video_inner(cfg, input_path, output_path, dump, progress);
    }

    let mut img = ImgF32::load(input_path)?;
    let ctx = make_ctx(cfg, img.w as i64, img.h as i64, dump, progress)?;

    if ctx.cfg.yes("INVERT_INPUT") {
        generate::negate(&mut img);
    }

    let layers = build_layers(&ctx)?;
    let out = process_one_frame(&ctx, img, &layers)?;
    out.save(output_path, ctx.d.output_bpc)?;
    Ok(())
}

/// Run the video pipeline: decode → temporal mix → CRT per-frame → encode.
///
/// LATENCY (tmix) and P_DECAY (lagfun) temporal effects are applied to the
/// processed output of each frame before writing, matching the ffcrt.sh
/// filter graph order.
pub fn run_video(
    cfg_path: &Path,
    input_path: &Path,
    output_path: &Path,
    dump: Option<PathBuf>,
    progress: Option<&dyn Fn(&str)>,
) -> Result<()> {
    let cfg = Config::load(cfg_path)?;
    for warning in cfg.validate() {
        eprintln!("crt-transform: warning: {warning}");
    }
    run_video_inner(cfg, input_path, output_path, dump, progress)
}

fn run_video_inner(
    cfg: Config,
    input_path: &Path,
    output_path: &Path,
    dump: Option<PathBuf>,
    progress: Option<&dyn Fn(&str)>,
) -> Result<()> {
    use crate::video::{FfmpegFrameSink, FfmpegFrameSource, FrameSink, TemporalMixer};

    let info = crate::video::probe_video(input_path)?;
    let ctx = make_ctx(cfg, info.w as i64, info.h as i64, dump, progress)?;

    let latency = ctx.cfg.i64_or("LATENCY", 0).max(0) as usize;
    let latency_alpha = ctx.cfg.f64_or("LATENCY_ALPHA", 0.0) as f32;
    let decay_factor = ctx.cfg.f64_or("P_DECAY_FACTOR", 0.0) as f32;
    let decay_alpha = ctx.cfg.f64_or("P_DECAY_ALPHA", 0.0) as f32;
    let mut mixer = TemporalMixer::new(latency, latency_alpha, decay_factor, decay_alpha);

    let layers = build_layers(&ctx)?;

    // Output dimensions come from the config (OY / OASPECT), not the input size.
    let out_w = ctx.d.ox as usize;
    let out_h = ctx.d.oy as usize;

    let invert = ctx.cfg.yes("INVERT_INPUT");
    let source = FfmpegFrameSource::open(input_path)?;
    let mut sink = FfmpegFrameSink::create(output_path, out_w, out_h, info.fps_num, info.fps_den)?;

    for frame_result in source {
        let mut frame = frame_result?;
        if invert {
            generate::negate(&mut frame);
        }
        let processed = process_one_frame(&ctx, frame, &layers)?;
        let mixed = mixer.mix(processed);
        sink.write(&mixed)?;
    }

    sink.finish()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Stage: bezel  (white canvas + rounded corners + bezel curvature)
// ---------------------------------------------------------------------------
#[profiling::function]
fn build_bezel(ctx: &Ctx) -> ImgF32 {
    let mut bezel = ImgF32::filled(ctx.d.px as usize, ctx.d.py as usize, [1.0, 1.0, 1.0, 1.0]);
    let radius = ctx.cfg.i64_or("CORNER_RADIUS", 0).max(0) as usize;
    generate::round_corners(&mut bezel, radius);
    if ctx.d.bezel_curvature != 0.0 {
        bezel = lens::lenscorrection(&bezel, ctx.d.bezel_curvature, ctx.d.bezel_curvature);
    }
    bezel
}

// ---------------------------------------------------------------------------
// Stage: scanlines  (sin^(1/weight) profile, tiled to PXxPY, blur + curvature)
// ---------------------------------------------------------------------------
#[profiling::function]
fn build_scanlines(ctx: &Ctx) -> ImgF32 {
    let weight = ctx.cfg.f64_or("SL_WEIGHT", 0.5).max(1e-3);
    // SCANLINE_PERIOD = PRESCALE_BY / SCAN_FACTOR (bc integer truncation).
    let period = ((ctx.d.prescale as f64) / ctx.d.scan_factor.factor())
        .trunc()
        .max(1.0) as usize;
    let col = generate::scanline_column(period, weight);
    // Build PXxPY directly by repeating the period (avoids tiling round-off).
    let px = ctx.d.px as usize;
    let py = ctx.d.py as usize;
    let mut s = ImgF32::new(px, py);
    s.for_each_row_mut(|y, row| {
        let lum = col.get(0, y % period);
        for x in 0..px {
            let di = x * 4;
            row[di..di + 4].copy_from_slice(&lum);
        }
    });
    // Soften + apply CRT curvature (script supersamples x3 for quality; we blur
    // at native res, see CLAUDE.md fidelity notes).
    s = blur::gblur_iso(&s, 0.8);
    if ctx.d.crt_curvature != 0.0 {
        s = lens::lenscorrection(&s, ctx.d.crt_curvature, ctx.d.crt_curvature);
    }
    s
}

// ---------------------------------------------------------------------------
// Stage: shadowmask overlay  (tile the _<type>.png mask, blur + curvature)
// ---------------------------------------------------------------------------
#[profiling::function]
fn build_shadowmask(ctx: &Ctx) -> Result<ImgF32> {
    let px = ctx.d.px as usize;
    let py = ctx.d.py as usize;
    if ctx.ovl_alpha <= 0.0 {
        // transparent canvas — a no-op under multiply at opacity 0
        return Ok(ImgF32::filled(px, py, [0.0, 0.0, 0.0, 0.0]));
    }

    let ovl_type = ctx.cfg.str_or("OVL_TYPE", "triad");
    let mask_path = PathBuf::from(format!("_{ovl_type}.png"));
    let mut mask = ImgF32::load(&mask_path)?;
    gamma::to_linear(&mut mask);

    let ovl_scale = ctx.cfg.f64_or("OVL_SCALE", 0.125);
    let mw = ((mask.w as f64 * ovl_scale).round() as usize).max(1);
    let mh = ((mask.h as f64 * ovl_scale).round() as usize).max(1);
    let mask = resample::resize(&mask, mw, mh, Filter::Lanczos);

    // tile to cover PXxPY
    let mut tiled = ImgF32::new(px, py);
    tiled.for_each_row_mut(|y, row| {
        let sy = y % mh;
        for x in 0..px {
            let sx = x % mw;
            let p = mask.get(sx, sy);
            let di = x * 4;
            row[di..di + 4].copy_from_slice(&p);
        }
    });
    tiled = blur::gblur_iso(&tiled, 1.0);
    if ctx.d.crt_curvature != 0.0 {
        tiled = lens::lenscorrection(&tiled, ctx.d.crt_curvature, ctx.d.crt_curvature);
    }
    gamma::from_linear(&mut tiled);
    Ok(tiled)
}

// ---------------------------------------------------------------------------
// Stage: flat-panel pixel grid  (gap pattern, gamma, scaled to PX wide)
// ---------------------------------------------------------------------------
#[profiling::function]
fn build_grid(ctx: &Ctx) -> ImgF32 {
    let pxgrid_alpha = ctx.cfg.f64_or("PXGRID_ALPHA", 0.0) as f32;
    let (lum_gap, lum_px) = if ctx.mon.pxgrid_invert {
        (pxgrid_alpha, 0.0)
    } else {
        (1.0 - pxgrid_alpha, 1.0)
    };
    let gx = (ctx.d.prescale / ctx.cfg.i64_or("PX_FACTOR_X", 1).max(1)).max(1) as usize;
    let gy = (ctx.d.prescale / ctx.cfg.i64_or("PX_FACTOR_Y", 1).max(1)).max(1) as usize;
    let gap_x = ctx.cfg.i64_or("PX_X_GAP", 0).max(0) as usize;
    let gap_y = ctx.cfg.i64_or("PX_Y_GAP", 0).max(0) as usize;

    let mut grid = generate::pixel_grid(
        ctx.d.sxint as usize,
        ctx.d.py as usize,
        gx,
        gy,
        gap_x,
        gap_y,
        lum_gap,
        lum_px,
    );
    gamma::to_linear(&mut grid);
    grid = resample::resize(&grid, ctx.d.px as usize, ctx.d.py as usize, Filter::Bicubic);
    gamma::from_linear(&mut grid);
    grid
}

// ---------------------------------------------------------------------------
// Stage 01: prescale (neighbor) + to-linear + aspect + grid + pixel blur
// ---------------------------------------------------------------------------
fn step01(ctx: &Ctx, img: &ImgF32, grid: Option<&ImgF32>) -> ImgF32 {
    let prescale = ctx.d.prescale as usize;
    // horizontal nearest prescale
    let mut cur = resample::nearest_scale(img, prescale, 1);
    gamma::to_linear(&mut cur);
    // aspect correction to exactly PX wide (fast_bilinear)
    cur = resample::resize(&cur, ctx.d.px as usize, cur.h, Filter::FastBilinear);
    // vertical nearest prescale -> PXxPY
    cur = resample::nearest_scale(&cur, 1, prescale);

    if let Some(grid) = grid {
        let mode = if ctx.mon.pxgrid_invert {
            Mode::Screen
        } else {
            Mode::Multiply
        };
        blend::blend(&mut cur, grid, mode, 1.0);
    }

    let h_px_blur = ctx.cfg.f64_or("H_PX_BLUR", 0.0);
    let sigma_h = h_px_blur / 100.0 * ctx.d.prescale as f64 * ctx.d.px_aspect.as_f64();
    cur = blur::gblur(&cur, sigma_h, ctx.d.vsigma);
    cur
}

// ---------------------------------------------------------------------------
// Stage 02: halation + from-linear + blackpoint + CRT curvature
// ---------------------------------------------------------------------------
fn step02(ctx: &Ctx, mut img: ImgF32) -> ImgF32 {
    if ctx.cfg.yes("HALATION_ON") {
        let radius = ctx.cfg.f64_or("HALATION_RADIUS", 0.0);
        let alpha = ctx.cfg.f64_or("HALATION_ALPHA", 0.0) as f32;
        let halo = blur::gblur_iso(&img, radius);
        blend::blend(&mut img, &halo, Mode::Lighten, alpha);
        gamma::from_linear_halation(&mut img);
    } else {
        gamma::from_linear(&mut img);
    }
    generate::blackpoint(&mut img, ctx.cfg.f64_or("BLACKPOINT", 0.0));
    if ctx.d.crt_curvature != 0.0 {
        img = lens::lenscorrection(&img, ctx.d.crt_curvature, ctx.d.crt_curvature);
    }
    img
}

// ---------------------------------------------------------------------------
// Stage 03: bloom + scanlines + shadowmask + bezel + brighten
// ---------------------------------------------------------------------------
fn step03(
    ctx: &Ctx,
    step02: ImgF32,
    scanlines: Option<&ImgF32>,
    shadowmask: &ImgF32,
    bezel: &ImgF32,
) -> ImgF32 {
    let brighten = ctx.cfg.f64_or("BRIGHTEN", 1.0);
    let skip_ovl = ctx.ovl_alpha == 0.0;
    let skip_bri = brighten == 1.0;
    let curvature_equal = ctx.d.bezel_curvature == ctx.d.crt_curvature;
    let corner_radius = ctx.cfg.i64_or("CORNER_RADIUS", 0);

    // The script's redundancy guard: nothing in step03 would change the image.
    if !ctx.scanlines_on && curvature_equal && corner_radius == 0 && skip_ovl && skip_bri {
        return step02;
    }

    // Compute the scanline texture before consuming step02, so bloom can clone
    // it without paying for an extra full-image copy in the non-bloom path.
    let sl_tex = scanlines.map(|sl| {
        if ctx.cfg.yes("BLOOM_ON") {
            let power = ctx.cfg.f64_or("BLOOM_POWER", 0.0) as f32;
            let mut desat = step02.clone();
            gamma::to_linear(&mut desat);
            generate::to_gray(&mut desat);
            gamma::from_linear(&mut desat);
            blend::bloom_expr(&mut desat, sl, power);
            desat
        } else {
            sl.clone()
        }
    });

    let mut cur = step02; // move — no clone needed here

    if let Some(sl_tex) = sl_tex {
        let sl_alpha = ctx.cfg.f64_or("SL_ALPHA", 1.0) as f32;
        blend::blend(&mut cur, &sl_tex, Mode::Multiply, sl_alpha);
    }

    // shadowmask multiply (opacity 0 when not a color monitor -> no-op)
    blend::blend(&mut cur, shadowmask, Mode::Multiply, ctx.ovl_alpha as f32);
    // bezel multiply (full opacity)
    blend::blend(&mut cur, bezel, Mode::Multiply, 1.0);
    // brightness fix
    generate::brighten(&mut cur, brighten);
    cur
}

// ---------------------------------------------------------------------------
// Stage: output  (crop + rescale + monochrome + vignette + pad + texture)
// ---------------------------------------------------------------------------
fn output(ctx: &Ctx, step03: ImgF32) -> Result<ImgF32> {
    // crop area: bounding box of the curved white screen reference
    let mut white = ImgF32::filled(ctx.d.px as usize, ctx.d.py as usize, [1.0, 1.0, 1.0, 1.0]);
    if ctx.d.crt_curvature != 0.0 {
        white = lens::lenscorrection(&white, ctx.d.crt_curvature, ctx.d.crt_curvature);
    }
    let rect = crop::detect(&white, 0.001);
    let mut cur = crop::crop(&step03, &rect);

    // to-linear, then monochrome gray (MONO_STR1)
    gamma::to_linear(&mut cur);
    if !ctx.mon.is_color {
        generate::to_gray(&mut cur);
    }

    // scale to (OX-2*margin) x (OY-2*margin) preserving aspect (decrease)
    let margin = ctx.d.omargin.max(0) as usize;
    let box_w = (ctx.d.ox as usize).saturating_sub(2 * margin).max(1);
    let box_h = (ctx.d.oy as usize).saturating_sub(2 * margin).max(1);
    let scale = (box_w as f64 / cur.w as f64).min(box_h as f64 / cur.h as f64);
    let nw = ((cur.w as f64 * scale).round() as usize).max(1);
    let nh = ((cur.h as f64 * scale).round() as usize).max(1);
    let ofilter = Filter::parse(&ctx.cfg.str_or("OFILTER", "bicubic"));
    cur = resample::resize(&cur, nw, nh, ofilter);

    // from-linear, then monochrome tint (MONO_STR2)
    gamma::from_linear(&mut cur);
    if !ctx.mon.is_color {
        apply_mono_tint(ctx, &mut cur);
    }

    // vignette
    if ctx.cfg.yes("VIGNETTE_ON") {
        vignette::vignette(&mut cur, ctx.cfg.f64_or("VIGNETTE_POWER", 0.0));
    }

    // pad/center to OX x OY (black)
    let mut out = ImgF32::filled(ctx.d.ox as usize, ctx.d.oy as usize, [0.0, 0.0, 0.0, 1.0]);
    let ox0 = (out.w.saturating_sub(cur.w)) / 2;
    let oy0 = (out.h.saturating_sub(cur.h)) / 2;
    let copy_h = cur.h.min(out.h);
    let copy_w = cur.w.min(out.w);
    out.data
        .par_chunks_exact_mut(out.w * 4)
        .skip(oy0)
        .take(copy_h)
        .enumerate()
        .for_each(|(y, row)| {
            let cy = y;
            for x in 0..copy_w {
                let p = cur.get(x, cy);
                let di = (ox0 + x) * 4;
                row[di..di + 4].copy_from_slice(&p);
            }
        });

    // texture overlay (paper / lcdgrain)
    match ctx.mon.texture {
        Texture::Paper => apply_paper(ctx, &mut out),
        Texture::LcdGrain => apply_lcdgrain(ctx, &mut out),
        Texture::None => {}
    }

    Ok(out)
}

fn apply_mono_tint(ctx: &Ctx, img: &mut ImgF32) {
    if ctx.mon.is_p7 {
        // p7 still: split -> lat curves, decay curves, lighten, screen w/ orig.
        // img acts as "orig" — we blend the result back into it at the end,
        // saving one full-image clone vs keeping a separate `orig` copy.
        let p_decay_alpha = ctx.cfg.f64_or("P_DECAY_ALPHA", 0.3) as f32;
        let mut lat = img.clone();
        if let Some(c) = &ctx.mon.p7_lat {
            c.apply(&mut lat);
        }
        let mut decay = img.clone();
        if let Some(c) = &ctx.mon.p7_dec {
            c.apply(&mut decay);
        }
        blend::blend(&mut lat, &decay, Mode::Lighten, p_decay_alpha);
        blend::blend(img, &lat, Mode::Screen, 1.0);
    } else if let Some(c) = &ctx.mon.curves {
        c.apply(img);
    }
}

// Paper substrate: noise -> contrast stretch -> 3-tone palette -> blur, multiply.
fn apply_paper(ctx: &Ctx, out: &mut ImgF32) {
    use crate::ops::noise;
    let oaspect = ctx.d.oaspect.as_f64();
    let paperx = ((ctx.d.oy as f64 * oaspect * 67.0 / 100.0) as usize).max(1);
    let papery = ((ctx.d.oy as f64 * 67.0 / 100.0) as usize).max(1);
    let mut tex = noise::gray_noise(paperx, papery, 5150, 100.0);
    // contrast stretch ((v-70/255)*255/115) and 3-tone palette
    tex.data.par_chunks_exact_mut(4).for_each(|px| {
        let v = ((px[0] - 70.0 / 255.0) * 255.0 / 115.0).clamp(0.0, 1.0);
        let band = (v * 255.0) as i32;
        let (r, g, b) = if band <= 101 {
            (207.0, 238.0, 255.0)
        } else if band <= 203 {
            (253.0, 225.0, 157.0)
        } else {
            (251.0, 204.0, 255.0)
        };
        px[0] = r / 255.0;
        px[1] = g / 255.0;
        px[2] = b / 255.0;
    });
    gamma::to_linear(&mut tex);
    let mut tex = resample::resize(&tex, out.w, out.h, Filter::Bilinear);
    tex = blur::gblur_iso(&tex, 3.0);
    gamma::from_linear(&mut tex);
    blend::blend(out, &tex, Mode::Multiply, 1.0);
}

// LCD grain: gray noise scaled up; vividlight w/ image then lighten-clamped.
fn apply_lcdgrain(ctx: &Ctx, out: &mut ImgF32) {
    use crate::ops::noise;
    let grain = ctx.cfg.f64_or("LCD_GRAIN", 0.0);
    let oaspect = ctx.d.oaspect.as_f64();
    let gx = ((ctx.d.oy as f64 * oaspect * 50.0 / 100.0) as usize).max(1);
    let gy = ((ctx.d.oy as f64 * 50.0 / 100.0) as usize).max(1);
    let tex = noise::gray_noise(gx, gy, 5150, grain);
    let tex = resample::resize(&tex, out.w, out.h, Filter::Lanczos);

    // notquite = vividlight(top=image, bottom=tex); tex is moved here (no clone)
    let mut notquite = tex;
    blend::blend(&mut notquite, out, Mode::VividLight, 1.0);
    // fix = clamp(image, 0, 110/256); out = lighten(top=notquite, bottom=fix)
    let lim = 110.0 / 256.0;
    out.map_rgb(|v| v.min(lim));
    blend::blend(out, &notquite, Mode::Lighten, 1.0);
}
