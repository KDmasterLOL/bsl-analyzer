//! End-to-end `ignored_authors` filtering in `analyze`: findings on lines
//! last touched by an ignored git author must not reach the report, while
//! the user's own, uncommitted and untracked code always does.

use std::{fs, path::Path, process::Command};

use tempfile::TempDir;

/// A module that reliably produces at least one diagnostic (unclosed procedure).
const BROKEN: &str = "Процедура Тест(\n";

const VENDOR_NAME: &str = "Фирма Тест";
const VENDOR_EMAIL: &str = "vendor@example.com";
const DEV_NAME: &str = "Разработчик";
const DEV_EMAIL: &str = "dev@example.com";

fn run_git_as(dir: &Path, name: &str, email: &str, args: &[&str]) {
    let mut cmd = Command::new("git");
    // Inherited GIT_* variables (e.g. GIT_AUTHOR_NAME when the test suite
    // itself runs inside a `git commit` pre-commit hook) override the `-c`
    // identity below and would silently reattribute the fixture commits.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(&key);
        }
    }
    let output = cmd
        .arg("-C")
        .arg(dir)
        .args(["-c", &format!("user.name={name}"), "-c", &format!("user.email={email}")])
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `Vendor.bsl` committed by the vendor author, `Own.bsl` by the developer.
fn setup_repo() -> TempDir {
    let temp = TempDir::new().expect("tempdir");
    let root = temp.path();

    fs::write(root.join("Vendor.bsl"), BROKEN).expect("vendor file");
    run_git_as(root, VENDOR_NAME, VENDOR_EMAIL, &["init", "-q"]);
    run_git_as(root, VENDOR_NAME, VENDOR_EMAIL, &["add", "."]);
    run_git_as(root, VENDOR_NAME, VENDOR_EMAIL, &["commit", "-q", "-m", "vendor"]);

    fs::write(root.join("Own.bsl"), BROKEN).expect("own file");
    run_git_as(root, DEV_NAME, DEV_EMAIL, &["add", "."]);
    run_git_as(root, DEV_NAME, DEV_EMAIL, &["commit", "-q", "-m", "own module"]);

    temp
}

fn run_analyze(root: &Path, extra_args: &[&str]) -> (bool, String, String) {
    let output_dir = root.join("reports");
    fs::create_dir_all(&output_dir).expect("output dir");

    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["analyze", "-s"])
        .arg(root)
        .args(["-r", "sarif", "-o"])
        .arg(&output_dir)
        .arg("-q")
        .args(extra_args)
        .output()
        .expect("run bsl-analyzer");

    let sarif = fs::read_to_string(output_dir.join("bsl-analyzer.sarif")).unwrap_or_default();
    (output.status.success(), sarif, String::from_utf8_lossy(&output.stderr).into_owned())
}

fn reported_files(sarif: &str) -> (bool, bool) {
    (sarif.contains("Vendor.bsl"), sarif.contains("Own.bsl"))
}

#[test]
fn ignored_author_findings_are_dropped_and_own_findings_kept() {
    let temp = setup_repo();

    // Without the filter both broken modules are reported.
    let (ok, sarif, _) = run_analyze(temp.path(), &[]);
    assert!(ok);
    let (vendor, own) = reported_files(&sarif);
    assert!(vendor && own, "unfiltered run must report both files:\n{sarif}");

    let (ok, sarif, _) = run_analyze(temp.path(), &["--ignored-author", VENDOR_NAME]);
    assert!(ok);
    let (vendor, own) = reported_files(&sarif);
    assert!(!vendor, "vendor-authored findings must be dropped:\n{sarif}");
    assert!(own, "developer-authored findings must stay:\n{sarif}");
}

#[test]
fn matching_by_email_and_config_without_cli_flags() {
    let temp = setup_repo();
    fs::write(
        temp.path().join("bsl-analyzer.toml"),
        format!("[analysis]\nignored_authors = [\"{VENDOR_EMAIL}\"]\n"),
    )
    .expect("config");

    let (ok, sarif, _) = run_analyze(temp.path(), &[]);
    assert!(ok);
    let (vendor, own) = reported_files(&sarif);
    assert!(!vendor && own, "[analysis].ignored_authors must filter by email:\n{sarif}");
}

