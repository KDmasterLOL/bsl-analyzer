//! Backend identity and rendezvous naming.
//!
//! The broker has no registry: a proxy finds its backend by recomputing the same
//! deterministic name from its own launch parameters. Two clients that resolve to
//! the same [`BackendKey`] therefore meet at the same socket; anything that should
//! fork a separate backend (different project, profile, binary version, or
//! embedding config) lands on a different name by construction.

use std::io;
use std::path::{Path, PathBuf};

use crate::McpProfile;

/// The identity of one shared backend. Equal keys ⇒ same socket ⇒ reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendKey {
    /// Canonicalized project root. Callers should pass an already-`canonicalize`d
    /// path; [`BackendKey::new`] canonicalizes defensively so `/p` and `/p/` and a
    /// symlink to either collapse to one key.
    source_dir: PathBuf,
    profile: McpProfile,
    /// Binary version (`CARGO_PKG_VERSION` of this crate = workspace version). An
    /// upgrade mints a fresh key, so a new proxy never speaks to an old backend on
    /// a possibly-changed protocol or schema; the old one drains by idle.
    version: &'static str,
    /// Fold of the embedding-related environment — see [`embedding_config_fingerprint`].
    config_fp: u64,
    /// Fold of the project's extension topology — see
    /// [`workspace_topology_fingerprint`]. Without it, the same source dir with a
    /// changed dependency graph would rendezvous with (and silently reuse) a daemon
    /// whose caches were built for the old graph.
    topology_fp: u64,
}

impl BackendKey {
    /// Build a key for the current process. `config_fp` should come from
    /// [`embedding_config_fingerprint`] so a backend serving one embedding model is
    /// never silently reused by a client expecting another; `topology_fp` from
    /// [`workspace_topology_fingerprint`] so a dependency-graph change forks a fresh
    /// backend (the old one drains by idle).
    pub fn new(
        source_dir: impl Into<PathBuf>,
        profile: McpProfile,
        config_fp: u64,
        topology_fp: u64,
    ) -> Self {
        let source_dir = source_dir.into();
        let source_dir = std::fs::canonicalize(&source_dir).unwrap_or(source_dir);
        Self { source_dir, profile, version: env!("CARGO_PKG_VERSION"), config_fp, topology_fp }
    }

    /// Stable 128-bit identity digest as 32 lowercase hex chars. Short enough to fit
    /// inside the `sun_path` limit when placed in a per-user runtime directory.
    pub fn digest(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.source_dir.as_os_str().as_encoded_bytes());
        hasher.update(b"\0");
        hasher.update(self.profile.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(self.version.as_bytes());
        hasher.update(b"\0");
        hasher.update(&self.config_fp.to_le_bytes());
        hasher.update(b"\0");
        hasher.update(&self.topology_fp.to_le_bytes());
        let hex = hasher.finalize().to_hex();
        hex[..32].to_owned()
    }

    /// Filesystem path of the unix-domain socket for this backend. Windows derives
    /// its named-pipe name from the same [`digest`](Self::digest) and applies an
    /// explicit current-user-only security descriptor when binding.
    ///
    /// On unix the result is checked against the `sockaddr_un.sun_path` budget so a
    /// long runtime/temp directory surfaces here as `InvalidInput` rather than a
    /// confusing `EINVAL` when the daemon later `bind`s.
    pub fn socket_path(&self) -> io::Result<PathBuf> {
        let path = runtime_socket_dir()?.join(format!("{}.sock", self.digest()));
        #[cfg(unix)]
        {
            let len = path.as_os_str().as_encoded_bytes().len();
            if len >= SUN_PATH_MAX {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "broker socket path is {len} bytes, exceeds the sun_path budget \
                         of {SUN_PATH_MAX}: {}",
                        path.display()
                    ),
                ));
            }
        }
        Ok(path)
    }
}

/// `sun_path` capacity (including the trailing NUL) the kernel accepts for a
/// unix-domain socket address. A path of `>= SUN_PATH_MAX` bytes cannot be bound.
#[cfg(target_os = "linux")]
const SUN_PATH_MAX: usize = 108;
#[cfg(target_os = "macos")]
const SUN_PATH_MAX: usize = 104;
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
const SUN_PATH_MAX: usize = 104;

