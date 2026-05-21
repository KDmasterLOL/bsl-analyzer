//! End-to-end tests for the `format` CLI subcommand.

use std::{env, fs, process::Command};

use tempfile::TempDir;

fn bsl_analyzer() -> Command {
    Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
}

fn write_fixture(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, contents).expect("write fixture");
    path
}

#[test]
fn format_prints_to_stdout_by_default() {
    let temp = TempDir::new().expect("tempdir");
    let file = write_fixture(&temp, "Module.bsl", "Процедура Т()\nА=1;\nКонецПроцедуры");

    let output = bsl_analyzer().arg("format").arg(&file).output().expect("run");
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(stdout.contains("\tА = 1;"), "expected reformatted body, got: {stdout:?}");
    // File on disk must not be touched without --write.
    let on_disk = fs::read_to_string(&file).expect("read");
    assert_eq!(on_disk, "Процедура Т()\nА=1;\nКонецПроцедуры");
}

#[test]
fn format_write_updates_file_in_place() {
    let temp = TempDir::new().expect("tempdir");
    let file = write_fixture(&temp, "Module.bsl", "Процедура Т()\nА=1;\nКонецПроцедуры");

    let output = bsl_analyzer().arg("format").arg("-w").arg(&file).output().expect("run");
    assert!(output.status.success());
    let on_disk = fs::read_to_string(&file).expect("read");
    assert!(on_disk.contains("\tА = 1;"));
    // Stdout should NOT contain the formatted text in --write mode.
    assert!(output.stdout.is_empty(), "stdout should be silent with -w");
}

#[test]
fn format_check_exits_zero_on_already_formatted() {
    let temp = TempDir::new().expect("tempdir");
    let file = write_fixture(&temp, "Module.bsl", "Процедура Т()\n\tА = 1;\nКонецПроцедуры\n");

    let output = bsl_analyzer().arg("format").arg("--check").arg(&file).output().expect("run");
    assert!(
        output.status.success(),
        "expected exit 0 on formatted file; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "--check should not print to stdout");
    assert!(
        output.stderr.is_empty(),
        "--check should be silent on success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn format_check_exits_nonzero_on_unformatted() {
    let temp = TempDir::new().expect("tempdir");
    let file = write_fixture(&temp, "Module.bsl", "Процедура Т()\nА=1;\nКонецПроцедуры");

    let output = bsl_analyzer().arg("format").arg("--check").arg(&file).output().expect("run");
    assert!(!output.status.success(), "expected exit 1 on unformatted file");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("would reformat"), "stderr should mention the file, got: {stderr}");
    // File on disk untouched.
    assert_eq!(fs::read_to_string(&file).expect("read"), "Процедура Т()\nА=1;\nКонецПроцедуры");
}

#[test]
fn format_is_idempotent() {
    // fmt(fmt(x)) == fmt(x).
    let temp = TempDir::new().expect("tempdir");
    let file = write_fixture(
        &temp,
        "Module.bsl",
        "Процедура Т()\nЕсли А Тогда\nБ=1;\nИначе\nВ=2;\nКонецЕсли;\nКонецПроцедуры",
    );

    let first = bsl_analyzer().arg("format").arg(&file).output().expect("run");
    assert!(first.status.success());
    let first_text = String::from_utf8(first.stdout).expect("utf-8");

    // Write the result back, format again — should match exactly.
    fs::write(&file, &first_text).expect("write");
    let second = bsl_analyzer().arg("format").arg(&file).output().expect("run");
    assert!(second.status.success());
    let second_text = String::from_utf8(second.stdout).expect("utf-8");
    assert_eq!(first_text, second_text, "format is not idempotent");

    // And --check on the now-formatted file should pass.
    let check = bsl_analyzer().arg("format").arg("--check").arg(&file).output().expect("run");
    assert!(check.status.success(), "--check on formatted file should exit 0");
}

#[test]
fn format_preserves_parse_errors_invariant() {
    // Formatting an unparseable BSL file must not *introduce* new parse
    // errors. The contract is one-way: fmt may emit code that parses
    // exactly as well as the original.
    let temp = TempDir::new().expect("tempdir");
    let src = "Процедура Т(\nА=1;\nКонецПроцедуры"; // missing `)` — parser recovers
    let file = write_fixture(&temp, "Module.bsl", src);

    let output = bsl_analyzer().arg("format").arg(&file).output().expect("run");
    assert!(
        output.status.success(),
        "formatter must not crash on parse errors; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let formatted = String::from_utf8(output.stdout).expect("utf-8");

    // Compare error counts: formatted should not have *more* errors.
    let orig_errors = parser::parse(src).errors().len();
    let fmt_errors = parser::parse(&formatted).errors().len();
    assert!(
        fmt_errors <= orig_errors,
        "format introduced parse errors: original={orig_errors}, formatted={fmt_errors}"
    );
}
