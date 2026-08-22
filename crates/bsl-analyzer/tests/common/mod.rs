//! Shared LSP harness for the diagnostics-baseline tests.
//!
//! Lives under `tests/common/` so it is a module, not a test target: included from a
//! sibling test file, every `#[test]` in it would be compiled and RUN once per
//! including target.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

pub const BROKEN: &str = "Процедура Тест(\n";

pub fn project() -> tempfile::TempDir {
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

pub struct Lsp {
    pub child: Child,
    pub stdin: ChildStdin,
    pub messages: Receiver<Value>,
}

impl Lsp {
    pub fn start(root: &Path) -> Self {
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

    pub fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn wait_for(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        loop {
            let message = self.messages.recv_timeout(Duration::from_secs(60)).unwrap();
            if predicate(&message) {
                return message;
            }
        }
    }

    pub fn open(&mut self, path: &Path, text: &str) -> Value {
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