/// Which parent directory a broker socket lives under. The variants are ordered by the
/// precedence in [`socket_dir_source`]; `Xdg` and `CanonicalRunUser` are both kernel/systemd
/// managed and uid-private, so they share the trusted-parent handling.
enum SocketDirSource {
    /// `$XDG_RUNTIME_DIR` was set.
    Xdg(PathBuf),
    /// `$XDG_RUNTIME_DIR` was unset, but the canonical `/run/user/<euid>` runtime dir exists
    /// and is ours — the same tmpfs the env var would have pointed to.
    CanonicalRunUser(PathBuf),
    /// No per-user runtime dir at all (headless / non-systemd / macOS): use shared temp.
    TmpFallback,
}

/// Pure precedence decision (no filesystem or env access, so it is unit-testable): an explicit
/// `$XDG_RUNTIME_DIR` wins, else the canonical per-user runtime dir if one was found, else the
/// shared-temp fallback. The middle tier is what lets a spawner that *drops* `$XDG_RUNTIME_DIR`
/// (e.g. Codex launching the backend) still rendezvous with one that keeps it at the standard
/// `/run/user/<euid>`, instead of forking a second multi-GB backend under `/tmp`. It does NOT
/// converge a process that deliberately points `$XDG_RUNTIME_DIR` at a *non-standard* path with
/// one that dropped the variable — the explicit env still wins above, which is the right call
/// and an unavoidable split short of propagating the variable.
fn socket_dir_source(xdg: Option<PathBuf>, canonical_run_user: Option<PathBuf>) -> SocketDirSource {
    if let Some(base) = xdg {
        return SocketDirSource::Xdg(base);
    }
    if let Some(base) = canonical_run_user {
        return SocketDirSource::CanonicalRunUser(base);
    }
    SocketDirSource::TmpFallback
}

/// The canonical per-user runtime dir `/run/user/<euid>` — the path `$XDG_RUNTIME_DIR` is set to
/// on a logind system — but only when it is, by its own metadata, a directory we own at mode
/// `0700`. `None` (→ temp fallback) otherwise. The trust is self-contained: `symlink_metadata`
/// (not `metadata`) refuses a symlink standing in for the dir, and the owner+mode check matches
/// the bar [`create_private_dir`] holds our own leaves to — we don't assume "it's systemd's".
/// Keyed on `euid`, never `$USER`/env, so two processes for the same user agree on the path.
#[cfg(unix)]
fn canonical_run_user_dir() -> Option<PathBuf> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let base = PathBuf::from(format!("/run/user/{}", current_euid()));
    let meta = std::fs::symlink_metadata(&base).ok()?;
    let ours = meta.is_dir()
        && meta.uid() == current_euid()
        && (meta.permissions().mode() & 0o777) == 0o700;
    ours.then_some(base)
}

/// Per-user directory that holds broker sockets. Prefers `$XDG_RUNTIME_DIR`, then the canonical
/// `/run/user/<euid>` (so a dropped env var doesn't split the rendezvous), then a uid/user-scoped
/// subdir of the system temp dir. Created `0700` on unix so a co-tenant cannot enumerate or
/// connect to another user's backend.
pub fn runtime_socket_dir() -> io::Result<PathBuf> {
    #[cfg(unix)]
    let canonical = canonical_run_user_dir();
    #[cfg(not(unix))]
    let canonical = None;

    match socket_dir_source(dirs::runtime_dir(), canonical) {
        // Both are kernel/systemd managed and already uid-private; trust them as the parent
        // and create/validate only our own leaf under it.
        SocketDirSource::Xdg(base) | SocketDirSource::CanonicalRunUser(base) => {
            let dir = base.join("bsl-mcp");
            create_private_dir(&dir)?;
            Ok(dir)
        }
        // Fall back into a shared temp dir. Every level we descend through must be ours —
        // otherwise an attacker who owns an ancestor could swap our socket dir after it is
        // validated. So validate the sanitized base (rejecting an attacker-pre-created one)
        // before creating the leaf, never descending recursively through an unvalidated
        // parent. `/tmp`'s sticky bit then prevents anyone from renaming a base we own.
        SocketDirSource::TmpFallback => {
            // The user tag must be a safe single path component — sanitize to alnum/_/-
            // so a spoofed `USER=../x` cannot escape the temp dir.
            let raw =
                std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
            let mut who: String = raw
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if who.is_empty() {
                who.push_str("default");
            }
            let base = std::env::temp_dir().join(format!("bsl-mcp-{who}"));
            create_private_dir(&base)?;
            let dir = base.join("bsl-mcp");
            create_private_dir(&dir)?;
            Ok(dir)
        }
    }
}

