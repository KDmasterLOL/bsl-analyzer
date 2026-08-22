mod common;

use ide::diagnostics_baseline::{
    diagnostic_fingerprint, DiagnosticsBaseline, DiagnosticsBaselineEntry, DiagnosticsBaselineRange,
};

use std::time::Duration;

use common::*;

#[test]
fn parity() {
    let dir = project();
    let baseline: DiagnosticsBaseline =
        serde_json::from_slice(&std::fs::read(dir.path().join("baseline.json")).unwrap()).unwrap();
    assert!(!baseline.diagnostics.is_empty(), "CLI must create at least one known diagnostic");

    let mut lsp = Lsp::start(dir.path());
    let published = lsp.open(&dir.path().join("src/Main.bsl"), BROKEN);
    assert!(
        published["params"]["diagnostics"].as_array().unwrap().is_empty(),
        "LSP must suppress the same diagnostics the CLI recorded: {published}"
    );
}

#[test]
fn partial_document() {
    let dir = project();
    let baseline_path = dir.path().join("baseline.json");
    let mut baseline: DiagnosticsBaseline =
        serde_json::from_slice(&std::fs::read(&baseline_path).unwrap()).unwrap();
    baseline.diagnostics.push(DiagnosticsBaselineEntry {
        fingerprint: diagnostic_fingerprint("src/Other.bsl", "UnreachableCode", "Возврат;", 0),
        path: "src/Other.bsl".to_owned(),
        code: "UnreachableCode".to_owned(),
        snippet: "Возврат;".to_owned(),
        occurrence: 0,
        message: "resolved outside the open document".to_owned(),
        severity: "warning".to_owned(),
        range: DiagnosticsBaselineRange {
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 8,
        },
    });
    std::fs::write(
        &baseline_path,
        ide::diagnostics_baseline::diagnostics_baseline_json(&baseline).unwrap(),
    )
    .unwrap();

    let mut lsp = Lsp::start(dir.path());
    let published = lsp.open(&dir.path().join("src/Main.bsl"), BROKEN);
    assert!(published["params"]["uri"].as_str().unwrap().ends_with("Main.bsl"));
    assert!(published["params"]["diagnostics"].as_array().unwrap().is_empty());

    // The resolved entry belongs to another file, so no publication may carry it —
    // neither this one nor any that follows. Asserting only on the (already empty)
    // publication above could not tell the two apart.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        match lsp.messages.recv_timeout(Duration::from_millis(50)) {
            Ok(message) => assert!(
                !message.to_string().contains("resolved outside the open document"),
                "a resolved entry of another file must not be synthesized: {message}"
            ),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}
