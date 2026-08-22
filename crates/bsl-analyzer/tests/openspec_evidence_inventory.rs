use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const CHANGE: &str = "add-selective-diagnostics-baseline-partitions";

fn change_directory(repository: &Path) -> PathBuf {
    let active = repository.join("openspec/changes").join(CHANGE);
    if active.is_dir() {
        return active;
    }
    std::fs::read_dir(repository.join("openspec/changes/archive"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(CHANGE))
        })
        .unwrap_or_else(|| panic!("OpenSpec change {CHANGE} is neither active nor archived"))
}

fn sources(root: &Path, extension: &str) -> String {
    fn visit(path: &Path, extension: &str, output: &mut String) {
        for entry in std::fs::read_dir(path).unwrap().filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, extension, output);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                output.push_str(&std::fs::read_to_string(path).unwrap());
            }
        }
    }
    let mut output = String::new();
    visit(root, extension, &mut output);
    output
}

#[test]
fn selective_diagnostics_baseline_evidence_is_complete() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let change = change_directory(&repository);
    let spec = std::fs::read_to_string(
        change.join("specs/selective-diagnostics-baseline-partitions/spec.md"),
    )
    .unwrap();
    let evidence: BTreeSet<_> = spec
        .lines()
        .filter(|line| line.contains("AUTOMATED EVIDENCE"))
        .map(|line| {
            line.split('`')
                .nth(1)
                .unwrap_or_else(|| panic!("evidence line has no code span: {line}"))
        })
        .collect();
    assert!(!evidence.is_empty());

    let traceability = std::fs::read_to_string(change.join("traceability.md")).unwrap();
    let rust_sources = sources(&repository.join("crates"), "rs");
    let ci_sources = sources(&repository.join(".github/workflows"), "yml");
    for name in &evidence {
        assert!(
            traceability.contains(&format!("| `{name}` |")),
            "{name} has no exact traceability mapping"
        );
        assert!(
            rust_sources.contains(&format!("fn {name}")) || ci_sources.contains(name),
            "{name} has no Rust test or CI route"
        );
    }
    let mapped: BTreeSet<_> = traceability
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split("` |").next())
        .collect();
    assert_eq!(mapped, evidence, "traceability evidence index drifted from delta spec");
}
