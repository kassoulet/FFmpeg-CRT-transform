use std::process::Command;

#[test]
fn cli_rejects_missing_file() {
    let out = Command::new(env!("CARGO_BIN_EXE_crt-transform"))
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
    let out = Command::new(env!("CARGO_BIN_EXE_crt-transform"))
        .arg("test-suite/01cfg.cfg")
        .arg("test-suite/01.mp4")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Phase B"));
}

#[test]
fn cli_accepts_png_input() {
    let tmp = std::env::temp_dir().join("crt-transform-test-08-output.png");
    let _ = std::fs::remove_file(&tmp);

    let out = Command::new(env!("CARGO_BIN_EXE_crt-transform"))
        .arg("test-suite/08cfg.cfg")
        .arg("test-suite/08.png")
        .arg(&tmp)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp.exists(), "output file was not created");

    let meta = std::fs::metadata(&tmp).unwrap();
    assert!(meta.len() > 1000, "output too small");

    let _ = std::fs::remove_file(&tmp);
}
