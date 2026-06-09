//! `crt-transform` — native-Rust CRT / flat-panel monitor simulation.
//!
//! Thin CLI wrapper over the `crt_transform` library. Phase A (still images) is
//! implemented end-to-end with no external binary. Video (Phase B) is not
//! yet wired up.

use anyhow::{bail, Context, Result};
use clap::Parser;
use crt_transform::{config::Config, pipeline};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// CRT Transform — native Rust port (VileR 2021, ported 2026).
#[derive(Parser)]
#[command(name = "crt-transform", about, long_about = None)]
struct Cli {
    /// Configuration file (.cfg) — same format as the batch script presets
    config_file: PathBuf,
    /// Input image (PNG/JPG/TIF/BMP). Omit when using --validate or --batch.
    input_file: Option<PathBuf>,
    /// Output file. If omitted: "(input)_(config).(input_ext)".
    output_file: Option<PathBuf>,
    /// Dump each pipeline stage as a PNG into this directory (for debugging).
    #[arg(long, value_name = "DIR")]
    dump_stages: Option<PathBuf>,
    /// Parse the config, print all settings, check overlay files, then exit
    /// without loading or processing any image.  Exits 1 if warnings are found.
    #[arg(long)]
    validate: bool,
    /// Process all images in INPUT_DIR and write results to OUTPUT_DIR in
    /// parallel (one rayon thread per image).
    #[arg(long, num_args = 2, value_names = ["INPUT_DIR", "OUTPUT_DIR"])]
    batch: Option<Vec<PathBuf>>,
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"];

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

fn cmd_validate(cfg_path: &Path) -> Result<()> {
    let cfg = Config::load(cfg_path)?;

    println!("Config: {}", cfg_path.display());
    println!();

    // Print all settings sorted alphabetically.
    let entries = cfg.entries();
    if entries.is_empty() {
        println!("  (no settings)");
    } else {
        let key_width = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
        for (k, v) in &entries {
            println!("  {k:<key_width$}  =  {v}");
        }
    }
    println!();

    // Overlay file check.
    let ovl_alpha = cfg.f64_or("OVL_ALPHA", 0.0);
    if ovl_alpha > 0.0 {
        let ovl_type = cfg.str_or("OVL_TYPE", "triad");
        let mask_path = PathBuf::from(format!("_{ovl_type}.png"));
        if mask_path.exists() {
            println!("Overlay: {mask_path:?} — OK");
        } else {
            println!("Overlay: {mask_path:?} — NOT FOUND (will fail at run time)");
        }
    } else {
        println!("Overlay: disabled (OVL_ALPHA=0)");
    }
    println!();

    // Validation warnings.
    let warnings = cfg.validate();
    if warnings.is_empty() {
        println!("Warnings: none");
        Ok(())
    } else {
        println!("Warnings ({}):", warnings.len());
        for w in &warnings {
            println!("  ! {w}");
        }
        // Exit 1 so scripts can detect problems.
        std::process::exit(1);
    }
}

fn cmd_batch(cfg_path: &Path, input_dir: &Path, output_dir: &Path) -> Result<()> {
    if !input_dir.is_dir() {
        bail!("Not a directory: {}", input_dir.display());
    }
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Cannot create output dir: {}", output_dir.display()))?;

    // Collect all image files from the input directory (non-recursive).
    let inputs: Vec<PathBuf> = std::fs::read_dir(input_dir)
        .with_context(|| format!("Cannot read dir: {}", input_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .collect();

    if inputs.is_empty() {
        eprintln!(
            "crt-transform: no image files found in {}",
            input_dir.display()
        );
        return Ok(());
    }

    eprintln!(
        "crt-transform batch: {} image(s) in {} -> {}",
        inputs.len(),
        input_dir.display(),
        output_dir.display()
    );

    let cfg_stem = cfg_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cfg");
    let ok_count = AtomicUsize::new(0);
    let err_count = AtomicUsize::new(0);

    // Process in parallel; errors are logged per-file, not propagated.
    inputs.par_iter().for_each(|input| {
        let ext = match input.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => return,
        };
        let in_stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
        let out_name = format!("{in_stem}_{cfg_stem}.{ext}");
        let output = output_dir.join(&out_name);

        match pipeline::run(cfg_path, input, &output, None, None) {
            Ok(()) => {
                eprintln!(
                    "  ok  {}",
                    input.file_name().unwrap_or_default().to_string_lossy()
                );
                ok_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!(
                    "  ERR {}  — {e:#}",
                    input.file_name().unwrap_or_default().to_string_lossy()
                );
                err_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    });

    let ok = ok_count.load(Ordering::Relaxed);
    let err = err_count.load(Ordering::Relaxed);
    eprintln!("crt-transform batch: {ok} ok, {err} error(s)");
    if err > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn main() -> Result<()> {
    profiling::register_thread!("Main Thread");
    let cli = Cli::parse();

    if !cli.config_file.is_file() {
        bail!("File not found: {}", cli.config_file.display());
    }

    if cli.validate {
        return cmd_validate(&cli.config_file);
    }

    if let Some(dirs) = cli.batch {
        return cmd_batch(&cli.config_file, &dirs[0], &dirs[1]);
    }

    let input_file = match cli.input_file {
        Some(f) => f,
        None => bail!("Input file required (or use --validate / --batch)"),
    };

    if !input_file.is_file() {
        bail!("File not found: {}", input_file.display());
    }

    let output = match cli.output_file {
        Some(o) => o,
        None => derive_output(&cli.config_file, &input_file)?,
    };

    eprintln!(
        "crt-transform: {} + {} -> {}",
        input_file.display(),
        cli.config_file.display(),
        output.display()
    );
    pipeline::run(
        &cli.config_file,
        &input_file,
        &output,
        cli.dump_stages,
        Some(&|stage| eprintln!("[{stage}]")),
    )?;
    eprintln!("Done: {}", output.display());
    Ok(())
}
