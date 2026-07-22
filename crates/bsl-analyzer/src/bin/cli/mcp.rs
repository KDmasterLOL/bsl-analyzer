use std::{collections::BTreeMap, env, error::Error, io, path::PathBuf, time::Duration};

use clap::{Args, Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum McpCommand {
    Serve(McpServeArgs),

    Install(McpInstallArgs),
}

#[derive(Args, Clone)]
pub struct McpServeArgs {
    #[arg(long = "profile", value_enum)]
    runtime_profile: McpProfileCli,

    #[arg(short = 's', long = "source-dir", required_if_eq("runtime_profile", "workspace"))]
    source_dir: Option<PathBuf>,

    /// Connection mode. `stdio` (default) serves one client directly. `broker`
    /// connects to a shared per-project backend (launching it if absent) and relays
    /// — so many clients/reviews reuse one heavy process. The backend stays warm across
    /// client disconnects and reconnects, then idles out on its own once no client has
    /// used it for the idle TTL (`BSL_MCP_IDLE_TTL_SECS`, default 300s); a backend that
    /// never served any traffic gives up after a short orphan grace
    /// (`BSL_MCP_ORPHAN_GRACE_SECS`, default 30s). `daemon` *is* that backend and is
    /// launched internally by a broker proxy; it is not meant to be run directly.
    #[arg(long = "mode", value_enum, default_value = "stdio")]
    mode: McpServeMode,

    #[arg(long)]
    onec_url: Option<String>,

    #[arg(long, default_value = "")]
    onec_user: String,

    #[arg(long, default_value = "")]
    onec_password: String,
}

impl McpCommand {
    /// Opt-in on-disk log destination for a daemon backend, enabled by
    /// `BSL_MCP_DAEMON_LOG` (`1`/`true`/`yes`/`on` → `.build/bsl-analyzer-daemon.log`
    /// next to the graph and search databases; any other non-empty value is taken
    /// as an explicit path). A daemon is spawned by a broker proxy with no
    /// terminal, so without a file its diagnostics (build heartbeat, stall
    /// watchdog reports) vanish into a closed stderr — set the variable when
    /// investigating daemon behaviour. Off by default: routine deployments should
    /// not accrete log files. The stall watchdog separately drops a one-shot
    /// report file next to the graph database when a build wedges, so that
    /// post-mortem survives even with logging off.
    ///
    /// The file is APPENDED across runs (each run stamps a session-start line),
    /// never truncated or renamed at startup: the broker's spawn race can start
    /// several daemon candidates within seconds, and a start-time rotation by a
    /// losing candidate would rename the winning daemon's live log out from
    /// under it. Rotation to `.prev` happens only on size (a wedged-build trail
    /// is kilobytes, so the rotation window and a spawn race cannot coincide).
    ///
    /// Returns `None` for non-daemon modes (stdio/broker keep stderr: a direct
    /// client or terminal is attached), when the opt-in is absent, and when the
    /// directory cannot be prepared (logging then falls back to stderr rather
    /// than failing startup).
    pub fn default_daemon_log_file(&self) -> Option<PathBuf> {
        self.daemon_log_file_for(std::env::var("BSL_MCP_DAEMON_LOG").ok().as_deref())
    }

    fn daemon_log_file_for(&self, opt_in: Option<&str>) -> Option<PathBuf> {
        const ROTATE_BYTES: u64 = 50 * 1024 * 1024;

        let McpCommand::Serve(args) = self else { return None };
        if !matches!(args.mode, McpServeMode::Daemon) {
            return None;
        }
        let log_path = match opt_in.map(str::trim) {
            None | Some("" | "0" | "false" | "no" | "off") => return None,
            Some("1" | "true" | "yes" | "on") => {
                let build_dir =
                    args.source_dir.as_deref().unwrap_or_else(|| ".".as_ref()).join(".build");
                std::fs::create_dir_all(&build_dir).ok()?;
                build_dir.join("bsl-analyzer-daemon.log")
            }
            Some(path) => {
                // An unpreparable explicit path degrades to stderr like every
                // other failure here — opt-in logging must never fail startup.
                let path = PathBuf::from(path);
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent).ok()?;
                }
                path
            }
        };
        if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
            // Append `.prev` to the whole file name (`with_extension` would
            // truncate a dotted name like `daemon.custom.log`).
            let mut rotated = log_path.file_name().unwrap_or_default().to_os_string();
            rotated.push(".prev");
            let _ = std::fs::rename(&log_path, log_path.with_file_name(rotated));
        }
        Some(log_path)
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum McpServeMode {
    Stdio,
    Broker,
    Daemon,
}

