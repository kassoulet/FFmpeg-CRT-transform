//! Benchmarks for individual DSP operations using Criterion.
//!
//! Run with:
//!   cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use crt_transform::image_buf::ImgF32;
use crt_transform::ops::{blur, lens, resample};

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

criterion_group!(benches, bench_blur, bench_lens, bench_resize);
criterion_main!(benches);
