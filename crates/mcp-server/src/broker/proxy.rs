//! The thin client-facing proxy.
//!
//! Spawned by the MCP client (Claude/Codex) on stdio exactly like the plain server,
//! but instead of building any analysis state it connects to the shared per-project
//! backend — launching it detached if absent — and relays the MCP byte stream
//! between the client's stdio and the backend socket. Holds no state; cheap to
//! start and stop, so per-client/per-review churn costs a socket connect, not a
//! multi-gigabyte rebuild.

use std::fs::OpenOptions;
use std::process::{Child, Command};
use std::time::Duration;

use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::tokio::Stream as TokioStream;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;

use crate::broker::name::{backend_name, runtime_socket_dir, BackendKey};

/// Upper bound on waiting for a backend to become reachable. The backend binds
/// before its heavy build, so this only needs to cover process startup — but we
/// keep it generous to ride out a slow cold start.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for a launched backend before assuming it died (crashed during
/// startup) and launching another. Re-launches are safe: a redundant backend loses
/// the bind race and exits immediately.
const RESPAWN_INTERVAL: Duration = Duration::from_secs(3);

/// Roll the per-backend log over once it passes this size, so it cannot grow without
/// bound across many backend generations.
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

/// Outcome of a proxy attempt, separating a pre-session connect failure from a
/// mid-session relay failure — they need different handling by the caller.
pub enum ProxyOutcome {
    /// The session was served to completion (or the client disconnected normally).
    Served,
    /// The backend could not be reached or launched. No stdin was consumed yet, so the
    /// caller may safely fall back to serving the client directly over stdio.
    Unavailable(anyhow::Error),
}

/// Connect to the backend for `key`, launching it via `daemon_cmd` if it is not yet
/// reachable, then relay stdio to it until either side closes.
///
/// A connect-phase failure is returned as [`ProxyOutcome::Unavailable`] (stdin
/// untouched → safe to fall back). A relay-phase failure propagates as `Err`: by then
/// the stdin pump has consumed bytes, so re-serving on the same stream would be wrong.
pub async fn connect_or_launch(
    key: BackendKey,
    daemon_cmd: Command,
) -> anyhow::Result<ProxyOutcome> {
    let stream = match connect_with_launch(&key, daemon_cmd).await {
        Ok(stream) => stream,
        Err(e) => return Ok(ProxyOutcome::Unavailable(e)),
    };
    relay_stdio(stream).await?;
    Ok(ProxyOutcome::Served)
}

async fn connect_with_launch(
    key: &BackendKey,
    mut daemon_cmd: Command,
) -> anyhow::Result<TokioStream> {
    // Initial direct connect.
    //
    // On Unix the per-user runtime dir is the trust boundary (mode 0700, owner
    // checked), so a connect succeeding against a socket in that dir proves the
    // peer is ours — return the stream directly.
    //
    // On Windows the deterministic pipe name can be raced by a hostile local
    // user pre-creating the pipe with their own DACL, so any successful connect
    // must pass the same `verify_pipe_server_trusted` gate used by the polling
    // loop below. A trusted already-running backend is reused; an unverified
    // pipe is dropped and we fall through to launch + verify our own child.
    #[cfg(unix)]
    if let Ok(stream) = TokioStream::connect(backend_name(key)?).await {
        return Ok(stream);
    }
    #[cfg(windows)]
    if let Ok(stream) = TokioStream::connect(backend_name(key)?).await {
        if let Some(trusted) = verify_or_drop_peer(stream) {
            return Ok(trusted);
        }
    }

    // No backend yet: launch one detached, then poll-connect. If the backend never
    // becomes reachable (e.g. it crashed during startup), relaunch periodically —
    // bind-wins makes a redundant launch self-correcting.
    let mut children: Vec<Child> = Vec::new();
    children.push(spawn_detached(key, &mut daemon_cmd)?);

    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    let mut next_respawn = Instant::now() + RESPAWN_INTERVAL;
    let mut delay = Duration::from_millis(25);
    loop {
        let last_err = match TokioStream::connect(backend_name(key)?).await {
            Ok(stream) => {
                if let Some(trusted) = verify_or_drop_peer(stream) {
                    reap(&mut children);
                    return Ok(trusted);
                }
                // Windows: the pre-existing pipe did not pass the trust gate.
                // Keep polling — our own launched backend may win the bind on a
                // later attempt; surface the last real connect error on timeout.
                None
            }
            Err(e) => Some(e),
        };
        if Instant::now() >= deadline {
            reap(&mut children);
            let detail = last_err
                .map(|e| format!("{e}"))
                .unwrap_or_else(|| "all successful connects were from unverified pipes".to_owned());
            return Err(anyhow::anyhow!(
                "broker backend did not become reachable within {}s: {detail}",
                LAUNCH_TIMEOUT.as_secs()
            ));
        }
        if Instant::now() >= next_respawn {
            reap(&mut children);
            children.push(spawn_detached(key, &mut daemon_cmd)?);
            next_respawn = Instant::now() + RESPAWN_INTERVAL;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_millis(500));
    }
}

