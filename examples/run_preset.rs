/// Minimal example: apply a preset to an image.
///
/// Usage:  cargo run --example run_preset -- <preset.cfg> <input.png> [output.png]
///
/// If output is omitted, writes to `out.png` in the current directory.
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <config.cfg> <input.png> [output.png]", args[0]);
        std::process::exit(1);
    }
    let config = PathBuf::from(&args[1]);
    let input = PathBuf::from(&args[2]);
    let output = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("out.png"));

    crt_transform::run(&config, &input, &output, None, None).unwrap();
    eprintln!("Written to {}", output.display());
}
