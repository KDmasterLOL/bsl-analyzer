use std::{env, fs, process::Command};

use tempfile::TempDir;

#[test]
fn analyze_writes_sarif_report() {
    let temp = TempDir::new().expect("tempdir");
    let source_dir = temp.path().join("src");
    let output_dir = temp.path().join("reports");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&output_dir).expect("output dir");
    fs::write(source_dir.join("Module.bsl"), "Процедура Тест(\n").expect("fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["analyze", "-s"])
        .arg(&source_dir)
        .args(["-r", "sarif", "-o"])
        .arg(&output_dir)
        .arg("-q")
        .output()
        .expect("run bsl-analyzer");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_path = output_dir.join("bsl-analyzer.sarif");
    assert!(report_path.exists(), "SARIF report should be written");

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report_path).expect("report contents"))
            .expect("valid sarif json");
    assert_eq!(report["version"], "2.1.0");
    assert_eq!(report["runs"][0]["tool"]["driver"]["name"], "bsl-analyzer");
}
