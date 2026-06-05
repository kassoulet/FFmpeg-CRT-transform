//! DSP primitives. Each submodule maps to one row of the `ops/` plan and is
//! deliberately ffmpeg-filter-shaped so stages read like the batch script.

pub mod blend;
pub mod blur;
pub mod crop;
pub mod curves;
pub mod gamma;
pub mod generate;
pub mod lens;
pub mod noise;
pub mod resample;
pub mod vignette;
