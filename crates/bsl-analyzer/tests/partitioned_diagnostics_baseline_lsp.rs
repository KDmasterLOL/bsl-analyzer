mod common;

use common::*;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn partitioned_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for source in ["src/cf", "src/cfe/A", "src/cfe/B"] {
        std::fs::create_dir_all(dir.path().join(source)).unwrap();
        std::fs::write(dir.path().join(source).join("Configuration.xml"), "<Configuration/>")
            .unwrap();
    }
    for path in ["src/cf/Main.bsl", "src/cfe/A/A.bsl", "src/cfe/B/B.bsl"] {
        std::fs::write(dir.path().join(path), BROKEN).unwrap();
    }
    std::fs::write(
        dir.path().join("bsl-analyzer.toml"),
        r#"[source]
root = "src/cf"
extensions = [{ name = "A", path = "src/cfe/A" }, { name = "B", path = "src/cfe/B" }]

[diagnostics.baseline]
directory = "baselines"

[[diagnostics.baseline.groups]]
name = "vendor"
extensions = ["A"]
"#,
    )
    .unwrap();
    let created = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .current_dir(dir.path())
        .args(["diagnostics", "baseline", "create", "-s", "."])
        .output()
        .unwrap();
    assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
    dir
}

fn expect_publications(lsp: &Lsp, root: &Path, paths: &[&str], empty: bool) {
    let mut remaining: std::collections::BTreeSet<_> = paths
        .iter()
        .map(|path| lsp_types::Url::from_file_path(root.join(path)).unwrap().to_string())
        .collect();
    while !remaining.is_empty() {
        let published = lsp.wait_for(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"].as_str().is_some_and(|uri| remaining.contains(uri))
        });
        let uri = published["params"]["uri"].as_str().unwrap();
        assert_eq!(
            published["params"]["diagnostics"].as_array().unwrap().is_empty(),
            empty,
            "unexpected publication for {uri}: {published}"
        );
        remaining.remove(uri);
    }
}

#[test]
fn partitioned_baseline_lsp_main_extension_group_partial_and_recovery() {
    let dir = partitioned_project();
    let root = dir.path();
    let mut lsp = Lsp::start(root);
    let paths = ["src/cf/Main.bsl", "src/cfe/A/A.bsl", "src/cfe/B/B.bsl"];
    for path in paths {
        let published = lsp.open(&root.join(path), BROKEN);
        assert!(
            published["params"]["diagnostics"].as_array().unwrap().is_empty(),
            "owner baseline must suppress {path}: {published}"
        );
    }

    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("baselines/manifest.json")).unwrap())
            .unwrap();
    let object = manifest["partitions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["partition_id"] == "group:vendor")
        .unwrap()["file"]
        .as_str()
        .unwrap();
    let object_relative = object.to_owned();
    let object = root.join("baselines").join(&object_relative);
    let valid = std::fs::read(&object).unwrap();
    std::fs::write(&object, b"{broken").unwrap();
    lsp.wait_for(|message| message["method"] == "window/showMessage");
    expect_publications(&lsp, root, &paths, false);

    let directory =
        project_model::ManagedBaselineDirectory::open(root, "baselines", false).unwrap();
    directory.create_file_new("replacement.tmp").unwrap().write_all(&valid).unwrap();
    directory.replace_file("replacement.tmp", &object_relative).unwrap();
    expect_publications(&lsp, root, &paths, true);
}
