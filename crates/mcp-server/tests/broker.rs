//! End-to-end broker mechanics: one backend serves many sessions, a second launch
//! defers to the live owner (bind-wins), and the backend exits after idle.
//!
//! Uses the lightweight `reference` profile so no heavy workspace build is needed,
//! and points the per-user runtime dir at a tempdir so the socket is isolated.

use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Stream as TokioStream;
use mcp_server::broker::{self, BackendKey};
use mcp_server::{serve_stream, McpProfile, McpServer, SharedState};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use tempfile::TempDir;

fn reference_server() -> McpServer {
    McpServer::new(McpProfile::Reference, SharedState::reference(None))
}

fn key_for(src: &TempDir) -> BackendKey {
    // Profile here only names the socket; the served profile is the passed server.
    BackendKey::new(src.path(), McpProfile::Workspace, 0)
}

async fn connect(key: &BackendKey) -> std::io::Result<TokioStream> {
    let name = broker::backend_name(key)?;
    TokioStream::connect(name).await
}

async fn connect_within(key: &BackendKey, budget: Duration) -> TokioStream {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if let Ok(s) = connect(key).await {
            return s;
        }
        assert!(tokio::time::Instant::now() < deadline, "backend never became reachable");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_backend_serves_many_sessions_then_idles_out() {
    // No `set_var` here: it races with other tests' `getenv` (a glibc env data race)
    // and would corrupt the resolved socket path. A unique source dir already gives a
    // unique socket name under the real runtime dir, so no isolation env is needed.
    let src = TempDir::new().unwrap();
    let key = key_for(&src);

    // Budgets are generous because this binary runs alongside the heavy workspace
    // concurrency test, so binding/connecting can lag under CPU contention. The idle
    // window must exceed connection latency, or the backend would idle-exit before the
    // clients connect.
    let idle = Duration::from_secs(8);
    let owner = tokio::spawn(broker::daemon::run(|| Ok(reference_server()), key_for(&src), idle));

    // Two concurrent client sessions, each doing a full MCP initialize handshake
    // through the one backend.
    let s1 = connect_within(&key, Duration::from_secs(25)).await;
    let s2 = connect(&key).await.expect("second session connects");
    let c1 = ().serve(s1).await.expect("client 1 initialized");
    let c2 = ().serve(s2).await.expect("client 2 initialized");

    assert!(c1.peer_info().is_some(), "session 1 saw server info");
    assert!(c2.peer_info().is_some(), "session 2 saw server info");
    let tools = c1.list_all_tools().await.expect("list tools");
    assert!(
        tools.iter().any(|t| t.name == "search"),
        "reference backend exposes its tools: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // Bind-wins: a second launch for the same key must defer to the live owner and
    // return promptly (the Ok(None) path), NOT block as a second owner for its idle
    // window.
    let second =
        broker::daemon::run(|| Ok(reference_server()), key_for(&src), Duration::from_secs(60));
    tokio::time::timeout(Duration::from_secs(15), second)
        .await
        .expect("second launch returns promptly (defers to live owner)")
        .expect("second launch ok");

    // Close both sessions; the now-idle owner must exit within a couple idle windows.
    c1.cancel().await.ok();
    c2.cancel().await.ok();
    let exited = tokio::time::timeout(Duration::from_secs(40), owner).await;
    assert!(exited.is_ok(), "backend exited after going idle");
    exited.unwrap().expect("owner task joined").expect("owner run ok");
}

/// M3 concurrency: many sessions sharing one workspace backend must serve in
/// parallel without deadlocking. Each session is an in-memory duplex pair fed to
/// `serve_stream` from one cloned `McpServer` — exactly how the daemon serves N
/// proxies from one `SharedState`. Every session fires a burst of calls that hit the
/// lazy loaders and shared mutexes (graph `ensure_loading`, diagnostics, search
/// status); the assertion is simply that none of them hang.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn workspace_backend_serves_concurrent_sessions_without_deadlock() {
    const SESSIONS: usize = 6;
    const ROUNDS: usize = 4;
    const ACTIONS: [(&str, &str); 5] = [
        ("graph", "schema"),
        ("graph", "status"),
        ("diagnostics", "catalog"),
        ("diagnostics", "schema"),
        ("search", "status"),
    ];

    let ws = TempDir::new().unwrap();
    let server =
        McpServer::new(McpProfile::Workspace, SharedState::workspace(ws.path().to_path_buf()));

    let mut clients = Vec::new();
    for _ in 0..SESSIONS {
        // The buffer must exceed the largest single response (the diagnostics catalog
        // is the biggest): an in-process duplex, unlike an OS socket, has no kernel
        // backpressure draining both directions independently, so a response larger
        // than the buffer would wedge the in-memory pipe — an artifact of this harness,
        // not of the socket transport the daemon actually uses.
        let (client_io, server_io) = tokio::io::duplex(4 * 1024 * 1024);
        tokio::spawn(serve_stream(server.clone(), server_io));
        clients.push(Arc::new(().serve(client_io).await.expect("session initialized")));
    }

    let mut handles = Vec::new();
    for (si, client) in clients.iter().enumerate() {
        for round in 0..ROUNDS {
            for (tool, action) in ACTIONS {
                let client = Arc::clone(client);
                let mut arguments = serde_json::Map::new();
                arguments.insert("action".to_owned(), serde_json::Value::String(action.to_owned()));
                let label = format!("session{si}/round{round}/{tool}:{action}");
                handles.push(tokio::spawn(async move {
                    let call = client
                        .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments));
                    match tokio::time::timeout(Duration::from_secs(20), call).await {
                        Ok(Ok(_)) => (label, "ok"),
                        Ok(Err(_)) => (label, "transport-err"),
                        Err(_) => (label, "HUNG"),
                    }
                }));
            }
        }
    }

    // A deadlock or lock-ordering hang surfaces as a per-call timeout. `is_error`
    // (e.g. "still indexing") is a valid response and not asserted against.
    let mut bad = Vec::new();
    for handle in handles {
        let (label, status) = handle.await.expect("session task did not panic");
        if status != "ok" {
            bad.push(format!("{label} => {status}"));
        }
    }
    assert!(
        bad.is_empty(),
        "calls did not complete cleanly under concurrency:\n{}",
        bad.join("\n")
    );
}
