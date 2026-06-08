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
// One full-pipeline smoke test per preset family
// ---------------------------------------------------------------------------

/// Color family — RGB shadowmask + scanlines.
#[test]
fn cli_runs_color_preset() {
    run_preset_test("test-suite/08cfg.cfg", "test-suite/08.png", "color", 1000);
}

/// Mono family — paperwhite tint, no shadowmask.
#[test]
fn cli_runs_mono_preset() {
    run_preset_test("test-suite/06cfg.cfg", "test-suite/06.png", "mono", 1000);
}

/// Mono-amber family — amber tint, scanlines.
#[test]
fn cli_runs_amber_preset() {
    run_preset_test("test-suite/09cfg.cfg", "test-suite/09.png", "amber", 1000);
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
