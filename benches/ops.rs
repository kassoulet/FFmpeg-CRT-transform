//! Benchmarks for individual DSP operations using Criterion.
//!
//! Run with:
//!   cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crt_transform::image_buf::ImgF32;
use crt_transform::ops::{blend, blur, gamma, generate, lens, resample, vignette};
use std::path::Path;

fn img_small() -> ImgF32 {
    ImgF32::filled(640, 480, [0.5, 0.5, 0.5, 1.0])
}

fn img_large() -> ImgF32 {
    ImgF32::filled(3200, 2400, [0.5, 0.5, 0.5, 1.0])
}

fn bench_blur(c: &mut Criterion) {
    let mut group = c.benchmark_group("blur");
    let small = img_small();
    group.bench_function("iso_small", |b| {
        b.iter(|| blur::gblur_iso(black_box(&small), 3.0))
    });

    let large = img_large();
    group.bench_function("iso_large", |b| {
        b.iter(|| blur::gblur_iso(black_box(&large), 3.0))
    });
    group.finish();
}

fn bench_lens(c: &mut Criterion) {
    let img = img_small();
    c.bench_function("lens_small", |b| {
        b.iter(|| lens::lenscorrection(black_box(&img), 0.06, 0.06))
    });
}

fn bench_resize(c: &mut Criterion) {
    let img = img_small();
    c.bench_function("resize_lanczos_down", |b| {
        b.iter(|| resample::resize(black_box(&img), 320, 240, resample::Filter::Lanczos))
    });
}

fn bench_gamma(c: &mut Criterion) {
    let mut group = c.benchmark_group("gamma");
    group.bench_function("to_linear", |b| {
        b.iter(|| {
            let mut img = img_small();
            gamma::to_linear(black_box(&mut img));
            img
        })
    });
    group.bench_function("from_linear", |b| {
        b.iter(|| {
            let mut img = img_small();
            gamma::from_linear(black_box(&mut img));
            img
        })
    });
    group.bench_function("roundtrip", |b| {
        b.iter(|| {
            let mut img = img_small();
            gamma::to_linear(black_box(&mut img));
            gamma::from_linear(black_box(&mut img));
            img
        })
    });
    group.finish();
}

fn bench_blend(c: &mut Criterion) {
    let mut group = c.benchmark_group("blend");
    let top = img_small();
    group.bench_function("multiply_0.75", |b| {
        b.iter(|| {
            let mut bot = img_small();
            blend::blend(
                black_box(&mut bot),
                black_box(&top),
                blend::Mode::Multiply,
                0.75,
            );
            bot
        })
    });
    group.bench_function("screen_0.75", |b| {
        b.iter(|| {
            let mut bot = img_small();
            blend::blend(
                black_box(&mut bot),
                black_box(&top),
                blend::Mode::Screen,
                0.75,
            );
            bot
        })
    });
    group.finish();
}

fn bench_vignette(c: &mut Criterion) {
    let img = img_small();
    c.bench_function("vignette_0.3", |b| {
        b.iter(|| {
            let mut v = img.clone();
            vignette::vignette(black_box(&mut v), 0.3);
            v
        })
    });
}

fn bench_to_gray(c: &mut Criterion) {
    let img = img_small();
    c.bench_function("to_gray_rec601", |b| {
        b.iter(|| {
            let mut v = img.clone();
            generate::to_gray(black_box(&mut v));
            v
        })
    });
}

/// End-to-end pipeline through a color CRT preset on a 640×480 input.
///
/// Uses `benches/color-fast.cfg` (PRESCALE_BY=2, OY=480) to keep each
/// iteration under ~2 s while exercising all major branches: shadowmask,
/// scanlines, bloom, halation, CRT curvature, vignette.
/// This is the canonical regression number for the full pipeline.
fn bench_full_pipeline(c: &mut Criterion) {
    let cfg = Path::new("benches/color-fast.cfg");
    let input = Path::new("test-suite/08.png");
    let output = std::env::temp_dir().join("crt-bench-pipeline-out.png");

    // Sanity check: fail loudly at bench startup rather than silently skip.
    assert!(cfg.exists(), "benches/color-fast.cfg not found");
    assert!(input.exists(), "test-suite/08.png not found");

    let mut group = c.benchmark_group("pipeline");
    group.sample_size(20);
    group.bench_function("color_640x480", |b| {
        b.iter(|| {
            crt_transform::run(
                black_box(cfg),
                black_box(input),
                black_box(&output),
                None,
                None,
            )
            .expect("pipeline run failed");
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_blur,
    bench_lens,
    bench_resize,
    bench_gamma,
    bench_blend,
    bench_vignette,
    bench_to_gray,
    bench_full_pipeline
);
criterion_main!(benches);