#[derive(Args)]
pub struct McpInstallArgs {
    #[arg(long, value_enum)]
    target: InstallTargetCli,

    #[arg(long, value_enum)]
    scope: Option<InstallScopeCli>,

    #[arg(long, value_enum, default_value_t = InstallPresetCli::Workspace)]
    preset: InstallPresetCli,

    #[arg(long)]
    name: Option<String>,

    #[arg(short = 's', long = "source-dir")]
    source_dir: Option<PathBuf>,

    #[arg(long)]
    onec_url: Option<String>,

    #[arg(long, default_value = "")]
    onec_user: String,

    #[arg(long, default_value = "")]
    onec_password: String,

    #[arg(long = "env", value_parser = parse_env_pair)]
    env: Vec<(String, String)>,

    #[arg(long)]
    force: bool,

    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum InstallTargetCli {
    Codex,
    Gemini,
    Claude,
    Cursor,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum InstallScopeCli {
    User,
    Project,
    Local,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum InstallPresetCli {
    #[default]
    Workspace,
    Reference,
    Recommended,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum McpProfileCli {
    Workspace,
    Reference,
}

pub fn run(command: McpCommand) -> Result<(), Box<dyn Error + Send + Sync>> {
    match command {
        McpCommand::Serve(args) => run_mcp_serve(args),
        McpCommand::Install(args) => run_mcp_install(args),
    }
}

fn run_mcp_serve(args: McpServeArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    // The broker passes the 1C credential to the detached daemon via the environment
    // (not argv, which `ps` would expose for the backend's whole lifetime), so fall
    // back to it when the flag is absent.
    let raw_password = if args.onec_password.is_empty() {
        env::var("BSL_ONEC_PASSWORD").unwrap_or_default()
    } else {
        args.onec_password.clone()
    };
    let password = decode_password(&raw_password);
    let profile = match args.runtime_profile {
        McpProfileCli::Workspace => mcp_server::McpProfile::Workspace,
        McpProfileCli::Reference => mcp_server::McpProfile::Reference,
    };

    if matches!(profile, mcp_server::McpProfile::Reference)
        && (args.onec_url.is_some() || !args.onec_user.is_empty() || !args.onec_password.is_empty())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reference profile does not accept --onec-url/--onec-user/--onec-password",
        )
        .into());
    }

    match resolve_serve_mode(args.mode, profile)? {
        McpServeMode::Stdio => {
            run_mcp_server(profile, args.source_dir, args.onec_url, &args.onec_user, &password)
        }
        McpServeMode::Broker => run_mcp_broker(profile, &args, &password),
        McpServeMode::Daemon => {
            run_mcp_daemon(profile, args.source_dir, args.onec_url, &args.onec_user, &password)
        }
    }
}

/// Resolve the effective serve mode.
///
/// An explicit `--mode` always wins (the re-exec'd daemon passes `--mode daemon`, so
/// inheriting the env switch can't turn it back into a proxy). Otherwise the broker
/// applies only to the heavy `workspace` profile, decided by env then platform:
///
/// - `BSL_MCP_BROKER` is an explicit opt-in/out on any OS. It is the only signal that
///   reaches Codex: Codex imports a project's `.mcp.json` but reconstructs the argv
///   (dropping extra flags) and does not propagate its `env` block — so the activation
///   has to live in the binary's own default, not the client config.
/// - With no override, the heavy `workspace` profile defaults to the broker on every
///   platform. Windows is now included: the named-pipe transport carries an explicit
///   current-user security descriptor and verifies the backend's identity (defeating pipe
///   squatting). The backend stays warm across client reconnects and idles out on its own.
///   `BSL_MCP_BROKER=0` forces plain stdio anywhere.
fn resolve_serve_mode(
    flag: McpServeMode,
    profile: mcp_server::McpProfile,
) -> Result<McpServeMode, io::Error> {
    let context =
        ServeModeContext { broker_override: env_broker_override(), platform_default_broker: true };
    Ok(resolve_serve_mode_with_override(flag, profile, context))
}

#[derive(Clone, Copy)]
struct ServeModeContext {
    broker_override: Option<bool>,
    /// Whether this platform defaults the workspace profile to the broker. Injected so
    /// the precedence (`--mode` flag → env override → profile → platform) stays
    /// unit-testable; production passes `true` on every platform.
    platform_default_broker: bool,
}

fn resolve_serve_mode_with_override(
    flag: McpServeMode,
    profile: mcp_server::McpProfile,
    context: ServeModeContext,
) -> McpServeMode {
    if !matches!(flag, McpServeMode::Stdio) {
        return flag;
    }
    if !matches!(profile, mcp_server::McpProfile::Workspace) {
        return McpServeMode::Stdio;
    }
    match context.broker_override {
        Some(true) => return McpServeMode::Broker,
        Some(false) => return McpServeMode::Stdio,
        None => {}
    }
    if context.platform_default_broker {
        McpServeMode::Broker
    } else {
        McpServeMode::Stdio
    }
}

/// `BSL_MCP_BROKER` parsed as an explicit tristate: `Some(true|false)` for a recognized
/// truthy/falsy value, `None` when unset or unrecognized (defer to the platform default).
fn env_broker_override() -> Option<bool> {
    let value = env::var("BSL_MCP_BROKER").ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The broker proxy: connect to (or launch) the shared per-project backend and relay
/// stdio to it. The backend is this same binary re-executed with `--mode daemon` and
/// the same launch parameters, so it resolves to the same backend identity.
fn run_mcp_broker(
    profile: mcp_server::McpProfile,
    args: &McpServeArgs,
    onec_password: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let source_dir = require_workspace_broker(profile, args.source_dir.clone())?;

    let key = mcp_server::broker::BackendKey::new(
        &source_dir,
        profile,
        mcp_server::broker::embedding_config_fingerprint(),
    );

    let mut cmd = std::process::Command::new(env::current_exe()?);
    cmd.arg("mcp")
        .arg("serve")
        .arg("--profile")
        .arg("workspace")
        .arg("--source-dir")
        .arg(&source_dir)
        .arg("--mode")
        .arg("daemon");
    if let Some(url) = &args.onec_url {
        cmd.arg("--onec-url").arg(url);
    }
    if !args.onec_user.is_empty() {
        cmd.arg("--onec-user").arg(&args.onec_user);
    }
    // Credential via environment, not argv: the daemon is long-lived, and argv is
    // visible in `ps`. Passed in the form we received it (possibly `base64:`) so the
    // daemon decodes it exactly as a direct invocation would.
    if !args.onec_password.is_empty() {
        cmd.env("BSL_ONEC_PASSWORD", &args.onec_password);
    }

    tracing::info!(?source_dir, "Starting MCP broker proxy");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let result = rt.block_on(mcp_server::broker::proxy::connect_or_launch(key, cmd));
    drop(rt);

    use mcp_server::broker::proxy::ProxyOutcome;
    match result? {
        ProxyOutcome::Served => Ok(()),
        // Connect-phase failure only: no client bytes were relayed yet, so fall back to
        // serving directly over stdio (the stdio server answers the pending `initialize`
        // cleanly). A relay-phase failure surfaced as `Err` above and is fatal — we must
        // not re-serve on a half-consumed stdin.
        ProxyOutcome::Unavailable(e) => {
            tracing::warn!(error = %e, "broker backend unavailable; serving directly over stdio");
            run_mcp_server(
                profile,
                Some(source_dir),
                args.onec_url.clone(),
                &args.onec_user,
                onec_password,
            )
        }
    }
}

/// The shared backend: build the resident state once and serve every connecting
/// proxy from it until idle.
fn run_mcp_daemon(
    profile: mcp_server::McpProfile,
    source_dir: Option<PathBuf>,
    onec_url: Option<String>,
    onec_user: &str,
    onec_password: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let source_dir = require_workspace_broker(profile, source_dir)?;

    let key = mcp_server::broker::BackendKey::new(
        &source_dir,
        profile,
        mcp_server::broker::embedding_config_fingerprint(),
    );

    // Build only after the daemon wins the bind, so a race loser never starts a
    // competing workspace build against the same per-project databases.
    let onec_user = onec_user.to_owned();
    let onec_password = onec_password.to_owned();
    let build = move || {
        build_server(profile, Some(source_dir), onec_url, &onec_user, &onec_password)
            .map_err(|e| anyhow::anyhow!("{e}"))
    };

    tracing::info!("Starting MCP broker backend (daemon)");
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let result = rt.block_on(mcp_server::broker::daemon::run(
        build,
        key,
        broker_orphan_grace(),
        broker_idle_ttl(),
    ));
    drop(rt);
    result?;
    Ok(())
}

/// Broker/daemon modes serve the heavy workspace backend and key on its source dir.
fn require_workspace_broker(
    profile: mcp_server::McpProfile,
    source_dir: Option<PathBuf>,
) -> Result<PathBuf, Box<dyn Error + Send + Sync>> {
    if !matches!(profile, mcp_server::McpProfile::Workspace) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "broker/daemon modes require --profile workspace",
        )
        .into());
    }
    source_dir.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "broker/daemon modes require --source-dir")
            .into()
    })
}

