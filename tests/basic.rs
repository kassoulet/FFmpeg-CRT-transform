use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_crt-transform"))
}

/// Run the binary with the given config + input, write output to a temp file,
/// assert success, assert the output file exists and is larger than `min_bytes`.
fn run_preset_test(cfg: &str, input: &str, tag: &str, min_bytes: u64) {
    let tmp = std::env::temp_dir().join(format!("crt-transform-test-{tag}.png"));
    let _ = std::fs::remove_file(&tmp);

    let out = bin().arg(cfg).arg(input).arg(&tmp).output().unwrap();

    assert!(
        out.status.success(),
        "{tag} stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp.exists(), "{tag}: output file was not created");

    let len = std::fs::metadata(&tmp).unwrap().len();
    assert!(len > min_bytes, "{tag}: output too small ({len} bytes)");

    let _ = std::fs::remove_file(&tmp);
}

// ---------------------------------------------------------------------------
// --batch tests (fast: uses fpanel-fast.cfg with PRESCALE_BY=1)
// ---------------------------------------------------------------------------

#[test]
fn batch_processes_directory() {
    let input_dir = std::env::temp_dir().join("crt-batch-test-in");
    let output_dir = std::env::temp_dir().join("crt-batch-test-out");
    let _ = std::fs::remove_dir_all(&input_dir);
    let _ = std::fs::remove_dir_all(&output_dir);
    std::fs::create_dir_all(&input_dir).unwrap();

    // Copy a small test image into the temp input dir.
    std::fs::copy("test-suite/08.png", input_dir.join("08.png")).unwrap();

    let out = bin()
        .arg("test-suite/fpanel-fast.cfg")
        .arg("--batch")
        .arg(&input_dir)
        .arg(&output_dir)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Output file should exist with the expected name.
    let expected = output_dir.join("08_fpanel-fast.png");
    assert!(
        expected.exists(),
        "output file not found: {}",
        expected.display()
    );
    assert!(std::fs::metadata(&expected).unwrap().len() > 1000);

    let _ = std::fs::remove_dir_all(&input_dir);
    let _ = std::fs::remove_dir_all(&output_dir);
}

#[test]
fn batch_empty_dir_exits_zero() {
    let input_dir = std::env::temp_dir().join("crt-batch-test-empty");
    let _ = std::fs::remove_dir_all(&input_dir);
    std::fs::create_dir_all(&input_dir).unwrap();

    let out = bin()
        .arg("test-suite/fpanel-fast.cfg")
        .arg("--batch")
        .arg(&input_dir)
        .arg(std::env::temp_dir().join("crt-batch-test-empty-out"))
        .output()
        .unwrap();

    assert!(out.status.success());
    let _ = std::fs::remove_dir_all(&input_dir);
}

// ---------------------------------------------------------------------------
// --validate tests (fast, no image processing)
// ---------------------------------------------------------------------------

#[test]
fn validate_clean_preset_exits_zero() {
    // test-suite/08cfg.cfg has no validation warnings.
    let out = bin()
        .arg("test-suite/08cfg.cfg")
        .arg("--validate")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Warnings: none"));
}

#[test]
fn validate_prints_all_keys() {
    let out = bin()
        .arg("test-suite/08cfg.cfg")
        .arg("--validate")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("MONITOR_COLOR"));
    assert!(stdout.contains("PRESCALE_BY"));
    assert!(stdout.contains("OVL_TYPE"));
}

#[test]
fn validate_overlay_ok_when_present() {
    let out = bin()
        .arg("test-suite/08cfg.cfg")
        .arg("--validate")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("OK"));
}

// ---------------------------------------------------------------------------
// Error-handling tests (fast)
// ---------------------------------------------------------------------------

#[test]
fn cli_rejects_missing_file() {
    let out = bin()
        .arg("nonexistent.cfg")
        .arg("test-suite/08.png")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not found"));
}

#[test]
fn cli_rejects_video_input() {
    let out = bin()
        .arg("test-suite/01cfg.cfg")
        .arg("test-suite/01.mp4")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Phase B"));
}

// ---------------------------------------------------------------------------
// Fast per-family smoke tests  (PRESCALE_BY=2, OY=480 — run in <5 s each)
// These run as part of the default `cargo test` suite.
// ---------------------------------------------------------------------------

/// Color family — RGB shadowmask + scanlines (fast config).
#[test]
fn cli_runs_color_fast() {
    run_preset_test(
        "benches/color-fast.cfg",
        "test-suite/08.png",
        "color-fast",
        1000,
    );
}

/// Mono family — paperwhite tint, scanlines, halation (fast config).
#[test]
fn cli_runs_mono_fast() {
    run_preset_test(
        "test-suite/mono-fast.cfg",
        "test-suite/06.png",
        "mono-fast",
        1000,
    );
}

/// Amber mono family — non-square pixels, fat-beam scanlines (fast config).
#[test]
fn cli_runs_amber_fast() {
    run_preset_test(
        "test-suite/amber-fast.cfg",
        "test-suite/09.png",
        "amber-fast",
        1000,
    );
}

/// P7 family — dual-phosphor path with special curve handling.
#[test]
fn cli_runs_p7_preset() {
    run_preset_test("test-suite/p7fast.cfg", "test-suite/08.png", "p7", 1000);
}

/// Flat-panel / LCD family — pixel-grid path, no scanlines or curvature.
#[test]
fn cli_runs_fpanel_preset() {
    run_preset_test(
        "test-suite/fpanel-fast.cfg",
        "test-suite/08.png",
        "fpanel",
        1000,
    );
}

// ---------------------------------------------------------------------------
// Reference-fidelity tests  (PRESCALE_BY 5–6, OY up to 2160 — each >60 s)
// Run with: cargo test -- --ignored
// These validate output against the full-resolution test-suite configs and
// are intended for CI on PRs, not for routine development.
// ---------------------------------------------------------------------------

/// Color — full-resolution reference config (08cfg: PRESCALE_BY=5, OY=1080).
#[test]
#[ignore = "slow: ~3 min; run with: cargo test -- --ignored"]
fn cli_runs_color_preset_ref() {
    run_preset_test(
        "test-suite/08cfg.cfg",
        "test-suite/08.png",
        "color-ref",
        1000,
    );
}

/// Mono — full-resolution reference config (06cfg: PRESCALE_BY=5, OY=1080, HALATION_RADIUS=60).
#[test]
#[ignore = "slow: ~3 min; run with: cargo test -- --ignored"]
fn cli_runs_mono_preset_ref() {
    run_preset_test(
        "test-suite/06cfg.cfg",
        "test-suite/06.png",
        "mono-ref",
        1000,
    );
}

/// Amber — full-resolution reference config (09cfg: PRESCALE_BY=6, OY=2160).
#[test]
#[ignore = "slow: ~5 min; run with: cargo test -- --ignored"]
fn cli_runs_amber_preset_ref() {
    run_preset_test(
        "test-suite/09cfg.cfg",
        "test-suite/09.png",
        "amber-ref",
        1000,
    );
}
