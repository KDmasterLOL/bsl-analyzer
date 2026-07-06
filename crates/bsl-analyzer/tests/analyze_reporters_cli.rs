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

/// Two analyze runs over the same tree must produce byte-identical SARIF: A/B
/// regression gates compare reports with plain `cmp`. The fixture provokes the
/// historical divergence source — several `DuplicateStringLiteral` groups per
/// file, whose emit order followed hash-map iteration and differed between
/// processes.
#[test]
fn analyze_sarif_is_byte_identical_across_runs() {
    let temp = TempDir::new().expect("tempdir");
    let source_dir = temp.path().join("src");
    fs::create_dir_all(&source_dir).expect("source dir");

    for file in ["ModuleA.bsl", "ModuleB.bsl"] {
        let mut text = String::from("Процедура Тест()\n");
        for group in 0..6 {
            for _ in 0..3 {
                text.push_str(&format!("    Значение = \"Литерал номер {group}\";\n"));
            }
        }
        text.push_str("КонецПроцедуры\n");
        fs::write(source_dir.join(file), &text).expect("fixture");
    }

    let run = |output_dir: &std::path::Path| {
        fs::create_dir_all(output_dir).expect("output dir");
        let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
            .args(["analyze", "-s"])
            .arg(&source_dir)
            .args(["-r", "sarif", "-o"])
            .arg(output_dir)
            .arg("-q")
            .output()
            .expect("run bsl-analyzer");
        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        fs::read(output_dir.join("bsl-analyzer.sarif")).expect("sarif bytes")
    };

    let first = run(&temp.path().join("reports-a"));
    let second = run(&temp.path().join("reports-b"));

    let findings: serde_json::Value = serde_json::from_slice(&first).expect("valid sarif json");
    let results = findings["runs"][0]["results"].as_array().expect("results array");
    assert!(!results.is_empty(), "fixture must actually produce diagnostics");

    assert_eq!(first, second, "SARIF must be byte-identical across runs");
}

#[test]
fn analyze_writes_codequality_report() {
    let temp = TempDir::new().expect("tempdir");
    let source_dir = temp.path().join("src");
    let output_dir = temp.path().join("reports");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&output_dir).expect("output dir");
    fs::write(source_dir.join("Module.bsl"), "Процедура Тест(\n").expect("fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["analyze", "-s"])
        .arg(&source_dir)
        .args(["-r", "codequality", "-o"])
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

    let report_path = output_dir.join("gl-code-quality-report.json");
    assert!(report_path.exists(), "code quality report should be written");

    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report_path).expect("report contents"))
            .expect("valid codequality json");
    let entries = report.as_array().expect("codequality is an array");
    assert!(!entries.is_empty(), "fixture must produce diagnostics");
    let entry = &entries[0];
    assert!(entry["fingerprint"].is_string());
    assert!(entry["check_name"].is_string());
    assert!(entry["location"]["path"].is_string());
    assert!(entry["location"]["lines"]["begin"].is_u64());
}

#[test]
fn analyze_writes_junit_report() {
    let temp = TempDir::new().expect("tempdir");
    let source_dir = temp.path().join("src");
    let output_dir = temp.path().join("reports");
    fs::create_dir_all(&source_dir).expect("source dir");
    fs::create_dir_all(&output_dir).expect("output dir");
    fs::write(source_dir.join("Module.bsl"), "Процедура Тест(\n").expect("fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["analyze", "-s"])
        .arg(&source_dir)
        .args(["-r", "junit", "-o"])
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

    let report_path = output_dir.join("bsl-analyzer.junit.xml");
    assert!(report_path.exists(), "junit report should be written");

    let xml = fs::read_to_string(report_path).expect("report contents");
    assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
    assert!(xml.contains("<testsuites"));
    assert!(xml.contains("</testsuites>"));
    assert!(xml.contains("<testcase"));
    // Every opened suite is closed → the document is well-formed enough for CI.
    assert_eq!(xml.matches("<testsuite ").count(), xml.matches("</testsuite>").count());
}
