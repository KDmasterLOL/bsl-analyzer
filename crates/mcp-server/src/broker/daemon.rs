//! The shared backend.
//!
//! Binds the per-project rendezvous *before* building the heavy [`SharedState`], so
//! a process that loses the launch race exits without ever starting a competing
//! build against the same per-project databases. The winner builds once and serves
//! every connecting proxy from it, then exits after an idle window with no
//! connections, releasing the analysis state.
//!
//! [`SharedState`]: crate::SharedState

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Listener as TokioListener;
use interprocess::local_socket::tokio::Stream as TokioStream;
use interprocess::local_socket::ListenerOptions;
use tokio::time::{interval, MissedTickBehavior};

use crate::broker::name::{backend_name, BackendKey};
use crate::{serve_stream, McpServer};

/// Run the backend for `key`. `build` is invoked only after this process wins the
/// bind, so the expensive state construction (which spawns background builds
/// touching the project DBs) never runs in a race loser.
///
/// Returns `Ok(())` both when this process served as the backend and exited on
/// idle, and when another live backend already owned the name (nothing to do).
pub async fn run<F>(build: F, key: BackendKey, idle: Duration) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<McpServer>,
{
    let Some(listener) = bind(&key).await? else {
        tracing::info!("backend already serving this project; nothing to do");
        return Ok(());
    };

    let server = build()?;
    let guard = server.clone();
    // Flush/persist resident state on the way out (success or failure), mirroring
    // the stdio path, before the process exits.
    let result = serve(server, listener, idle).await;
    guard.shutdown();
    result
}

async fn serve(server: McpServer, listener: TokioListener, idle: Duration) -> anyhow::Result<()> {
    tracing::info!(
        pid = std::process::id(),
        idle_secs = idle.as_secs(),
        "broker backend listening"
    );

    // `active` counts in-flight sessions; `total` counts every accepted session.
    // Idle = no active session AND no new session across one full `idle` window.
    let active = Arc::new(AtomicU64::new(0));
    let total = Arc::new(AtomicU64::new(0));
    let mut last_total = 0u64;

    let mut ticker = interval(idle);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let conn = accepted?;
                if !peer_authorized(&conn) {
                    tracing::warn!("rejected backend connection from an unauthorized peer");
                    continue;
                }
                let served = total.fetch_add(1, Ordering::SeqCst) + 1;
                // The guard decrements `active` on drop, so a panicking session task
                // can never strand the count and keep the backend alive forever.
                let guard = ActiveGuard::new(Arc::clone(&active));
                tracing::debug!(
                    active = active.load(Ordering::SeqCst),
                    served,
                    "broker accepted a session"
                );
                let session = server.clone();
                tokio::spawn(async move {
                    let _guard = guard;
                    if let Err(e) = serve_stream(session, conn).await {
                        tracing::warn!(error = %e, "broker session ended with error");
                    }
                });
            }
            _ = ticker.tick() => {
                let seen = total.load(Ordering::SeqCst);
                if active.load(Ordering::SeqCst) == 0 && seen == last_total {
                    tracing::info!("idle with no connections; shutting down backend");
                    break;
                }
                last_total = seen;
            }
        }
    }

    Ok(())
}

/// Decrements the active-session count on drop (including unwind), so the idle
/// accounting stays correct even if a session task panics.
struct ActiveGuard(Arc<AtomicU64>);

impl ActiveGuard {
    fn new(count: Arc<AtomicU64>) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Bind the rendezvous, winner-takes-all.
///
/// - `Ok(Some(listener))` — we own the name and should serve.
/// - `Ok(None)` — another live backend already owns it; defer to it.
/// - `Err(..)` — a real failure.
///
/// On `AddrInUse` we probe with a connect: a successful connect means a live owner
/// (defer); a refused connect on unix means a stale socket file from a crashed
/// backend, which we reclaim and rebind once — and if a concurrent cold-starter
/// beats us to that rebind, we defer to it rather than erroring.
async fn bind(key: &BackendKey) -> anyhow::Result<Option<TokioListener>> {
    match ListenerOptions::new().name(backend_name(key)?).create_tokio() {
        Ok(listener) => Ok(Some(listener)),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if probe_live(key).await? {
                return Ok(None);
            }
            #[cfg(unix)]
            {
                let path = key.socket_path()?;
                tracing::info!(path = %path.display(), "reclaiming stale backend socket");
                let _ = std::fs::remove_file(&path);
                match ListenerOptions::new().name(backend_name(key)?).create_tokio() {
                    Ok(listener) => Ok(Some(listener)),
                    Err(e2) if e2.kind() == std::io::ErrorKind::AddrInUse => {
                        // A concurrent cold-starter rebound first; defer to it.
                        if probe_live(key).await? {
                            Ok(None)
                        } else {
                            Err(e2.into())
                        }
                    }
                    Err(e2) => Err(e2.into()),
                }
            }
            #[cfg(not(unix))]
            {
                Err(e.into())
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Is a backend actually accepting on this name? A successful connect proves a live
/// listener (queued in its backlog even mid-build); a refused/not-found connect
/// means the name is stale.
async fn probe_live(key: &BackendKey) -> anyhow::Result<bool> {
    Ok(TokioStream::connect(backend_name(key)?).await.is_ok())
}

/// Reject a peer running as a different user. On unix this reads `SO_PEERCRED`; the
/// runtime dir is already 0700/owned-by-us, so this is defense in depth (and covers
/// the abstract-namespace case where there is no socket file to permission). When
/// the platform cannot report a euid we allow and rely on the directory/pipe ACL.
#[cfg(unix)]
fn peer_authorized(conn: &TokioStream) -> bool {
    match conn.peer_creds() {
        Ok(creds) => {
            creds.euid().map(|uid| uid == crate::broker::name::current_euid()).unwrap_or(true)
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not read peer credentials; relying on dir/pipe ACL");
            true
        }
    }
}

/// On Windows the named pipe's default ACL restricts it to the creating user's
/// session, so connection-time uid checking is not applicable here.
#[cfg(not(unix))]
fn peer_authorized(_conn: &TokioStream) -> bool {
    true
}
