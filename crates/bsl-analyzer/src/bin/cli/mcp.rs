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
    /// — so many clients/reviews reuse one heavy process. `daemon` *is* that backend
    /// and is launched internally by a broker proxy; it is not meant to be run
    /// directly.
    #[arg(long = "mode", value_enum, default_value = "stdio")]
    mode: McpServeMode,

    #[arg(long)]
    onec_url: Option<String>,

    #[arg(long, default_value = "")]
    onec_user: String,

    #[arg(long, default_value = "")]
    onec_password: String,
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

    match resolve_serve_mode(args.mode) {
        McpServeMode::Stdio => {
            run_mcp_server(profile, args.source_dir, args.onec_url, &args.onec_user, &password)
        }
        McpServeMode::Broker => run_mcp_broker(profile, &args),
        McpServeMode::Daemon => {
            run_mcp_daemon(profile, args.source_dir, args.onec_url, &args.onec_user, &password)
        }
    }
}

/// Resolve the effective serve mode. An explicit `--mode` always wins; otherwise the
/// `BSL_MCP_BROKER` env var promotes the default (stdio) to broker. The env path
/// exists because some clients reconstruct the server argv and drop extra flags when
/// importing `.mcp.json` (Codex does this), but they DO propagate the `env` block —
/// so an env switch is the only activation that reaches every client. The re-exec'd
/// daemon is launched with explicit `--mode daemon`, which takes precedence here, so
/// inheriting the env var cannot turn it back into a proxy.
fn resolve_serve_mode(flag: McpServeMode) -> McpServeMode {
    match flag {
        McpServeMode::Stdio if env_flag_enabled("BSL_MCP_BROKER") => McpServeMode::Broker,
        other => other,
    }
}

fn env_flag_enabled(key: &str) -> bool {
    env::var(key)
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

/// The broker proxy: connect to (or launch) the shared per-project backend and relay
/// stdio to it. The backend is this same binary re-executed with `--mode daemon` and
/// the same launch parameters, so it resolves to the same backend identity.
fn run_mcp_broker(
    profile: mcp_server::McpProfile,
    args: &McpServeArgs,
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
    result?;
    Ok(())
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
    let result = rt.block_on(mcp_server::broker::daemon::run(build, key, broker_idle_timeout()));
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

/// Idle window after which a backend with no live connections exits. While any client
/// is open its proxy holds the connection, which keeps the backend alive regardless;
/// this window only governs how long to stay warm *after the last client disconnects*,
/// so a quick reopen reuses the backend. Default 120s; override via `BSL_MCP_IDLE_SECS`.
fn broker_idle_timeout() -> Duration {
    let secs = env::var("BSL_MCP_IDLE_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
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
            let mut state = mcp_server::SharedState::workspace(source_dir);
            if let Some(ref url) = onec_url {
                tracing::info!(%url, "Configuring 1C HTTP client");
                state.set_onec_client(onec_client::Client::new(url, onec_user, onec_password));
            }
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
