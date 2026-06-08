# Performance and Profiling Guide

This document outlines how to measure, analyze, and optimize the performance of the `crt-transform` Rust implementation.

## 1. Benchmarking with Criterion

We use [Criterion.rs](https://github.com/bheisler/criterion.rs) for statistics-driven benchmarking. It provides reliable measurements and HTML reports to track performance changes over time.

### Running Benchmarks
To run the standard benchmark suite:
```bash
cargo bench
```

### Viewing Reports
Criterion generates detailed HTML reports. After running benchmarks, you can find them at:
`target/criterion/report/index.html`

### Adding New Benchmarks
Benchmarks are located in the `benches/` directory. To add a new benchmark:
1. Open `benches/ops.rs`.
2. Create a new function for your benchmark.
3. Add it to the `criterion_group!` macro.

## 2. Instrumentation with the `profiling` crate

The project uses the `profiling` crate as a unified abstraction layer. This allows you to instrument code once and swap out profiling backends (like Puffin, Tracy, or Optick) at compile-time.

### instrumented Functions
Major pipeline stages in `src/pipeline.rs` are already instrumented with `#[profiling::function]`:
- `build_bezel`
- `build_scanlines`
- `build_shadowmask`
- `build_grid`

### Profiling with Puffin
[Puffin](https://github.com/EmbarkStudios/puffin) is an easy-to-use internal profiler.

1. **Run with Puffin enabled**:
   ```bash
   cargo run --features profile-with-puffin -- <config> <input> [output]
   ```
2. **View results**: Use a puffin viewer (like `puffin_viewer`) to connect to the running application.

## 3. Pre-commit Quality Checks

To ensure that performance-critical code remains well-formatted and idiomatic, we use **prek** (a high-performance Rust-based pre-commit tool).

### Installation
```bash
# If not already installed
prek install
```

### Manual Run
```bash
prek run --all-files
```

The hooks automatically run:
- `cargo fmt`: Ensures consistent formatting.
- `cargo clippy`: Catches common performance pitfalls and non-idiomatic code.
- Basic file hygiene (trailing whitespace, end-of-file fixers).

## 4. Sampling Profilers (System-wide)

For a lower-level view without instrumentation, you can use sampling profilers:

### samply (Linux, macOS, Windows)
```bash
cargo install samply
samply record cargo run --release -- <config> <input> [output]
```

### cargo-flamegraph
```bash
cargo install flamegraph
cargo flamegraph -- <config> <input> [output]
```
