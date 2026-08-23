use std::{env, fs, process::Command};

use tempfile::TempDir;

const WARNING: &str = "failed to deserialize project diagnostics config; using defaults";

fn analyze(project: &TempDir) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["analyze", "-s"])
        .arg(project.path())
        .arg("-q")
        .env("BSL_LOG", "warn")
        .output()
        .expect("run bsl-analyzer")
}

fn project() -> TempDir {
    let project = TempDir::new().expect("tempdir");
    fs::write(project.path().join("Module.bsl"), "Процедура Тест()\nКонецПроцедуры\n")
        .expect("fixture");
    project
}

#[test]
fn analyze_distinguishes_absent_and_invalid_diagnostics_config() {
    let absent = project();
    let output = analyze(&absent);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert_eq!(stderr.matches(WARNING).count(), 0, "stderr:\n{stderr}");

    let invalid = project();
    fs::write(
        invalid.path().join(".bsl-analyzer.json"),
        r#"{"diagnostics":{"ordinaryAppSupport":"invalid"}}"#,
    )
    .expect("config");
    let output = analyze(&invalid);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "stderr:\n{stderr}");
    assert_eq!(stderr.matches(WARNING).count(), 1, "stderr:\n{stderr}");
}