/// Grace window for a backend that has never served real MCP traffic and has no live
/// connections — the launching proxy died before its first request, or only liveness
/// probes connected. It bounds how long such a never-used backend lingers before giving
/// up; a backend that *has* served traffic uses the longer [`broker_idle_ttl`] instead.
/// Default 30s; override via `BSL_MCP_ORPHAN_GRACE_SECS`.
fn broker_orphan_grace() -> Duration {
    let secs =
        env::var("BSL_MCP_ORPHAN_GRACE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    Duration::from_secs(secs)
}

/// How long a backend that has served real MCP traffic stays warm after its last session
/// disconnects, so an editor that restarts or cycles its MCP link (opencode, Zed, …)
/// reconnects to the resident state instead of paying the multi-second cold rebuild.
/// Default 300s (5min), kept modest because a warm workspace backend is memory-heavy and
/// each project keeps its own; override via `BSL_MCP_IDLE_TTL_SECS` (legacy
/// `BSL_MCP_IDLE_SECS` still honored).
fn broker_idle_ttl() -> Duration {
    let secs = env::var("BSL_MCP_IDLE_TTL_SECS")
        .or_else(|_| env::var("BSL_MCP_IDLE_SECS"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

fn run_mcp_install(args: McpInstallArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_analyzer::mcp_install::{
        self, default_server_name, InstallPreset, InstallRequest, InstallScope, InstallTarget,
        InstallTargetSelector,
    };

    let project_dir = env::current_dir()?;
    let env = args.env.into_iter().collect::<BTreeMap<_, _>>();
    let password = decode_password(&args.onec_password);
    let source_dir = args.source_dir.unwrap_or_else(|| project_dir.clone());
    let name = args.name;

    let target = match args.target {
        InstallTargetCli::Codex => InstallTargetSelector::One(InstallTarget::Codex),
        InstallTargetCli::Gemini => InstallTargetSelector::One(InstallTarget::Gemini),
        InstallTargetCli::Claude => InstallTargetSelector::One(InstallTarget::Claude),
        InstallTargetCli::Cursor => InstallTargetSelector::One(InstallTarget::Cursor),
        InstallTargetCli::All => InstallTargetSelector::All,
    };

    let requests = match args.preset {
        InstallPresetCli::Workspace => vec![InstallRequest {
            target,
            scope: resolve_scope(args.scope, InstallScope::Project)?,
            preset: InstallPreset::Workspace,
            name: name.unwrap_or_else(|| default_server_name(InstallPreset::Workspace).to_owned()),
            project_dir: project_dir.clone(),
            source_dir,
            onec_url: args.onec_url,
            onec_user: args.onec_user,
            onec_password: password,
            env,
            force: args.force,
            dry_run: args.dry_run,
        }],
        InstallPresetCli::Reference => vec![InstallRequest {
            target,
            scope: resolve_scope(args.scope, InstallScope::User)?,
            preset: InstallPreset::Reference,
            name: name.unwrap_or_else(|| default_server_name(InstallPreset::Reference).to_owned()),
            project_dir: project_dir.clone(),
            source_dir,
            onec_url: args.onec_url,
            onec_user: args.onec_user,
            onec_password: password,
            env,
            force: args.force,
            dry_run: args.dry_run,
        }],
        InstallPresetCli::Recommended => {
            if args.scope.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--scope is not used with '--preset recommended'; it installs 'reference:user' and 'workspace:project'",
                )
                .into());
            }

            let (reference_name, workspace_name) = resolve_recommended_names(name.as_deref())?;

            vec![
                InstallRequest {
                    target,
                    scope: InstallScope::User,
                    preset: InstallPreset::Reference,
                    name: reference_name,
                    project_dir: project_dir.clone(),
                    source_dir: source_dir.clone(),
                    onec_url: args.onec_url.clone(),
                    onec_user: args.onec_user.clone(),
                    onec_password: password.clone(),
                    env: env.clone(),
                    force: args.force,
                    dry_run: args.dry_run,
                },
                InstallRequest {
                    target,
                    scope: InstallScope::Project,
                    preset: InstallPreset::Workspace,
                    name: workspace_name,
                    project_dir,
                    source_dir,
                    onec_url: args.onec_url,
                    onec_user: args.onec_user,
                    onec_password: password,
                    env,
                    force: args.force,
                    dry_run: args.dry_run,
                },
            ]
        }
    };

    let result = match mcp_install::install_many(requests) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("{}", format_install_error(&err));
            std::process::exit(2);
        }
    };

    for entry in result.entries {
        println!(
            "[{}:{}] {} -> {}",
            entry.target,
            entry.scope,
            status_label(entry.status),
            entry.location
        );
        if !entry.detail.is_empty() {
            println!("{}", entry.detail);
        }
    }

    Ok(())
}