/// Current effective uid. `geteuid()` has no failure mode or preconditions.
#[cfg(unix)]
pub(crate) fn current_euid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    match std::fs::metadata(dir) {
        Ok(meta) if meta.is_dir() => {
            // A pre-existing dir must be private AND owned by us; otherwise a co-tenant
            // could have pre-created a 0700 dir they own to host (and observe or
            // pre-empt) our socket. This matters for the shared-/tmp fallback;
            // `$XDG_RUNTIME_DIR` is already kernel-private to our uid.
            let mode = meta.permissions().mode() & 0o777;
            if mode != 0o700 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "broker runtime dir {} has mode {mode:#o}, expected 0o700",
                        dir.display()
                    ),
                ));
            }
            if meta.uid() != current_euid() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "broker runtime dir {} is owned by uid {}, not the current user {}",
                        dir.display(),
                        meta.uid(),
                        current_euid()
                    ),
                ));
            }
            Ok(())
        }
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("broker runtime path {} exists and is not a directory", dir.display()),
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Non-recursive: the parent must already exist and be trusted (the caller
            // validates each level top-down), so we never create or descend through an
            // unvalidated ancestor.
            std::fs::DirBuilder::new().mode(0o700).create(dir)
        }
        Err(e) => Err(e),
    }
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

/// Fold the embedding-relevant environment into a stable 64-bit fingerprint. Only
/// variables that change what an index *means* are included (endpoint, model,
/// dimension, provider); auth (`EMBEDDING_API_KEY`) and perf knobs are excluded so
/// key rotation and tuning do not fork the backend — and no secret-derived bytes
/// land in a socket name.
pub fn embedding_config_fingerprint() -> u64 {
    const KEYS: [&str; 4] =
        ["EMBEDDING_URL", "EMBEDDING_MODEL", "EMBEDDING_DIM", "EMBEDDING_PROVIDER"];
    let mut hasher = blake3::Hasher::new();
    for key in KEYS {
        hasher.update(key.as_bytes());
        hasher.update(b"=");
        if let Ok(value) = std::env::var(key) {
            hasher.update(value.as_bytes());
        }
        hasher.update(b"\0");
    }
    let bytes = hasher.finalize();
    u64::from_le_bytes(bytes.as_bytes()[..8].try_into().expect("blake3 yields >= 8 bytes"))
}

/// Env var carrying the spawning proxy's frozen topology fingerprint to the daemon
/// child, so both sides key the SAME rendezvous even if the config changes between
/// their respective project reads.
pub const TOPOLOGY_FP_ENV: &str = "BSL_MCP_TOPOLOGY_FP";

/// Stable 64-bit identity of the project's extension topology at `source_dir`, for
/// backend keying: a daemon whose graph/search/diagnostics caches were built for
/// one dependency graph must not be reused by a client whose config now declares
/// another. Proxy and daemon both derive it through this one function (from the
/// same `--source-dir`), so they agree by construction. An invalid or unloadable
/// project folds as `0` on both sides — still one rendezvous, just untagged.
pub fn workspace_topology_fingerprint(source_dir: &Path) -> u64 {
    match project_model::Project::new(source_dir) {
        Ok(project) => crate::graph::scan::topology_hex_u64(
            &project.extension_topology().fingerprint().to_hex(),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "broker key: project topology unavailable, folding as 0");
            0
        }
    }
}