#[test]
fn cli_flag_replaces_the_config_list() {
    let temp = setup_repo();
    // The config ignores the developer; the CLI flag must replace (not merge
    // with) that list, so only vendor findings disappear.
    fs::write(
        temp.path().join("bsl-analyzer.toml"),
        format!("[analysis]\nignored_authors = [\"{DEV_NAME}\"]\n"),
    )
    .expect("config");

    let (ok, sarif, _) = run_analyze(temp.path(), &["--ignored-author", VENDOR_NAME]);
    assert!(ok);
    let (vendor, own) = reported_files(&sarif);
    assert!(!vendor, "CLI-listed vendor author must be filtered:\n{sarif}");
    assert!(own, "config-listed author must be overridden by the CLI list:\n{sarif}");
}

#[test]
fn untracked_and_uncommitted_code_is_never_filtered() {
    let temp = setup_repo();

    // Untracked broken module plus an uncommitted broken addition to the
    // vendor file: both are the user's own work regardless of blame history.
    fs::write(temp.path().join("New.bsl"), BROKEN).expect("untracked file");
    fs::write(temp.path().join("Vendor.bsl"), format!("{BROKEN}КонецПроцедуры\nПроцедура Ещё(\n"))
        .expect("uncommitted vendor edit");

    let (ok, sarif, _) = run_analyze(temp.path(), &["--ignored-author", VENDOR_NAME]);
    assert!(ok);
    assert!(sarif.contains("New.bsl"), "untracked file must not be filtered:\n{sarif}");
    assert!(
        sarif.contains("Vendor.bsl"),
        "findings on uncommitted lines must survive even in a vendor file:\n{sarif}"
    );
}

#[test]
fn shallow_clone_is_a_hard_error() {
    let temp = setup_repo();

    // A file:// clone honours --depth and produces a real shallow repository.
    let clones = TempDir::new().expect("clone dir");
    let clone_dir = clones.path().join("shallow");
    run_git_as(
        temp.path(),
        DEV_NAME,
        DEV_EMAIL,
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &format!("file://{}", temp.path().display()),
            clone_dir.to_str().expect("utf-8 path"),
        ],
    );

    let (ok, _, stderr) = run_analyze(&clone_dir, &["--ignored-author", VENDOR_NAME]);
    assert!(!ok, "author filtering on a shallow clone must fail the run");
    assert!(stderr.contains("unshallow"), "the error must point at the fix:\n{stderr}");
}

#[test]
fn author_filter_composes_with_git_diff_scope() {
    let temp = setup_repo();
    run_git_as(temp.path(), DEV_NAME, DEV_EMAIL, &["branch", "base"]);

    // Own.bsl gains a broken addition after the base branch point; Vendor.bsl
    // is untouched, so the scope alone already excludes it.
    fs::write(temp.path().join("Own.bsl"), format!("{BROKEN}КонецПроцедуры\nПроцедура Ещё(\n"))
        .expect("own edit");
    run_git_as(temp.path(), DEV_NAME, DEV_EMAIL, &["commit", "-aqm", "own change"]);

    let (ok, sarif, _) = run_analyze(
        temp.path(),
        &["--incremental", "--git-diff", "base", "--ignored-author", VENDOR_NAME],
    );
    assert!(ok);
    let (vendor, own) = reported_files(&sarif);
    assert!(!vendor, "out-of-scope vendor file must stay excluded:\n{sarif}");
    assert!(own, "in-scope developer change must be reported:\n{sarif}");
}

#[test]
fn baseline_classification_precedes_author_presentation_filter() {
    let temp = setup_repo();
    fs::write(
        temp.path().join("bsl-analyzer.toml"),
        "[diagnostics.baseline]\npath = \"baseline.json\"\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("baseline.json"),
        r#"{
  "schema_version": 1,
  "scope": { "source_root": "", "extensions": [] },
  "diagnostics": []
}
"#,
    )
    .unwrap();

    let run = |extra: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
            .args(["analyze", "-s"])
            .arg(temp.path())
            .args(["--format", "jsonl"])
            .args(extra)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>()
    };
    let unfiltered = run(&[]);
    let filtered = run(&["--ignored-author", VENDOR_NAME]);
    let unfiltered_done = unfiltered.iter().find(|event| event["type"] == "done").unwrap();
    let filtered_done = filtered.iter().find(|event| event["type"] == "done").unwrap();
    let findings = |events: &[serde_json::Value]| {
        events
            .iter()
            .filter(|event| event["type"] == "file")
            .map(|event| event["diagnostics"].as_array().unwrap().len())
            .sum::<usize>()
    };

    assert_eq!(unfiltered_done["baseline"]["new"], filtered_done["baseline"]["new"]);
    assert_eq!(filtered_done["baseline"]["state"], "partial");
    assert!(findings(&filtered) < findings(&unfiltered));
}
