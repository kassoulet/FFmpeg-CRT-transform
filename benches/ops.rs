//! Benchmarks for individual DSP operations.
//!
//! Run with nightly Rust:
//!   rustup run nightly cargo bench
//!
//! Or with criterion (stable):
//!   cargo bench

use ffcrt::image_buf::ImgF32;
use ffcrt::ops::{blur, lens, resample};

fn img_small() -> ImgF32 {
    ImgF32::filled(640, 480, [0.5, 0.5, 0.5, 1.0])
}

fn img_large() -> ImgF32 {
    ImgF32::filled(3200, 2400, [0.5, 0.5, 0.5, 1.0])
}

#[cfg(feature = "bench")]
mod bench {
    use super::*;
    use test::Bencher;

    #[bench]
    fn blur_iso_small(b: &mut Bencher) {
        let img = img_small();
        b.iter(|| blur::gblur_iso(&img, 3.0));
    }

    #[bench]
    fn blur_iso_large(b: &mut Bencher) {
        let img = img_large();
        b.iter(|| blur::gblur_iso(&img, 3.0));
    }

    #[bench]
    fn lenscorrection_small(b: &mut Bencher) {
        let img = img_small();
        b.iter(|| lens::lenscorrection(&img, 0.06, 0.06));
    }

    #[bench]
    fn resize_lanczos_down(b: &mut Bencher) {
        let img = img_small();
        b.iter(|| resample::resize(&img, 320, 240, resample::Filter::Lanczos));
    }
}
