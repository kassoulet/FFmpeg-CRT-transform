//! `ffcrt` — native-Rust CRT / flat-panel monitor simulation.
//!
//! Thin CLI wrapper over the `ffcrt` library. Phase A (still images) is
//! implemented end-to-end with no external binary. Video (Phase B) is not
//! yet wired up.

use anyhow::{bail, Result};
use clap::Parser;
use ffcrt::pipeline;
use std::path::{Path, PathBuf};

/// FFmpeg CRT transform — native Rust port (VileR 2021, ported 2026).
#[derive(Parser)]
#[command(name = "ffcrt", about, long_about = None)]
struct Cli {
    /// Configuration file (.cfg) — same format as the batch script presets
    config_file: PathBuf,
    /// Input image (PNG/JPG/TIF/BMP). Video is Phase B (not yet implemented).
    input_file: PathBuf,
    /// Output file. If omitted: "(input)_(config).(input_ext)".
    output_file: Option<PathBuf>,
    /// Dump each pipeline stage as a PNG into this directory (for debugging).
    #[arg(long, value_name = "DIR")]
    dump_stages: Option<PathBuf>,
}

fn derive_output(config: &Path, input: &Path) -> Result<PathBuf> {
    let ext = match input.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => bail!("Input file has no extension: {}", input.display()),
    };
    let in_stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
    let cfg_stem = config.file_stem().and_then(|s| s.to_str()).unwrap_or("cfg");
    let dir = input.parent().unwrap_or_else(|| Path::new("."));
    Ok(dir.join(format!("{in_stem}_{cfg_stem}.{ext}")))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if !cli.config_file.is_file() {
        bail!("File not found: {}", cli.config_file.display());
    }
    if !cli.input_file.is_file() {
        bail!("File not found: {}", cli.input_file.display());
    }

    let output = match cli.output_file {
        Some(o) => o,
        None => derive_output(&cli.config_file, &cli.input_file)?,
    };

    eprintln!(
        "ffcrt: {} + {} -> {}",
        cli.input_file.display(),
        cli.config_file.display(),
        output.display()
    );
    pipeline::run(&cli.config_file, &cli.input_file, &output, cli.dump_stages)?;
    eprintln!("Done: {}", output.display());
    Ok(())
}