fn run_mcp_server(
    profile: mcp_server::McpProfile,
    source_dir: Option<PathBuf>,
    onec_url: Option<String>,
    onec_user: &str,
    onec_password: &str,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!(?profile, ?source_dir, ?onec_url, "Starting MCP server (stdio)");

    let server = build_server(profile, source_dir, onec_url, onec_user, onec_password)?;
    let shutdown_guard = server.clone();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let serve_result = rt.block_on(mcp_server::serve_stdio(server));

    drop(rt);
    shutdown_guard.shutdown();
    drop(shutdown_guard);

    serve_result?;
    Ok(())
}

/// Build the MCP server (resident state + tool router) for a profile. Shared by the
/// stdio path and the broker backend so both construct identical state.
fn build_server(
    profile: mcp_server::McpProfile,
    source_dir: Option<PathBuf>,
    onec_url: Option<String>,
    onec_user: &str,
    onec_password: &str,
) -> Result<mcp_server::McpServer, Box<dyn Error + Send + Sync>> {
    let state = match profile {
        mcp_server::McpProfile::Workspace => {
            let source_dir = source_dir.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "workspace profile requires --source-dir",
                )
            })?;
            let source_dir = source_dir.canonicalize().unwrap_or(source_dir);
            let mut state = mcp_server::SharedState::workspace(source_dir)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
            if let Some(ref url) = onec_url {
                tracing::info!(%url, "Configuring 1C HTTP client");
                state.set_onec_client(onec_client::Client::new(url, onec_user, onec_password));
            }
            state.warm_start();
            state
        }
        mcp_server::McpProfile::Reference => mcp_server::SharedState::reference(source_dir),
    };

    Ok(mcp_server::McpServer::new(profile, state))
}

