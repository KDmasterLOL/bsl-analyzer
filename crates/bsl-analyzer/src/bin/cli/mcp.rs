use std::{collections::BTreeMap, env, error::Error, io, path::PathBuf};

use clap::{Args, Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum McpCommand {
    /// Start MCP server (Model Context Protocol for AI agents)
    Serve(McpServeArgs),

    /// Install MCP config into supported AI tools
    Install(McpInstallArgs),
}

#[derive(Args, Clone)]
pub struct McpServeArgs {
    /// Runtime profile
    #[arg(long = "profile", value_enum)]
    runtime_profile: McpProfileCli,

    /// Source directory containing 1C configuration (with Configuration.xml)
    #[arg(short = 's', long = "source-dir", required_if_eq("runtime_profile", "workspace"))]
    source_dir: Option<PathBuf>,

    /// URL of 1C HTTP service for live database queries (e.g., http://localhost/base/hs/bsl-analyzer)
    #[arg(long)]
    onec_url: Option<String>,

    /// 1C username for HTTP service authentication
    #[arg(long, default_value = "")]
    onec_user: String,

    /// 1C password for HTTP service authentication.
    /// Supports base64 encoding: prefix with "base64:" (e.g. "base64:cGFzc3dvcmQ=")
    #[arg(long, default_value = "")]
    onec_password: String,
}

#[derive(Args)]
pub struct McpInstallArgs {
    /// Target application: codex, gemini, claude, cursor or all
    #[arg(long, value_enum)]
    target: InstallTargetCli,

    /// Configuration scope
    #[arg(long, value_enum)]
    scope: Option<InstallScopeCli>,

    /// Installation preset or recommended bundle
    #[arg(long, value_enum, default_value_t = InstallPresetCli::Workspace)]
    preset: InstallPresetCli,

    /// MCP server name inside target config
    #[arg(long)]
    name: Option<String>,

    /// Source directory containing 1C configuration (with Configuration.xml)
    #[arg(short = 's', long = "source-dir")]
    source_dir: Option<PathBuf>,

    /// URL of 1C HTTP service for live database queries (e.g., http://localhost/base/hs/bsl-analyzer)
    #[arg(long)]
    onec_url: Option<String>,

    /// 1C username for HTTP service authentication
    #[arg(long, default_value = "")]
    onec_user: String,

    /// 1C password for HTTP service authentication.
    /// Supports base64 encoding: prefix with "base64:" (e.g. "base64:cGFzc3dvcmQ=")
    #[arg(long, default_value = "")]
    onec_password: String,

    /// Extra environment variable for the spawned MCP server (repeatable KEY=value)
    #[arg(long = "env", value_parser = parse_env_pair)]
    env: Vec<(String, String)>,

    /// Replace an existing MCP entry with the same name
    #[arg(long)]
    force: bool,

    /// Print the generated command or config instead of writing it
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
    let password = decode_password(&args.onec_password);
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

    run_mcp_server(profile, args.source_dir, args.onec_url, &args.onec_user, &password)
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

    let server = mcp_server::McpServer::new(profile, state);
    let shutdown_guard = server.clone();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let serve_result = rt.block_on(mcp_server::serve_stdio(server));

    // Keep one clone alive until after the Tokio runtime is dropped so sync
    // PostgreSQL baseline clients/pools are destroyed outside the active
    // runtime context and avoid nested-runtime panics during MCP stdio shutdown.
    drop(rt);
    shutdown_guard.shutdown();
    drop(shutdown_guard);

    serve_result?;
    Ok(())
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

/// Decode password: if prefixed with `base64:`, decode the suffix; otherwise
/// return as-is. Not encryption — just obfuscation for `.mcp.json`.
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
