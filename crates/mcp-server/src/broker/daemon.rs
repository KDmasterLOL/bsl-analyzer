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
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::broker::name::{backend_name, BackendKey};
use crate::{serve_stream, McpServer};

/// Cap on connections held while the resident state builds. The listener keeps draining
/// past this (so a concurrent liveness probe still succeeds), but excess connections are
/// dropped rather than parked, bounding memory against a runaway local client.
const MAX_PARKED_DURING_BUILD: usize = 128;

/// Run the backend for `key`. `build` is invoked only after this process wins the
/// bind, so the expensive state construction (which spawns background builds
/// touching the project DBs) never runs in a race loser.
///
/// Returns `Ok(())` both when this process served as the backend and exited on
/// idle, and when another live backend already owned the name (nothing to do).
pub async fn run<F>(build: F, key: BackendKey, idle: Duration) -> anyhow::Result<()>
where
    F: FnOnce() -> anyhow::Result<McpServer> + Send + 'static,
{
    let Some(listener) = bind(&key).await? else {
        tracing::info!("backend already serving this project; nothing to do");
        return Ok(());
    };

    // Build off the async runtime, and start accepting immediately. A bound socket that
    // isn't being accepted looks dead to a second daemon's liveness probe during the
    // multi-minute cold build — which would make that daemon reclaim (steal) our socket
    // and split the project across two backends. Draining the backlog from the first
    // accept keeps the probe honest; connections that arrive mid-build are parked and
    // served once the resident state is ready.
    let mut build_handle = tokio::task::spawn_blocking(build);
    let mut parked: Vec<TokioStream> = Vec::new();
    let server = loop {
        tokio::select! {
            built = &mut build_handle => {
                break built.map_err(|e| anyhow::anyhow!("backend build task panicked: {e}"))??;
            }
            accepted = listener.accept() => {
                let conn = accepted?;
                if !peer_authorized(&conn) {
                    tracing::warn!("rejected backend connection from an unauthorized peer");
                } else if parked.len() >= MAX_PARKED_DURING_BUILD {
                    // Keep draining the backlog past the cap so a concurrent liveness
                    // probe still succeeds, but drop the excess instead of parking it —
                    // bounding memory against a runaway local client during a long build.
                    tracing::warn!(
                        cap = MAX_PARKED_DURING_BUILD,
                        "too many connections during build; dropping excess"
                    );
                } else {
                    parked.push(conn);
                }
            }
        }
    };

    let guard = server.clone();
    // Flush/persist resident state on the way out (success or failure), mirroring
    // the stdio path, before the process exits.
    let result = serve(server, listener, parked, idle).await;
    guard.shutdown();
    result
}

async fn serve(
    server: McpServer,
    listener: TokioListener,
    parked: Vec<TokioStream>,
    idle: Duration,
) -> anyhow::Result<()> {
    tracing::info!(
        pid = std::process::id(),
        idle_secs = idle.as_secs(),
        parked = parked.len(),
        "broker backend listening"
    );

    // `active` counts in-flight sessions. The backend exits once it has had no active
    // session continuously for `idle`. `idle_since` marks when the current idle stretch
    // began (set at startup since there are no connections yet, cleared on any accept,
    // (re)started once a poll observes zero active sessions). Polling at a fraction of
    // `idle` makes the exit land ~`idle` after the last disconnect, not up to 2×.
    let active = Arc::new(AtomicU64::new(0));
    let mut idle_since = Some(Instant::now());
    let mut served = 0u64;

    // Serve the connections that arrived during the build first.
    for conn in parked {
        idle_since = None;
        served += 1;
        spawn_session(&server, &active, conn, served);
    }

    let poll = (idle / 4).clamp(Duration::from_millis(100), Duration::from_secs(15));
    let mut ticker = interval(poll);
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
                idle_since = None; // activity resets the idle clock
                served += 1;
                spawn_session(&server, &active, conn, served);
            }
            _ = ticker.tick() => {
                if active.load(Ordering::SeqCst) == 0 {
                    let since = *idle_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= idle {
                        tracing::info!(
                            idle_secs = idle.as_secs(),
                            "no connections for the idle window; shutting down backend"
                        );
                        break;
                    }
                } else {
                    idle_since = None;
                }
            }
        }
    }

    Ok(())
}

/// Serve one accepted connection on its own task. The [`ActiveGuard`] decrements the
/// active-session count on drop, so a panicking session can never strand the count and
/// keep the backend alive forever.
fn spawn_session(server: &McpServer, active: &Arc<AtomicU64>, conn: TokioStream, served: u64) {
    let guard = ActiveGuard::new(Arc::clone(active));
    tracing::debug!(active = active.load(Ordering::SeqCst), served, "broker accepted a session");
    let session = server.clone();
    tokio::spawn(async move {
        let _guard = guard;
        if let Err(e) = serve_stream(session, conn).await {
            tracing::warn!(error = %e, "broker session ended with error");
        }
    });
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
    match TokioStream::connect(backend_name(key)?).await {
        Ok(_) => Ok(true),
        // Only a clearly-absent listener counts as stale. Any other (transient) connect
        // error is treated as live, so we never unlink+rebind a backend that is actually
        // up — the conservative choice for the reclaim decision.
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(false)
        }
        Err(e) => {
            tracing::warn!(error = %e, "liveness probe inconclusive; assuming the backend is live");
            Ok(true)
        }
    }
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
