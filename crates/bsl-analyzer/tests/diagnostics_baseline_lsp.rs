use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use ide::diagnostics_baseline::{
    diagnostic_fingerprint, DiagnosticsBaseline, DiagnosticsBaselineEntry, DiagnosticsBaselineRange,
};
use serde_json::{json, Value};

const BROKEN: &str = "Процедура Тест(\n";

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/Main.bsl"), BROKEN).unwrap();
    std::fs::write(
        dir.path().join("bsl-analyzer.toml"),
        "[source]\nroot = \"src\"\n\n[diagnostics.baseline]\npath = \"baseline.json\"\n",
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

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<Value>,
}

impl Lsp {
    fn start(root: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
            .arg("lsp")
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).ok().filter(|&n| n > 0).is_none() {
                        return;
                    }
                    if header == "\r\n" {
                        break;
                    }
                    if let Some(value) = header.strip_prefix("Content-Length:") {
                        length = value.trim().parse::<usize>().ok();
                    }
                }
                let Some(length) = length else { return };
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                if let Ok(message) = serde_json::from_slice(&body) {
                    if tx.send(message).is_err() {
                        return;
                    }
                }
            }
        });
        let mut lsp = Self { child, stdin, messages };
        let root_uri = lsp_types::Url::from_directory_path(root).unwrap();
        lsp.send(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"rootUri": root_uri, "capabilities": {}}
        }));
        lsp.wait_for(|message| message["id"] == 1);
        lsp.send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        lsp
    }

    fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn wait_for(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self.messages.recv_timeout(Duration::from_secs(60)).unwrap();
            if predicate(&message) {
                return message;
            }
        }
    }

    fn open(&mut self, path: &Path, text: &str) -> Value {
        let uri = lsp_types::Url::from_file_path(path).unwrap();
        self.send(json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": {"textDocument": {"uri": uri, "languageId": "bsl", "version": 1, "text": text}}
        }));
        self.wait_for(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri.as_str()
        })
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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
    assert!(published["params"]["diagnostics"].as_array().unwrap().is_empty());
    assert!(
        !published.to_string().contains("resolved outside the open document"),
        "a document publication must not synthesize a global resolved diagnostic"
    );
}