fn resolve_scope(
    scope: Option<InstallScopeCli>,
    default: bsl_analyzer::mcp_install::InstallScope,
) -> Result<bsl_analyzer::mcp_install::InstallScope, io::Error> {
    Ok(match scope {
        Some(InstallScopeCli::User) => bsl_analyzer::mcp_install::InstallScope::User,
        Some(InstallScopeCli::Project) => bsl_analyzer::mcp_install::InstallScope::Project,
        Some(InstallScopeCli::Local) => bsl_analyzer::mcp_install::InstallScope::Local,
        None => default,
    })
}

fn resolve_recommended_names(name: Option<&str>) -> Result<(String, String), io::Error> {
    match name {
        None => Ok((
            bsl_analyzer::mcp_install::default_server_name(
                bsl_analyzer::mcp_install::InstallPreset::Reference,
            )
            .to_owned(),
            bsl_analyzer::mcp_install::default_server_name(
                bsl_analyzer::mcp_install::InstallPreset::Workspace,
            )
            .to_owned(),
        )),
        Some(base) => {
            let base = base.trim();
            if base.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "--name must not be empty",
                ));
            }
            Ok((format!("{base}-reference"), format!("{base}-workspace")))
        }
    }
}

fn format_install_error(err: &bsl_analyzer::mcp_install::InstallError) -> String {
    if let Some(hint) = err.hint() {
        format!("{err}\nHint: {hint}")
    } else {
        err.to_string()
    }
}

