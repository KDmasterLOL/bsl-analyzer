//! The shared backend.
//!
//! Binds the per-project rendezvous *before* building the heavy [`SharedState`], so
//! a process that loses the launch race exits without ever starting a competing
//! build against the same per-project databases. The winner builds once and serves
//! every connecting proxy from it.
//!
//! Lifetime is tied to an **owner**: the first session that actually sends MCP traffic
//! becomes the owner, and the backend shuts down the moment that owner session ends —
//! for any reason (clean disconnect, crash, SIGKILL: the kernel closes the owner's
//! socket on process death, so the backend sees EOF either way). Shutdown drops every
//! other session, so each connected proxy gets EOF and exits in turn — no orphaned
//! backend, no lingering proxies. An orphan grace covers only the degenerate case where
//! an owner never establishes (e.g. the launching proxy died before its first request).
//!
//! [`SharedState`]: crate::SharedState

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Listener as TokioListener;
use interprocess::local_socket::tokio::Stream as TokioStream;
use interprocess::local_socket::ListenerOptions;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
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
/// `orphan_grace` bounds how long a backend with no owner and no active connections
/// waits before giving up — it does **not** keep a warm backend alive after its owner
/// leaves (that is owner-driven and immediate).
///
/// Returns `Ok(())` both when this process served as the backend and exited, and when
/// another live backend already owned the name (nothing to do).
pub async fn run<F>(build: F, key: BackendKey, orphan_grace: Duration) -> anyhow::Result<()>
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
    let result = serve(server, listener, parked, orphan_grace).await;
    guard.shutdown();
    result
}

async fn serve(
    server: McpServer,
    listener: TokioListener,
    parked: Vec<TokioStream>,
    orphan_grace: Duration,
) -> anyhow::Result<()> {
    tracing::info!(
        pid = std::process::id(),
        orphan_grace_secs = orphan_grace.as_secs(),
        parked = parked.len(),
        "broker backend listening"
    );

    // `active` counts in-flight sessions. `owner_claimed` flips once the first real
    // session (one that sends data) takes ownership; from then on the backend lives
    // exactly as long as that owner session, and `shutdown` is fired when it ends.
    let active = Arc::new(AtomicU64::new(0));
    let owner_claimed = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(Notify::new());
    let mut served = 0u64;
    // Live session tasks. On shutdown we abort them (and drop the listener) so every
    // connected proxy is severed deterministically — the cascade is an explicit teardown,
    // not a side effect of the process happening to exit afterwards.
    let mut sessions: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    // Serve the connections that arrived during the build first.
    for conn in parked {
        served += 1;
        sessions.push(spawn_session(&server, &active, &owner_claimed, &shutdown, conn, served));
    }

    // The orphan timer only guards the no-owner-ever window; once an owner claims the
    // backend it is disabled and shutdown is owner-driven. `orphan_since` marks when the
    // current owner-less, connection-less stretch began. Poll at a fraction of the grace
    // so the exit lands ~`orphan_grace` after the daemon goes quiet, not up to 2×.
    let poll = (orphan_grace / 4).clamp(Duration::from_millis(100), Duration::from_secs(15));
    let mut orphan_since = Some(Instant::now());
    let mut ticker = interval(poll);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            // The owner session ended: tear the backend down. Dropping the listener and
            // every other session socket gives each connected proxy an EOF, so they exit
            // in turn.
            _ = shutdown.notified() => {
                tracing::info!("owner session ended; shutting down backend");
                break;
            }
            accepted = listener.accept() => {
                let conn = accepted?;
                if !peer_authorized(&conn) {
                    tracing::warn!("rejected backend connection from an unauthorized peer");
                    continue;
                }
                served += 1;
                sessions.push(spawn_session(&server, &active, &owner_claimed, &shutdown, conn, served));
            }
            _ = ticker.tick() => {
                // Reap finished session handles so the bookkeeping vector can't grow without
                // bound across a long-lived owner's many short dependent sessions.
                sessions.retain(|h| !h.is_finished());
                // Once an owner is in charge the orphan guard never applies — the only exit
                // path is that owner leaving (handled above). Before then, exit if nothing
                // has connected for the grace, so a backend launched by a proxy that died
                // before its first request doesn't linger.
                if owner_claimed.load(Ordering::SeqCst) || active.load(Ordering::SeqCst) != 0 {
                    orphan_since = None;
                } else {
                    let since = *orphan_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= orphan_grace {
                        tracing::info!(
                            orphan_grace_secs = orphan_grace.as_secs(),
                            "no owner established within the orphan grace; shutting down backend"
                        );
                        break;
                    }
                }
            }
        }
    }

    // Teardown cascade: stop accepting and sever every still-connected session, so each
    // proxy gets an EOF and exits. Aborting a session task drops its socket; dropping the
    // listener frees the rendezvous name.
    drop(listener);
    for handle in &sessions {
        handle.abort();
    }

    Ok(())
}