/// Cross-platform rendezvous name for a backend.
///
/// Unix uses the filesystem socket path ([`BackendKey::socket_path`]) so we own
/// stale-file recovery. Windows uses a namespaced named pipe derived from the
/// same digest, with pipe security applied by the daemon listener.
pub fn backend_name(key: &BackendKey) -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::{GenericFilePath, ToFsName};
        key.socket_path()?.to_fs_name::<GenericFilePath>()
    }
    #[cfg(windows)]
    {
        use interprocess::local_socket::{GenericNamespaced, ToNsName};
        let _ = runtime_socket_dir();
        format!("bsl-mcp-{}.sock", key.digest()).to_ns_name::<GenericNamespaced>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(dir: &str, profile: McpProfile, fp: u64) -> BackendKey {
        // Bypass `new`'s canonicalize (the path need not exist in unit tests).
        BackendKey {
            source_dir: PathBuf::from(dir),
            profile,
            version: "test",
            config_fp: fp,
            topology_fp: 0,
        }
    }

    #[test]
    fn digest_is_deterministic_and_32_hex() {
        let k = key("/srv/erp", McpProfile::Workspace, 7);
        let d = k.digest();
        assert_eq!(d.len(), 32);
        assert!(d.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(d, k.digest());
    }

    #[test]
    fn each_identity_axis_forks_the_digest() {
        let base = key("/srv/erp", McpProfile::Workspace, 7).digest();
        assert_ne!(base, key("/srv/other", McpProfile::Workspace, 7).digest(), "source_dir");
        assert_ne!(base, key("/srv/erp", McpProfile::Reference, 7).digest(), "profile");
        assert_ne!(base, key("/srv/erp", McpProfile::Workspace, 8).digest(), "config_fp");

        let mut bumped = key("/srv/erp", McpProfile::Workspace, 7);
        bumped.version = "test-next";
        assert_ne!(base, bumped.digest(), "version");

        let mut retopologized = key("/srv/erp", McpProfile::Workspace, 7);
        retopologized.topology_fp = 1;
        assert_ne!(
            base,
            retopologized.digest(),
            "same dir with a different dependency graph must land on a different socket"
        );
    }

    #[test]
    fn socket_path_sits_under_runtime_dir_and_is_named_by_digest() {
        let k = key("/srv/erp", McpProfile::Workspace, 7);
        let path = k.socket_path().expect("runtime dir resolvable");
        assert_eq!(path.parent(), Some(runtime_socket_dir().unwrap().as_path()));
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some(format!("{}.sock", k.digest()).as_str())
        );
    }

    #[test]
    fn fingerprint_tracks_only_identity_env() {
        // No assertion on the absolute value (depends on ambient env); just that the
        // function is callable and stable within one environment.
        assert_eq!(embedding_config_fingerprint(), embedding_config_fingerprint());
    }

    #[test]
    fn socket_dir_source_falls_back_to_canonical_when_xdg_is_dropped() {
        use super::{socket_dir_source, SocketDirSource};
        let xdg = PathBuf::from("/run/user/1000");
        let canonical = PathBuf::from("/run/user/1000");

        // $XDG_RUNTIME_DIR set → it wins outright.
        assert!(matches!(
            socket_dir_source(Some(xdg.clone()), Some(canonical.clone())),
            SocketDirSource::Xdg(p) if p == xdg
        ));
        // The regression this fix targets: a spawner (e.g. Codex) dropped $XDG_RUNTIME_DIR but
        // the canonical /run/user/<euid> is there — use it, NOT the /tmp fallback, so both
        // processes meet at the same socket instead of forking a second backend.
        assert!(matches!(
            socket_dir_source(None, Some(canonical.clone())),
            SocketDirSource::CanonicalRunUser(p) if p == canonical
        ));
        // Neither available → shared-temp fallback.
        assert!(matches!(socket_dir_source(None, None), SocketDirSource::TmpFallback));
    }
}