fn parse_env_pair(input: &str) -> Result<(String, String), String> {
    let Some((key, value)) = input.split_once('=') else {
        return Err("expected KEY=value".to_owned());
    };
    if key.is_empty() {
        return Err("environment variable name must not be empty".to_owned());
    }
    Ok((key.to_owned(), value.to_owned()))
}

fn status_label(status: bsl_analyzer::mcp_install::InstallStatus) -> &'static str {
    match status {
        bsl_analyzer::mcp_install::InstallStatus::Installed => "installed",
        bsl_analyzer::mcp_install::InstallStatus::Updated => "updated",
        bsl_analyzer::mcp_install::InstallStatus::DryRun => "dry-run",
    }
}

fn decode_password(password: &str) -> String {
    if let Some(encoded) = password.strip_prefix("base64:") {
        if let Some(decoded) = base64_decode(encoded) {
            if let Ok(s) = String::from_utf8(decoded) {
                return s;
            }
        }
    }
    password.to_owned()
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;
    for &b in input.as_bytes() {
        if b == b'=' || b == b'\n' || b == b'\r' {
            continue;
        }
        let val = TABLE.iter().position(|&c| c == b)? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_serve_mode_with_override, McpCommand, McpProfileCli, McpServeArgs, McpServeMode,
        ServeModeContext,
    };

    #[test]
    fn workspace_profile_defaults_to_broker() {
        // Production passes `platform_default_broker: true` on every platform, so the
        // workspace profile with no override resolves to the broker everywhere.
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Stdio,
            mcp_server::McpProfile::Workspace,
            ServeModeContext { broker_override: None, platform_default_broker: true },
        );

        assert!(matches!(mode, McpServeMode::Broker));
    }

    #[test]
    fn workspace_profile_defaults_to_stdio_when_platform_default_is_off() {
        // The platform default is an injected seam; with it off, the workspace profile
        // stays on stdio. No production platform sets this today, but the precedence
        // must remain correct if one ever does.
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Stdio,
            mcp_server::McpProfile::Workspace,
            ServeModeContext { broker_override: None, platform_default_broker: false },
        );

        assert!(matches!(mode, McpServeMode::Stdio));
    }

    #[test]
    fn broker_env_override_off_forces_stdio_even_when_platform_defaults_to_broker() {
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Stdio,
            mcp_server::McpProfile::Workspace,
            ServeModeContext { broker_override: Some(false), platform_default_broker: true },
        );

        assert!(matches!(mode, McpServeMode::Stdio));
    }

    #[test]
    fn broker_env_override_on_forces_broker_even_when_platform_default_is_off() {
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Stdio,
            mcp_server::McpProfile::Workspace,
            ServeModeContext { broker_override: Some(true), platform_default_broker: false },
        );

        assert!(matches!(mode, McpServeMode::Broker));
    }

    #[test]
    fn reference_profile_never_defaults_to_broker() {
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Stdio,
            mcp_server::McpProfile::Reference,
            ServeModeContext { broker_override: Some(true), platform_default_broker: true },
        );

        assert!(matches!(mode, McpServeMode::Stdio));
    }

    #[test]
    fn explicit_mode_flag_wins_over_defaults() {
        // The re-exec'd daemon passes `--mode daemon`; an explicit flag must survive
        // regardless of profile or platform default.
        let mode = resolve_serve_mode_with_override(
            McpServeMode::Daemon,
            mcp_server::McpProfile::Workspace,
            ServeModeContext { broker_override: Some(false), platform_default_broker: false },
        );

        assert!(matches!(mode, McpServeMode::Daemon));
    }

    fn serve_command(mode: McpServeMode, source_dir: &std::path::Path) -> McpCommand {
        McpCommand::Serve(McpServeArgs {
            runtime_profile: McpProfileCli::Workspace,
            source_dir: Some(source_dir.to_path_buf()),
            mode,
            onec_url: None,
            onec_user: String::new(),
            onec_password: String::new(),
        })
    }

    #[test]
    fn daemon_log_requires_explicit_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        let cmd = serve_command(McpServeMode::Daemon, dir.path());
        for off in [None, Some(""), Some("0"), Some("false"), Some("off")] {
            assert!(cmd.daemon_log_file_for(off).is_none(), "{off:?} must keep stderr");
        }
    }

    #[test]
    fn daemon_log_opt_in_lands_in_build_dir_and_rotates_only_by_size() {
        let dir = tempfile::tempdir().unwrap();

        let path = serve_command(McpServeMode::Daemon, dir.path())
            .daemon_log_file_for(Some("1"))
            .expect("opt-in enables the default log file");
        assert_eq!(path, dir.path().join(".build/bsl-analyzer-daemon.log"));
        assert!(path.parent().unwrap().is_dir(), "`.build` is created eagerly");

        // A small live log is left in place: a concurrent daemon candidate must
        // not rename the winner's file out from under it.
        std::fs::write(&path, "live winner log").unwrap();
        let again = serve_command(McpServeMode::Daemon, dir.path())
            .daemon_log_file_for(Some("true"))
            .unwrap();
        assert_eq!(again, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "live winner log");
        let prev = dir.path().join(".build/bsl-analyzer-daemon.log.prev");
        assert!(!prev.exists());

        // An oversized log rotates to `.prev` (sparse file keeps the test cheap).
        std::fs::File::create(&path).unwrap().set_len(51 * 1024 * 1024).unwrap();
        serve_command(McpServeMode::Daemon, dir.path()).daemon_log_file_for(Some("1")).unwrap();
        assert!(prev.exists());
        assert!(!path.exists());
    }

    #[test]
    fn daemon_log_opt_in_accepts_an_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("custom.log");
        let path = serve_command(McpServeMode::Daemon, dir.path())
            .daemon_log_file_for(Some(custom.to_str().unwrap()))
            .unwrap();
        assert_eq!(path, custom);
    }

    #[test]
    fn daemon_log_explicit_path_prepares_missing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("nested/dirs/daemon.log");
        let path = serve_command(McpServeMode::Daemon, dir.path())
            .daemon_log_file_for(Some(custom.to_str().unwrap()))
            .expect("missing parent directories are created, not fatal");
        assert_eq!(path, custom);
        assert!(path.parent().unwrap().is_dir());
    }

    #[test]
    fn daemon_log_rotation_appends_prev_to_dotted_names() {
        let dir = tempfile::tempdir().unwrap();
        let custom = dir.path().join("daemon.custom.log");
        std::fs::File::create(&custom).unwrap().set_len(51 * 1024 * 1024).unwrap();
        serve_command(McpServeMode::Daemon, dir.path())
            .daemon_log_file_for(Some(custom.to_str().unwrap()))
            .unwrap();
        assert!(dir.path().join("daemon.custom.log.prev").exists());
        assert!(!custom.exists());
    }

    #[test]
    fn stdio_and_broker_modes_keep_stderr_logging() {
        let dir = tempfile::tempdir().unwrap();
        for mode in [McpServeMode::Stdio, McpServeMode::Broker] {
            assert!(serve_command(mode, dir.path()).daemon_log_file_for(Some("1")).is_none());
        }
    }
}