/// Serve one accepted connection on its own task. The [`ActiveGuard`] decrements the
/// active-session count on drop, so a panicking session can never strand the count.
///
/// The connection is wrapped so the first byte the peer sends claims ownership (atomically,
/// first-claim-wins). When the session that won ownership ends, it fires `shutdown` — the
/// single trigger that takes the backend down. Returns the task handle so the serve loop
/// can abort it during the shutdown cascade.
///
/// Ownership contract: the owner is the **first session the backend observes sending data**,
/// not the first raw connection (which could be a liveness probe that sends nothing). In the
/// real proxy topology this is unambiguously the launching client: its connection is first in
/// the `parked` list and is spawned before the accept loop, so it polls its first byte — the
/// eagerly-sent `initialize` — before any later client, which connects only after the backend
/// already exists and is owned. Concurrent cold-starts by multiple real clients are the only
/// case where which one wins is scheduler-dependent; any of them leaving then tears the
/// backend down, which is the intended "backend dies with its owner" behavior regardless.
fn spawn_session(
    server: &McpServer,
    active: &Arc<AtomicU64>,
    owner_claimed: &Arc<AtomicBool>,
    shutdown: &Arc<Notify>,
    conn: TokioStream,
    served: u64,
) -> tokio::task::JoinHandle<()> {
    let guard = ActiveGuard::new(Arc::clone(active));
    tracing::debug!(active = active.load(Ordering::SeqCst), served, "broker accepted a session");
    let session = server.clone();
    let owner_claimed = Arc::clone(owner_claimed);
    let shutdown = Arc::clone(shutdown);
    tokio::spawn(async move {
        let _guard = guard;
        let is_owner = Arc::new(AtomicBool::new(false));
        let probe = OwnerProbe::new(conn, {
            let owner_claimed = Arc::clone(&owner_claimed);
            let is_owner = Arc::clone(&is_owner);
            // First byte on this connection: claim ownership if nobody has. A liveness
            // probe connects and closes without sending, so it never reaches here.
            move || {
                if owner_claimed
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    is_owner.store(true, Ordering::SeqCst);
                }
            }
        });
        if let Err(e) = serve_stream(session, probe).await {
            tracing::warn!(error = %e, "broker session ended with error");
        }
        if is_owner.load(Ordering::SeqCst) {
            shutdown.notify_one();
        }
    })
}

/// Wraps a backend connection and fires a one-shot callback the first time the peer
/// sends any data. A liveness probe connects and closes without writing, so it never
/// fires — only a real MCP session (which sends `initialize` immediately) does. This is
/// what lets ownership be claimed by the first *real* session rather than the first
/// raw connection.
struct OwnerProbe<S> {
    inner: S,
    on_first_byte: Option<Box<dyn FnOnce() + Send>>,
}

impl<S> OwnerProbe<S> {
    fn new(inner: S, on_first_byte: impl FnOnce() + Send + 'static) -> Self {
        Self { inner, on_first_byte: Some(Box::new(on_first_byte)) }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for OwnerProbe<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            if buf.filled().len() > before {
                if let Some(cb) = self.on_first_byte.take() {
                    cb();
                }
            }
        }
        poll
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for OwnerProbe<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Decrements the active-session count on drop (including unwind), so the accounting
/// stays correct even if a session task panics.
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
/// When the name is already taken we probe with a connect: a successful connect means
/// a live owner (defer); otherwise the name is stale. On unix that stale name is a
/// leftover socket file from a crashed backend, which we reclaim and rebind once (and if
/// a concurrent cold-starter beats us to the rebind, we defer to it). On Windows the
/// named pipe instance vanishes with its owner, so a stale name just means the pipe is
/// already gone and we rebind directly.
async fn bind(key: &BackendKey) -> anyhow::Result<Option<TokioListener>> {
    match ListenerOptions::new().name(backend_name(key)?).create_tokio() {
        Ok(listener) => Ok(Some(listener)),
        Err(e) if is_name_in_use(&e) => {
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
                    Err(e2) if is_name_in_use(&e2) => {
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
                // No file to unlink: a non-live probe means the previous pipe owner is
                // gone, so the name is free. Rebind once; if a concurrent starter won the
                // race, defer to it rather than erroring.
                match ListenerOptions::new().name(backend_name(key)?).create_tokio() {
                    Ok(listener) => Ok(Some(listener)),
                    Err(e2) if is_name_in_use(&e2) => {
                        if probe_live(key).await? {
                            Ok(None)
                        } else {
                            Err(e2.into())
                        }
                    }
                    Err(e2) => Err(e2.into()),
                }
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Whether a failed bind means the rendezvous name is already taken (so we should probe
/// and defer/reclaim rather than error out). Unix reports `AddrInUse`; Windows fails the
/// `CreateNamedPipe` of an already-existing instance with `ERROR_ACCESS_DENIED` (5)
/// instead, so we map that to the same "name in use" decision.
fn is_name_in_use(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::AddrInUse {
        return true;
    }
    #[cfg(windows)]
    {
        const ERROR_ACCESS_DENIED: i32 = 5;
        if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
            return true;
        }
    }
    false
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