/// Verify a connected peer is a trusted backend, returning the stream if
/// trusted or `None` if it must be dropped.
///
/// On Unix the runtime dir + DACL is the trust boundary, so any successful
/// connect is accepted unchanged. On Windows the deterministic pipe name is
/// raceable, so `security::verify_pipe_server_trusted` introspects the server
/// PID's image path and owner SID via `sysinfo` and compares them against the
/// current process — the same gate used by `daemon::probe_live`. No
/// project-local `unsafe`, no handwritten Win32 FFI.
#[cfg(unix)]
fn verify_or_drop_peer(stream: TokioStream) -> Option<TokioStream> {
    Some(stream)
}

#[cfg(windows)]
fn verify_or_drop_peer(stream: TokioStream) -> Option<TokioStream> {
    if crate::broker::security::verify_pipe_server_trusted(&stream) {
        Some(stream)
    } else {
        None
    }
}

/// Reap any spawned backend that has already exited (a race loser), clearing the
/// zombie. The live winner reports `None` and keeps running; its handle is dropped
/// without killing the process, which reparents to init when this proxy exits.
fn reap(children: &mut Vec<Child>) {
    children.retain_mut(|child| matches!(child.try_wait(), Ok(None)));
}

/// Relay bytes both ways. The backend→client direction is authoritative: it is
/// drained to EOF and flushed before returning, so a final response is never
/// truncated by the client closing stdin first. The stdin→backend pump runs
/// concurrently and half-closes the write side on stdin EOF so the backend sees the
/// session end.
async fn relay_stdio(stream: TokioStream) -> anyhow::Result<()> {
    let (mut from_backend, mut to_backend) = stream.split();

    let pump_in = tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let _ = tokio::io::copy(&mut stdin, &mut to_backend).await;
        let _ = to_backend.shutdown().await;
    });

    let mut stdout = tokio::io::stdout();
    let copied = tokio::io::copy(&mut from_backend, &mut stdout).await;
    let _ = stdout.flush().await;

    // Backend closed (after seeing our EOF, or on its own): the stdin pump is now
    // irrelevant.
    pump_in.abort();
    copied?;
    Ok(())
}

/// Spawn the backend so it outlives this proxy: a new process group (unix) /
/// detached, new-group process (windows), with stdin closed and stdout+stderr
/// redirected to a per-backend log file in the runtime dir. Returns the child
/// handle so a fast-exiting race loser can be reaped.
fn spawn_detached(key: &BackendKey, cmd: &mut Command) -> anyhow::Result<Child> {
    let log_path = runtime_socket_dir()?.join(format!("{}.log", key.digest()));
    // Bound the per-backend log instead of appending forever: roll it over when it
    // passes the cap. A concurrent race-loser's truncate is harmless — losers exit
    // immediately and write next to nothing.
    let oversized = std::fs::metadata(&log_path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false);
    let mut log_opts = OpenOptions::new();
    log_opts.create(true);
    if oversized {
        log_opts.write(true).truncate(true);
    } else {
        log_opts.append(true);
    }
    let log = log_opts.open(&log_path)?;
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log.try_clone()?))
        .stderr(std::process::Stdio::from(log));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn()?;
    tracing::info!(log = %log_path.display(), "launched broker backend");
    Ok(child)
}
