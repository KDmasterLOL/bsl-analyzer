//! End-to-end broker mechanics: one backend serves many sessions, a second launch
//! defers to the live owner (bind-wins), and the backend dies with its owner session.
//!
//! Uses the lightweight `reference` profile so no heavy workspace build is needed,
//! and points the per-user runtime dir at a tempdir so the socket is isolated.

use std::sync::Arc;
use std::time::Duration;

#[cfg(any(unix, windows))]
use interprocess::local_socket::tokio::prelude::*;
#[cfg(any(unix, windows))]
use interprocess::local_socket::tokio::Stream as TokioStream;
#[cfg(any(unix, windows))]
use mcp_server::broker::{self, BackendKey};
use mcp_server::{serve_stream, McpProfile, McpServer, SharedState};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use tempfile::TempDir;

fn reference_server() -> McpServer {
    McpServer::new(McpProfile::Reference, SharedState::reference(None))
}

#[cfg(any(unix, windows))]
fn key_for(src: &TempDir) -> BackendKey {
    // Profile here only names the socket; the served profile is the passed server.
    BackendKey::new(src.path(), McpProfile::Workspace, 0)
}

#[cfg(any(unix, windows))]
async fn connect(key: &BackendKey) -> std::io::Result<TokioStream> {
    let name = broker::backend_name(key)?;
    TokioStream::connect(name).await
}

#[cfg(any(unix, windows))]
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
#[cfg(any(unix, windows))]
async fn backend_dies_with_its_owner_session_and_drops_the_rest() {
    // No `set_var` here: it races with other tests' `getenv` (a glibc env data race)
    // and would corrupt the resolved socket path. A unique source dir already gives a
    // unique socket name under the real runtime dir, so no isolation env is needed.
    let src = TempDir::new().unwrap();
    let key = key_for(&src);

    // Generous orphan grace so the backend never orphan-exits before clients connect.
    // Once an owner is claimed the grace is irrelevant — lifetime becomes owner-driven.
    let grace = Duration::from_secs(30);
    let backend =
        tokio::spawn(broker::daemon::run(|| Ok(reference_server()), key_for(&src), grace));

    // Two concurrent client sessions through the one backend. The first to complete its
    // initialize handshake (c1) sends bytes first and so becomes the owner; c2 is a
    // dependent session.
    let s1 = connect_within(&key, Duration::from_secs(25)).await;
    let c1 = ().serve(s1).await.expect("owner client initialized");
    let s2 = connect(&key).await.expect("second session connects");
    let c2 = ().serve(s2).await.expect("dependent client initialized");

    assert!(c1.peer_info().is_some(), "owner session saw server info");
    assert!(c2.peer_info().is_some(), "dependent session saw server info");
    let tools = c1.list_all_tools().await.expect("list tools");
    assert!(
        tools.iter().any(|t| t.name == "search"),
        "reference backend exposes its tools: {:?}",
        tools.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // Bind-wins: a second launch for the same key must defer to the live owner and
    // return promptly (the Ok(None) path), NOT block as a second owner. Its liveness
    // probe connects without sending data, so it must not be mistaken for the owner.
    let second =
        broker::daemon::run(|| Ok(reference_server()), key_for(&src), Duration::from_secs(60));
    tokio::time::timeout(Duration::from_secs(15), second)
        .await
        .expect("second launch returns promptly (defers to live owner)")
        .expect("second launch ok");

    // Close the OWNER session only: the backend must shut down, even though c2 is still
    // open. Dropping the listener and c2's socket then ends the backend task.
    c1.cancel().await.ok();
    let exited = tokio::time::timeout(Duration::from_secs(20), backend).await;
    assert!(exited.is_ok(), "backend shut down when its owner left");
    exited.unwrap().expect("backend task joined").expect("backend run ok");

    // The dependent session was dropped by the backend shutdown: a call on it must not
    // succeed (its socket was closed under it), proving the cascade reached c2. In
    // production the daemon process exits and the OS closes every FD at once, so the EOF
    // is immediate; here the runtime stays alive and only the explicit abort severs c2,
    // which under load may not be polled instantly — so a generous timeout stands in for
    // that delayed EOF. Either a transport error or no response means c2 is unusable; a
    // successful call would mean it is still being served, which must not happen.
    let mut args = serde_json::Map::new();
    args.insert("action".to_owned(), serde_json::Value::String("status".to_owned()));
    let call = c2.call_tool(CallToolRequestParams::new("search").with_arguments(args));
    let after = tokio::time::timeout(Duration::from_secs(10), call).await;
    assert!(
        !matches!(after, Ok(Ok(_))),
        "dependent session must be severed once the backend is gone, got: {after:?}"
    );
}

/// Regression: while the first backend is still doing its (slow) build, a second
/// launch for the same key must DEFER, not reclaim the socket — a bound-but-not-yet-
/// accepting backend must not look stale. And a client that connects mid-build must be
/// parked and served once the build completes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg(any(unix, windows))]
async fn second_launch_defers_while_first_is_still_building() {
    let src = TempDir::new().unwrap();
    let key = key_for(&src);

    // First backend: a deliberately slow build so the window "bound but not built" is
    // wide. The daemon must accept (and park) connections during this window.
    let slow_build = || {
        std::thread::sleep(Duration::from_secs(2));
        Ok(reference_server())
    };
    let first =
        tokio::spawn(broker::daemon::run(slow_build, key_for(&src), Duration::from_secs(15)));

    // A client connects during the build — the connect must succeed (backlog drained),
    // proving the socket is live (not stealable).
    let stream = connect_within(&key, Duration::from_secs(10)).await;

    // A second launch for the same key, while the first is still building, must defer
    // promptly. If it stole the socket it would become a second owner and block on its
    // own serve loop until idle (15s), tripping this timeout.
    let second =
        broker::daemon::run(|| Ok(reference_server()), key_for(&src), Duration::from_secs(15));
    tokio::time::timeout(Duration::from_secs(8), second)
        .await
        .expect("second launch defers while the first is building (no socket steal)")
        .expect("second launch ok");

    // The connection parked during the build is served once the build completes.
    let client = ().serve(stream).await.expect("parked session served after build");
    assert!(client.peer_info().is_some(), "parked session saw server info");
    client.cancel().await.ok();
    first.abort();
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
