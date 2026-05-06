//! BSL Analyzer - Language Server for 1C:Enterprise BSL.
//!
//! This is the main entry point for the LSP server.

#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use std::{
    collections::BTreeMap,
    env,
    error::Error,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};

use clap::{Args, Parser, Subcommand, ValueEnum};
use ide_db::metadata;

/// Output format for analysis results.
#[derive(Debug, Clone, Default, ValueEnum)]
enum OutputFormat {
    /// Console output with reporters (default)
    #[default]
    Console,
    /// JSON Lines streaming output (for SonarQube integration)
    Jsonl,
}

#[derive(Parser)]
#[command(name = "bsl-analyzer")]
#[command(version)]
#[command(about = "BSL Language Server and Analyzer")]
struct Cli {
    /// Run in LSP server mode via stdio (same as no arguments)
    #[arg(long)]
    stdio: bool,

    /// Enable hierarchical profiling (filter syntax: pattern@depth>threshold_ms)
    /// Example: '*>10' profiles all operations taking >10ms
    #[arg(long = "trace-profile", global = true)]
    profile: Option<String>,

    /// Enable JSON profiling output (filter syntax: pattern)
    /// Example: '*' outputs timing for all spans as JSON to stderr
    #[arg(long = "trace-profile-json", global = true)]
    profile_json: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)] // CLI command enum, boxing not appropriate
enum Commands {
    /// Run static analysis on a project
    Analyze {
        /// Source directory containing BSL files (default: current directory)
        /// Aliases: --srcDir, --src, --project
        #[arg(
            short = 's',
            long = "source-dir",
            alias = "srcDir",
            alias = "src",
            alias = "project",
            default_value = "."
        )]
        source_dir: PathBuf,

        /// Workspace directory for relative paths in reports (default: source directory)
        /// Alias: --workspaceDir
        #[arg(short = 'w', long = "workspace-dir", alias = "workspaceDir")]
        workspace_dir: Option<PathBuf>,

        /// Output directory for analysis reports (default: current directory)
        /// Alias: --outputDir
        #[arg(short = 'o', long = "output-dir", alias = "outputDir")]
        output_dir: Option<PathBuf>,

        /// Configuration file path (Alias: --configuration)
        #[arg(short = 'c', long = "config", alias = "configuration")]
        config: Option<PathBuf>,

        /// Reporters to use (can be specified multiple times or comma-separated)
        /// Valid values: console, json, sarif, tslint, junit, generic, code-quality
        /// (Alias: --reporter)
        #[arg(short = 'r', long = "reporters", alias = "reporter", value_delimiter = ',')]
        reporters: Vec<String>,

        /// Silent mode - disable progress bar (Alias: --silent)
        #[arg(short = 'q', long = "quiet", alias = "silent")]
        quiet: bool,

        /// Enable incremental analysis (only analyze affected modules)
        #[arg(long)]
        incremental: bool,

        /// Comma-separated list of changed files (for incremental mode)
        #[arg(long, value_delimiter = ',', requires = "incremental")]
        changed_files: Option<Vec<PathBuf>>,

        /// Git ref to compare against (e.g., HEAD~1, origin/main) for incremental mode
        #[arg(long, requires = "incremental", conflicts_with = "changed_files")]
        git_diff: Option<String>,

        /// [deprecated] Streaming is now the default for CLI, this flag is ignored
        #[arg(long, hide = true)]
        streaming: bool,

        /// Use Salsa mode instead of streaming (for testing/debugging only)
        #[arg(long, hide = true)]
        salsa: bool,

        /// Number of worker threads (default: CPU cores)
        #[arg(long)]
        workers: Option<usize>,

        /// Output format
        /// jsonl: JSON Lines streaming output (for SonarQube integration)
        #[arg(long, value_enum, default_value_t = OutputFormat::Console)]
        format: OutputFormat,

        /// Run only this diagnostic (enables profiling output)
        /// Example: --only-diagnostic IncorrectUseOfStrTemplate
        #[arg(long)]
        only_diagnostic: Option<String>,

        /// JSON file with diff information for filtering diagnostics.
        /// Only diagnostics within changed line ranges will be reported.
        /// Format: {"base_ref": "...", "head_ref": "...", "files": {"path": {"hunks": [[start, end], ...]}}}
        #[arg(long)]
        diff_filter: Option<PathBuf>,
    },

    /// Check configuration file
    CheckConfig {
        /// Path to configuration file
        #[arg(short, long)]
        config: std::path::PathBuf,
    },

    /// Format a BSL file
    Format {
        /// Path to the BSL file to format
        file: PathBuf,

        /// Write formatted output back to file (default: print to stdout)
        #[arg(short = 'w', long)]
        write: bool,

        /// Use spaces instead of tabs (default: tabs)
        #[arg(long)]
        spaces: bool,

        /// Number of spaces per indent level (default: 4, only with --spaces)
        #[arg(long, default_value = "4")]
        indent_size: u32,
    },

    /// Start LSP server (default)
    Lsp,

    /// Start MCP server or install MCP config into AI tools
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },

    /// Export built-in 1C extension (BSL_Analyzer) to a directory
    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },

    /// Start DAP debug adapter (Debug Adapter Protocol via stdio)
    Dap,

    /// Search infrastructure commands
    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },

    /// Export diagnostic rules metadata
    Rules {
        #[command(subcommand)]
        command: RulesCommands,
    },
}

#[derive(Subcommand)]
enum McpCommand {
    /// Start MCP server (Model Context Protocol for AI agents)
    Serve(McpServeArgs),

    /// Install MCP config into supported AI tools
    Install(McpInstallArgs),
}

#[derive(Subcommand)]
enum SearchCommand {
    /// Baseline storage operations for centralized search
    Baseline {
        #[command(subcommand)]
        command: SearchBaselineCommand,
    },
}

#[derive(Subcommand)]
enum SearchBaselineCommand {
    /// Build a local code snapshot and publish it into centralized PostgreSQL storage
    Publish(SearchBaselinePublishArgs),

    /// Inspect centralized PostgreSQL baseline storage
    Inspect {
        #[command(subcommand)]
        command: SearchBaselineInspectCommand,
    },

    /// Run administrative PostgreSQL baseline storage operations
    Admin {
        #[command(subcommand)]
        command: SearchBaselineAdminCommand,
    },
}

#[derive(Subcommand)]
enum SearchBaselineInspectCommand {
    /// List published snapshots from centralized PostgreSQL storage
    ListSnapshots(SearchBaselineListSnapshotsArgs),

    /// Show one published snapshot from centralized PostgreSQL storage
    ShowSnapshot(SearchBaselineShowSnapshotArgs),

    /// List shared file objects stored in centralized PostgreSQL storage
    ListFileObjects(SearchBaselineListFileObjectsArgs),

    /// Show one shared file object from centralized PostgreSQL storage
    ShowFileObject(SearchBaselineShowFileObjectArgs),

    /// List embedding inventories stored in centralized PostgreSQL storage
    ListEmbeddings(SearchBaselineListEmbeddingsArgs),

    /// Show active embedding coverage for centralized PostgreSQL storage
    ShowEmbeddingCoverage(SearchBaselineShowEmbeddingCoverageArgs),

    /// Analyze snapshot retention policy for centralized PostgreSQL storage
    Retention(SearchBaselineRetentionArgs),
}

#[derive(Subcommand)]
enum SearchBaselineAdminCommand {
    /// Initialize or migrate PostgreSQL baseline storage schema
    Migrate(SearchBaselineAdminMigrateArgs),

    /// Garbage-collect unreferenced shared baseline objects in PostgreSQL storage
    Gc(SearchBaselineAdminGcArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SearchBaselineCorpusCli {
    WorkspaceCode,
    Reference,
}

#[derive(Args, Clone)]
struct SearchBaselinePublishArgs {
    /// Corpus to publish into centralized baseline storage
    #[arg(long, value_enum, default_value_t = SearchBaselineCorpusCli::WorkspaceCode)]
    corpus: SearchBaselineCorpusCli,

    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Immutable snapshot identifier. Optional when --branch/--commit are provided.
    #[arg(long = "snapshot-id")]
    snapshot_id: Option<String>,

    /// Optional branch name associated with the snapshot.
    /// Falls back to CI_COMMIT_BRANCH or CI_COMMIT_REF_NAME.
    #[arg(long)]
    branch: Option<String>,

    /// Optional commit SHA associated with the snapshot.
    /// Falls back to CI_COMMIT_SHA or GITHUB_SHA.
    #[arg(long)]
    commit: Option<String>,

    /// Optional parent snapshot identifier for lineage tracking.
    /// When omitted, the CLI reuses the latest published snapshot in the same
    /// corpus and branch, or the same corpus when branch is not specified.
    #[arg(long = "parent-snapshot-id")]
    parent_snapshot_id: Option<String>,

    /// Allow publishing a workspace branch that is not listed in branch policy.
    #[arg(long = "allow-non-policy-branch")]
    allow_non_policy_branch: bool,
}

#[derive(Args, Clone)]
struct SearchBaselineListSnapshotsArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Optional corpus filter
    #[arg(long, value_enum)]
    corpus: Option<SearchBaselineCorpusCli>,

    /// Optional branch filter
    #[arg(long)]
    branch: Option<String>,

    /// Optional commit filter
    #[arg(long)]
    commit: Option<String>,

    /// Maximum number of snapshots to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Args, Clone)]
struct SearchBaselineShowSnapshotArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Snapshot identifier to inspect
    #[arg(long = "snapshot-id")]
    snapshot_id: String,
}

#[derive(Args, Clone)]
struct SearchBaselineListFileObjectsArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Optional collection filter
    #[arg(long)]
    collection: Option<String>,

    /// Maximum number of file objects to return
    #[arg(long, default_value_t = 20)]
    limit: usize,
}

#[derive(Args, Clone)]
struct SearchBaselineShowFileObjectArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// File object identifier to inspect
    #[arg(long = "file-object-id")]
    file_object_id: String,
}

#[derive(Args, Clone)]
struct SearchBaselineListEmbeddingsArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Optional embedding model filter
    #[arg(long = "model")]
    model_id: Option<String>,

    /// Optional embedding dimension filter
    #[arg(long)]
    dimension: Option<usize>,
}

#[derive(Args, Clone)]
struct SearchBaselineShowEmbeddingCoverageArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Optional embedding model filter
    #[arg(long = "model")]
    model_id: Option<String>,

    /// Optional embedding dimension filter
    #[arg(long)]
    dimension: Option<usize>,
}

#[derive(Args, Clone)]
struct SearchBaselineAdminGcArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Apply deletions. Without this flag the command reports a dry-run summary.
    #[arg(long)]
    execute: bool,
}

#[derive(Args, Clone)]
struct SearchBaselineRetentionArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,

    /// Optional branch filter
    #[arg(long)]
    branch: Option<String>,

    /// Maximum number of snapshots to inspect
    #[arg(long, default_value_t = 200)]
    limit: usize,
}

#[derive(Args, Clone)]
struct SearchBaselineAdminMigrateArgs {
    /// Project root containing bsl-analyzer.toml
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    source_dir: PathBuf,
}

#[derive(Args, Clone)]
struct McpServeArgs {
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
struct McpInstallArgs {
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
enum InstallTargetCli {
    Codex,
    Gemini,
    Claude,
    Cursor,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InstallScopeCli {
    User,
    Project,
    Local,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum InstallPresetCli {
    #[default]
    Workspace,
    Reference,
    Recommended,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum McpProfileCli {
    Workspace,
    Reference,
}

#[derive(Subcommand)]
enum ExtensionCommands {
    /// Export extension XML files to a directory for loading into 1C infobase
    Export {
        /// Output directory (will be created if it doesn't exist)
        #[arg(short, long)]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum RulesCommands {
    /// Export rules in SonarQube-compatible format
    Export {
        /// Output format
        #[arg(long, value_enum, default_value_t = RulesFormat::Sonarqube)]
        format: RulesFormat,

        /// Language for descriptions (ru, en)
        #[arg(long, default_value = "ru")]
        lang: String,

        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// List all available diagnostic codes
    List,
}

#[derive(Debug, Clone, Default, ValueEnum)]
enum RulesFormat {
    /// SonarQube plugin format (JSON)
    #[default]
    Sonarqube,
    /// Simple JSON format
    Json,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let cli = Cli::parse();

    let log_file = env::var("BSL_LOG_FILE").ok().map(PathBuf::from);

    // Setup logging with CLI options (CLI args override env vars)
    let profile_filter = cli.profile.clone().or_else(|| env::var("BSL_PROFILE").ok());
    let json_profile_filter =
        cli.profile_json.clone().or_else(|| env::var("BSL_PROFILE_JSON").ok());

    if let Err(e) = setup_logging(log_file.clone(), profile_filter, json_profile_filter) {
        // If logging setup fails, write to stderr as fallback
        eprintln!("Failed to setup logging: {}", e);
        // Try to write to log file directly
        if let Some(ref path) = log_file {
            let _ = fs::write(path, format!("ERROR: Failed to setup logging: {}\n", e));
        }
        return Err(e.into());
    }

    tracing::info!("BSL Analyzer starting (pid: {})", std::process::id());
    tracing::info!("Working directory: {:?}", env::current_dir().ok());
    tracing::info!("Command line args: {:?}", env::args().collect::<Vec<_>>());

    // --stdio flag is same as running without arguments (LSP mode)
    if cli.stdio && cli.command.is_some() {
        tracing::error!("Cannot use --stdio with other commands");
        eprintln!("Error: --stdio flag cannot be used with other subcommands");
        std::process::exit(1);
    }

    match cli.command {
        Some(Commands::Analyze {
            source_dir,
            workspace_dir,
            output_dir,
            config,
            reporters,
            quiet,
            incremental,
            changed_files,
            git_diff,
            streaming: _, // deprecated, ignored
            salsa,
            workers,
            format,
            only_diagnostic,
            diff_filter,
        }) => analyze(
            source_dir,
            workspace_dir,
            output_dir,
            config,
            reporters,
            quiet,
            incremental,
            changed_files,
            git_diff,
            salsa,
            workers,
            format,
            only_diagnostic,
            diff_filter,
        ),
        Some(Commands::CheckConfig { config }) => check_config(config),
        Some(Commands::Format { file, write, spaces, indent_size }) => {
            run_format(file, write, spaces, indent_size)
        }
        Some(Commands::Mcp { command }) => run_mcp_command(command),
        Some(Commands::Extension { command }) => run_extension_command(command),
        Some(Commands::Dap) => run_dap_server(),
        Some(Commands::Search { command }) => run_search_command(command),
        Some(Commands::Rules { command }) => run_rules_command(command),
        Some(Commands::Lsp) | None => run_lsp_server(),
    }
}

fn run_mcp_command(command: McpCommand) -> Result<(), Box<dyn Error + Send + Sync>> {
    match command {
        McpCommand::Serve(args) => run_mcp_serve(args),
        McpCommand::Install(args) => run_mcp_install(args),
    }
}

fn run_search_command(command: SearchCommand) -> Result<(), Box<dyn Error + Send + Sync>> {
    match command {
        SearchCommand::Baseline { command } => match command {
            SearchBaselineCommand::Publish(args) => run_search_baseline_publish(args),
            SearchBaselineCommand::Inspect { command } => match command {
                SearchBaselineInspectCommand::ListSnapshots(args) => {
                    run_search_baseline_list_snapshots(args)
                }
                SearchBaselineInspectCommand::ShowSnapshot(args) => {
                    run_search_baseline_show_snapshot(args)
                }
                SearchBaselineInspectCommand::ListFileObjects(args) => {
                    run_search_baseline_list_file_objects(args)
                }
                SearchBaselineInspectCommand::ShowFileObject(args) => {
                    run_search_baseline_show_file_object(args)
                }
                SearchBaselineInspectCommand::ListEmbeddings(args) => {
                    run_search_baseline_list_embeddings(args)
                }
                SearchBaselineInspectCommand::ShowEmbeddingCoverage(args) => {
                    run_search_baseline_show_embedding_coverage(args)
                }
                SearchBaselineInspectCommand::Retention(args) => {
                    run_search_baseline_retention(args)
                }
            },
            SearchBaselineCommand::Admin { command } => match command {
                SearchBaselineAdminCommand::Migrate(args) => {
                    run_search_baseline_admin_migrate(args)
                }
                SearchBaselineAdminCommand::Gc(args) => run_search_baseline_admin_gc(args),
            },
        },
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

fn run_search_baseline_publish(
    args: SearchBaselinePublishArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_search::{
        fingerprint_documents, fingerprint_indexed_documents, BaselinePublisher, CorpusId,
        Embedder, EmbeddingProgress, SemanticPublishPhase, SemanticPublishProgress, Snapshot,
        SnapshotPublishMetadata,
    };

    let project = project_model::Project::new(&args.source_dir);
    let source_path = project.source_path().to_path_buf();
    let branch = resolve_publish_branch(args.branch.as_deref(), &project.root);
    let commit = resolve_publish_commit(args.commit.as_deref(), &project.root);
    let corpus = match args.corpus {
        SearchBaselineCorpusCli::WorkspaceCode => CorpusId::WorkspaceCode,
        SearchBaselineCorpusCli::Reference => CorpusId::Reference,
    };
    if matches!(corpus, CorpusId::WorkspaceCode) {
        validate_workspace_publish_policy(
            &project.config.search.baseline.workspace_code.policy,
            branch.as_deref(),
            args.allow_non_policy_branch,
        )?;
    }
    let snapshot_id = resolve_snapshot_id(
        &corpus,
        args.snapshot_id.as_deref(),
        branch.as_deref(),
        commit.as_deref(),
    )?;
    let resolved_pg = resolve_project_postgres_url(
        &project.config.search.baseline.postgres,
        project_model::PostgresAccessMode::Writer,
    )
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to resolve PostgreSQL writer credentials: {error}"),
        )
    })?;

    eprintln!("[1/5] Indexing source files...");
    let (indexed_files, documents) = match corpus {
        CorpusId::WorkspaceCode => build_workspace_code_baseline_documents(&source_path)?,
        CorpusId::Reference => build_reference_baseline_documents()?,
        CorpusId::Custom(_) => unreachable!("CLI corpus variants are exhaustive"),
    };
    eprintln!("      {} files -> {} chunks", indexed_files, documents.len());

    if documents.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no documents were indexed for corpus {}", corpus.as_str()),
        )
        .into());
    }

    let adapter = build_postgres_baseline_adapter(
        &resolved_pg.url,
        project.config.search.baseline.postgres.schema.as_deref(),
    )?;
    let schema_label = project
        .config
        .search
        .baseline
        .postgres
        .schema
        .clone()
        .unwrap_or_else(|| "bsl_search".to_owned());

    eprintln!("[2/5] Connecting to PostgreSQL (schema: {})...", schema_label);
    let fingerprint = match corpus {
        CorpusId::WorkspaceCode => fingerprint_indexed_documents(&documents),
        CorpusId::Reference => {
            let reference_documents = build_reference_source_documents();
            fingerprint_documents(&reference_documents)
        }
        CorpusId::Custom(_) => unreachable!("CLI corpus variants are exhaustive"),
    };
    let mut snapshot = Snapshot::new(snapshot_id, corpus.clone()).with_fingerprint(fingerprint);
    if let Some(parent_snapshot_id) = resolve_parent_snapshot_id(
        &adapter,
        &corpus,
        branch.as_deref(),
        &snapshot.id.0,
        args.parent_snapshot_id.as_deref(),
        Some(&project.config.search.baseline.workspace_code.policy),
    )? {
        snapshot = snapshot.with_parent(parent_snapshot_id);
    }
    let publish_metadata =
        SnapshotPublishMetadata { branch: branch.clone(), commit: commit.clone() };

    eprintln!("[3/5] Publishing snapshot ({} chunks)...", documents.len());
    let embedder = embedding_config_from_env().map(Embedder::new);
    let has_embedder = embedder.is_some();
    let embedding_progress = |event: EmbeddingProgress| match event {
        EmbeddingProgress::Plan { total_unique, cached, to_compute } => {
            eprintln!(
                "[4/5] Computing embeddings ({} unique, {} cached, {} to compute)...",
                total_unique, cached, to_compute
            );
        }
        EmbeddingProgress::Batch { processed, total, batches_done, total_batches } => {
            let pct = (processed * 100).checked_div(total).unwrap_or(100);
            eprintln!(
                "      {}/{} ({}%) — batch {}/{}",
                processed, total, pct, batches_done, total_batches
            );
        }
    };
    let publish_report = BaselinePublisher::new(embedding_execution_policy_from_env()).publish(
        &adapter,
        &snapshot,
        &publish_metadata,
        &documents,
        embedder.as_ref(),
        if has_embedder { Some(&embedding_progress) } else { None },
    )?;
    eprintln!(
        "      Reused {} files, wrote {}, deleted {}",
        publish_report.snapshot.reused_files,
        publish_report.snapshot.written_files,
        publish_report.snapshot.deleted_files,
    );

    if let Some(ref embedding_stats) = publish_report.embeddings {
        let format_duration = |duration: std::time::Duration| {
            if duration.as_secs() >= 1 {
                format!("{:.1}s", duration.as_secs_f64())
            } else {
                format!("{}ms", duration.as_millis())
            }
        };
        let semantic_phase_label = |phase: &SemanticPublishPhase| match phase {
            SemanticPublishPhase::PrepareRows => "Prepare semantic rows",
            SemanticPublishPhase::CopyParentRows => "Copy unchanged rows from parent",
            SemanticPublishPhase::WriteServingRows => "Write serving rows",
        };
        let semantic_progress = |event: SemanticPublishProgress| match event {
            SemanticPublishProgress::Plan {
                strategy,
                changed_files,
                deleted_paths,
                parent_snapshot_id,
                phase_count,
            } => {
                let parent_label = parent_snapshot_id.as_deref().unwrap_or("-");
                eprintln!(
                    "[5/5] Populating serving semantic index ({strategy}; {changed_files} changed files; {deleted_paths} deletions; {phase_count} phases; parent: {parent_label})..."
                );
            }
            SemanticPublishProgress::PhaseStarted { phase, phase_index, phase_count, detail } => {
                eprintln!(
                    "      [{}/{}] {} — {}",
                    phase_index,
                    phase_count,
                    semantic_phase_label(&phase),
                    detail
                );
            }
            SemanticPublishProgress::PhaseCompleted {
                phase,
                phase_index,
                phase_count,
                elapsed,
                output_rows,
            } => {
                eprintln!(
                    "      [{}/{}] {} done in {} ({} rows)",
                    phase_index,
                    phase_count,
                    semantic_phase_label(&phase),
                    format_duration(elapsed),
                    output_rows
                );
            }
            SemanticPublishProgress::Completed {
                total_rows,
                copied_rows,
                inserted_rows,
                missing_embeddings,
                total_elapsed,
            } => {
                eprintln!(
                    "      Done in {} — total {} rows (copied {}, inserted {}, missing embeddings {})",
                    format_duration(total_elapsed),
                    total_rows,
                    copied_rows,
                    inserted_rows,
                    missing_embeddings
                );
            }
        };
        let serving_count = adapter.populate_serving_semantic_with_progress(
            &snapshot.id.0,
            &embedding_stats.model_id,
            embedding_stats.dimension,
            Some(&semantic_progress),
        )?;
        eprintln!("      {} rows", serving_count);
    } else {
        eprintln!("[4/5] Skipped (no embedding config)");
        eprintln!("[5/5] Skipped (no embeddings)");
    }

    let published_files = documents
        .iter()
        .map(|document| document.path.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let branch_label = branch.as_deref().unwrap_or("-");
    let commit_label = commit.as_deref().unwrap_or("-");

    println!();
    println!("Published search baseline to PostgreSQL.");
    println!("  Corpus:        {}", corpus.as_str());
    println!("  Mode:          {}", if snapshot.parent_id.is_some() { "delta" } else { "root" });
    println!("  Snapshot:      {}", snapshot.id.0);
    println!("  Schema:        {}", schema_label);
    println!(
        "  Parent:        {}",
        snapshot.parent_id.as_ref().map(|value| value.0.as_str()).unwrap_or("-")
    );
    println!("  Branch:        {}", branch_label);
    println!("  Commit:        {}", commit_label);
    println!("  Indexed files: {}", indexed_files);
    println!("  Reused files:  {}", publish_report.snapshot.reused_files);
    println!("  Written files: {}", publish_report.snapshot.written_files);
    println!("  Deleted files: {}", publish_report.snapshot.deleted_files);
    println!("  Files:         {}", published_files);
    println!("  Reused chunks: {}", publish_report.snapshot.reused_documents);
    println!("  Written chunks: {}", publish_report.snapshot.written_documents);
    println!("  Chunks:        {}", documents.len());
    if let Some(ref embedding_stats) = publish_report.embeddings {
        println!("  Embedding model: {}", embedding_stats.model_id);
        println!("  Reused embeddings: {}", embedding_stats.reused);
        println!("  Stored embeddings: {}", embedding_stats.stored);
    }

    Ok(())
}

fn run_search_baseline_list_snapshots(
    args: SearchBaselineListSnapshotsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let corpus = args.corpus.map(search_baseline_corpus_cli_to_domain);
    let snapshots = adapter.list_snapshots(
        corpus.as_ref().map(|corpus| corpus.as_str()),
        args.branch.as_deref(),
        args.commit.as_deref(),
        args.limit,
    )?;

    if snapshots.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    println!("Published search baselines:");
    for snapshot in snapshots {
        println!();
        println!("  Snapshot:  {}", snapshot.snapshot_id);
        println!("  Corpus:    {}", snapshot.corpus);
        println!("  Created:   {}", snapshot.created_at);
        println!("  Parent:    {}", snapshot.parent_snapshot_id.as_deref().unwrap_or("-"));
        println!("  Branch:    {}", snapshot.branch.as_deref().unwrap_or("-"));
        println!("  Commit:    {}", snapshot.commit.as_deref().unwrap_or("-"));
        println!("  Files:     {}", snapshot.files);
        println!("  Chunks:    {}", snapshot.documents);
        println!(
            "  Fingerprint: {}",
            snapshot.fingerprint.as_deref().map(shorten_fingerprint).unwrap_or("-")
        );
    }

    Ok(())
}

fn run_search_baseline_show_snapshot(
    args: SearchBaselineShowSnapshotArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let Some(details) = adapter.snapshot_details(&args.snapshot_id)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("snapshot '{}' was not found", args.snapshot_id),
        )
        .into());
    };

    println!("Search baseline snapshot:");
    println!("  Snapshot:    {}", details.snapshot.snapshot_id);
    println!("  Corpus:      {}", details.snapshot.corpus);
    println!("  Created:     {}", details.snapshot.created_at);
    println!("  Parent:      {}", details.snapshot.parent_snapshot_id.as_deref().unwrap_or("-"));
    println!("  Branch:      {}", details.snapshot.branch.as_deref().unwrap_or("-"));
    println!("  Commit:      {}", details.snapshot.commit.as_deref().unwrap_or("-"));
    println!("  Files:       {}", details.snapshot.files);
    println!("  Chunks:      {}", details.snapshot.documents);
    println!(
        "  Fingerprint: {}",
        details.snapshot.fingerprint.as_deref().map(shorten_fingerprint).unwrap_or("-")
    );

    if details.collections.is_empty() {
        println!("  Collections: -");
    } else {
        println!("  Collections:");
        for collection in details.collections {
            println!(
                "    {}: files={}, chunks={}",
                collection.collection, collection.files, collection.documents
            );
        }
    }

    Ok(())
}

fn run_search_baseline_list_file_objects(
    args: SearchBaselineListFileObjectsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let file_objects = adapter.list_file_objects(args.collection.as_deref(), args.limit)?;

    if file_objects.is_empty() {
        println!("No file objects found.");
        return Ok(());
    }

    println!("Shared baseline file objects:");
    for file_object in file_objects {
        println!();
        println!("  File object:  {}", file_object.file_object_id);
        println!("  Collection:   {}", file_object.collection);
        println!("  Snapshots:    {}", file_object.snapshots);
        println!("  Chunks:       {}", file_object.documents);
        println!("  Fingerprint:  {}", shorten_fingerprint(&file_object.fingerprint));
    }

    Ok(())
}

fn run_search_baseline_show_file_object(
    args: SearchBaselineShowFileObjectArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let Some(details) = adapter.file_object_details(&args.file_object_id)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("file object '{}' was not found", args.file_object_id),
        )
        .into());
    };

    println!("Shared baseline file object:");
    println!("  File object:  {}", details.file_object.file_object_id);
    println!("  Collection:   {}", details.file_object.collection);
    println!("  Snapshots:    {}", details.file_object.snapshots);
    println!("  Chunks:       {}", details.file_object.documents);
    println!("  Fingerprint:  {}", shorten_fingerprint(&details.file_object.fingerprint));
    if details.references.is_empty() {
        println!("  References:   -");
    } else {
        println!("  References:");
        for reference in details.references {
            println!("    {} -> {}", reference.snapshot_id, reference.path);
        }
    }

    Ok(())
}

fn run_search_baseline_list_embeddings(
    args: SearchBaselineListEmbeddingsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let models = adapter.list_embedding_models(args.model_id.as_deref(), args.dimension)?;

    if models.is_empty() {
        println!("No embeddings found.");
        return Ok(());
    }

    println!("Shared embedding inventories:");
    for model in models {
        println!();
        println!("  Model:       {}", model.model_id);
        println!("  Dimension:   {}", model.dimension);
        println!("  Embeddings:  {}", model.embeddings);
    }

    Ok(())
}

fn run_search_baseline_show_embedding_coverage(
    args: SearchBaselineShowEmbeddingCoverageArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let coverage = adapter.embedding_coverage(args.model_id.as_deref(), args.dimension)?;

    if coverage.is_empty() {
        println!("No embedding inventories found.");
        return Ok(());
    }

    println!("Shared embedding coverage:");
    for record in coverage {
        println!();
        println!("  Model:            {}", record.model_id);
        println!("  Dimension:        {}", record.dimension);
        println!("  Active payloads:  {}", record.active_payloads);
        println!("  Covered payloads: {}", record.covered_payloads);
        println!(
            "  Coverage:         {}",
            format_ratio(record.covered_payloads, record.active_payloads)
        );
        println!("  Embeddings:       {}", record.embeddings);
    }

    Ok(())
}

fn run_search_baseline_admin_gc(
    args: SearchBaselineAdminGcArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Migrator,
    )?;
    let report = adapter.garbage_collect(args.execute)?;

    if args.execute {
        println!("Shared baseline garbage collection finished.");
        println!("  Deleted file objects:       {}", report.deleted_file_objects);
        println!("  Deleted file object items:  {}", report.deleted_file_object_items);
        println!("  Deleted semantic rows:      {}", report.deleted_semantic_embeddings);
    } else {
        println!("Shared baseline garbage collection dry-run.");
        println!("  Use --execute to apply deletions.");
    }
    println!("  Orphan file objects:        {}", report.orphan_file_objects);
    println!("  Orphan file object items:   {}", report.orphan_file_object_items);
    println!("  Orphan semantic rows:       {}", report.orphan_semantic_embeddings);

    Ok(())
}

fn run_search_baseline_admin_migrate(
    args: SearchBaselineAdminMigrateArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let project = project_model::Project::new(&args.source_dir);
    let resolved_pg = resolve_project_postgres_url(
        &project.config.search.baseline.postgres,
        project_model::PostgresAccessMode::Migrator,
    )
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to resolve PostgreSQL migrator credentials: {error}"),
        )
    })?;
    let adapter = build_postgres_baseline_adapter(
        &resolved_pg.url,
        project.config.search.baseline.postgres.schema.as_deref(),
    )?;
    let schema_label = project
        .config
        .search
        .baseline
        .postgres
        .schema
        .clone()
        .unwrap_or_else(|| "bsl_search".to_owned());

    adapter.migrate_storage()?;

    println!("PostgreSQL baseline storage is ready.");
    println!("  Schema: {}", schema_label);
    println!("  Role:   migrator");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotRetentionStatus {
    ActiveHead,
    SafetyHead,
    WithinWindow,
    ExpiredCandidate,
}

impl SnapshotRetentionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::ActiveHead => "active-head",
            Self::SafetyHead => "safety-head",
            Self::WithinWindow => "within-window",
            Self::ExpiredCandidate => "expired-candidate",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotRetentionAssessment {
    snapshot_id: String,
    branch: String,
    created_at: String,
    age_days: Option<u32>,
    status: SnapshotRetentionStatus,
    protections: Vec<String>,
    reason: String,
}

fn run_search_baseline_retention(
    args: SearchBaselineRetentionArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let project = project_model::Project::new(&args.source_dir);
    let policy = &project.config.search.baseline.workspace_code.policy;
    if !policy.is_configured() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace baseline policy is not configured in .bsl-analyzer.json",
        )
        .into());
    }

    let adapter = build_project_postgres_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let snapshots = adapter.list_snapshots(
        Some(bsl_search::CorpusId::WorkspaceCode.as_str()),
        args.branch.as_deref(),
        None,
        args.limit,
    )?;

    if snapshots.is_empty() {
        println!("No workspace-code snapshots found.");
        return Ok(());
    }

    let assessments = analyze_snapshot_retention(policy, &snapshots, Utc::now());
    let active_heads = assessments
        .iter()
        .filter(|assessment| matches!(assessment.status, SnapshotRetentionStatus::ActiveHead))
        .count();
    let safety_heads = assessments
        .iter()
        .filter(|assessment| matches!(assessment.status, SnapshotRetentionStatus::SafetyHead))
        .count();
    let within_window = assessments
        .iter()
        .filter(|assessment| matches!(assessment.status, SnapshotRetentionStatus::WithinWindow))
        .count();
    let expired_candidates = assessments
        .iter()
        .filter(|assessment| matches!(assessment.status, SnapshotRetentionStatus::ExpiredCandidate))
        .count();
    let protected_by_min = assessments
        .iter()
        .filter(|assessment| {
            assessment.protections.iter().any(|value| value == "minimum-preservation")
        })
        .count();
    let protected_by_ancestry = assessments
        .iter()
        .filter(|assessment| assessment.protections.iter().any(|value| value == "has-descendants"))
        .count();

    println!("Shared baseline retention analysis:");
    println!("  Corpus:                     workspace-code");
    println!("  Develop retention:          {} days", policy.retention.develop_retention_days);
    println!("  Vendor heads kept:          {}", policy.retention.vendor_keep_heads);
    println!("  Min snapshots per branch:   {}", policy.retention.min_snapshots_per_branch);
    println!("  Note:                       destructive snapshot cleanup is not implemented; candidates are advisory only.");
    println!();
    println!("Summary:");
    println!("  Active heads:               {}", active_heads);
    println!("  Safety heads:               {}", safety_heads);
    println!("  Within window:              {}", within_window);
    println!("  Expired candidates:         {}", expired_candidates);
    println!("  Protected by min rule:      {}", protected_by_min);
    println!("  Protected by ancestry:      {}", protected_by_ancestry);

    for assessment in assessments {
        println!();
        println!("  Snapshot:     {}", assessment.snapshot_id);
        println!("  Branch:       {}", assessment.branch);
        println!("  Created:      {}", assessment.created_at);
        println!(
            "  Age:          {}",
            assessment
                .age_days
                .map(|days| format!("{days} days"))
                .unwrap_or_else(|| "unknown".to_owned())
        );
        println!("  Retention:    {}", assessment.status.as_str());
        println!("  Reason:       {}", assessment.reason);
        println!(
            "  Protections:  {}",
            if assessment.protections.is_empty() {
                "-".to_owned()
            } else {
                assessment.protections.join(", ")
            }
        );
    }

    Ok(())
}

fn analyze_snapshot_retention(
    policy: &project_model::SearchBaselinePolicyConfig,
    snapshots: &[bsl_search::BaselineSnapshotRecord],
    now: DateTime<Utc>,
) -> Vec<SnapshotRetentionAssessment> {
    let mut by_branch = BTreeMap::<String, Vec<&bsl_search::BaselineSnapshotRecord>>::new();
    let mut children = std::collections::HashMap::<String, usize>::new();

    for snapshot in snapshots {
        if let Some(parent_snapshot_id) = snapshot.parent_snapshot_id.as_deref() {
            *children.entry(parent_snapshot_id.to_owned()).or_default() += 1;
        }
        if let Some(branch) = snapshot.branch.as_deref().filter(|branch| !branch.trim().is_empty())
        {
            by_branch.entry(branch.to_owned()).or_default().push(snapshot);
        }
    }

    for branch_snapshots in by_branch.values_mut() {
        branch_snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot_created_at(snapshot)));
    }

    let mut assessments = Vec::new();
    for (branch, branch_snapshots) in by_branch {
        for (index, snapshot) in branch_snapshots.into_iter().enumerate() {
            let age_days = snapshot_created_at(snapshot).and_then(|created_at| {
                now.signed_duration_since(created_at).num_days().try_into().ok()
            });
            let mut protections = Vec::new();
            if index < policy.retention.min_snapshots_per_branch {
                protections.push("minimum-preservation".to_owned());
            }
            if children.contains_key(&snapshot.snapshot_id) {
                protections.push("has-descendants".to_owned());
            }

            let (status, reason) = if index == 0 {
                (SnapshotRetentionStatus::ActiveHead, "latest snapshot for branch".to_owned())
            } else if branch == "vendor" && index < policy.retention.vendor_keep_heads {
                (
                    SnapshotRetentionStatus::SafetyHead,
                    format!("vendor keeps {} latest heads", policy.retention.vendor_keep_heads),
                )
            } else if branch != "vendor"
                && age_days.is_some_and(|days| days <= policy.retention.develop_retention_days)
            {
                (
                    SnapshotRetentionStatus::WithinWindow,
                    format!(
                        "branch is within {}-day retention window",
                        policy.retention.develop_retention_days
                    ),
                )
            } else {
                (
                    SnapshotRetentionStatus::ExpiredCandidate,
                    format!(
                        "outside branch retention policy (develop-like window: {} days)",
                        policy.retention.develop_retention_days
                    ),
                )
            };

            assessments.push(SnapshotRetentionAssessment {
                snapshot_id: snapshot.snapshot_id.clone(),
                branch: branch.clone(),
                created_at: snapshot.created_at.clone(),
                age_days,
                status,
                protections,
                reason,
            });
        }
    }

    assessments.sort_by(|lhs, rhs| {
        rhs.branch
            .cmp(&lhs.branch)
            .then_with(|| rhs.age_days.unwrap_or(0).cmp(&lhs.age_days.unwrap_or(0)))
            .then_with(|| lhs.snapshot_id.cmp(&rhs.snapshot_id))
    });
    assessments
}

fn snapshot_created_at(snapshot: &bsl_search::BaselineSnapshotRecord) -> Option<DateTime<Utc>> {
    project_model::parse_timestamp_utc(&snapshot.created_at)
}

fn resolve_snapshot_id(
    corpus: &bsl_search::CorpusId,
    snapshot_id: Option<&str>,
    branch: Option<&str>,
    commit: Option<&str>,
) -> Result<String, io::Error> {
    if let Some(snapshot_id) = snapshot_id.filter(|value| !value.trim().is_empty()) {
        return Ok(snapshot_id.to_owned());
    }

    let prefix = corpus.as_str();
    match (
        branch.filter(|value| !value.trim().is_empty()),
        commit.filter(|value| !value.trim().is_empty()),
    ) {
        (Some(branch), Some(commit)) => Ok(format!("{prefix}:{branch}@{commit}")),
        (Some(branch), None) => Ok(format!("{prefix}:{branch}")),
        (None, Some(commit)) => Ok(format!("{prefix}:{commit}")),
        _ if matches!(corpus, bsl_search::CorpusId::Reference) => {
            Ok(format!("reference:{}", env!("CARGO_PKG_VERSION")))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--snapshot-id is required unless --branch or --commit is provided",
        )),
    }
}

fn resolve_parent_snapshot_id(
    adapter: &bsl_search::ExternalBaselineAdapter,
    corpus: &bsl_search::CorpusId,
    branch: Option<&str>,
    current_snapshot_id: &str,
    explicit_parent_snapshot_id: Option<&str>,
    workspace_policy: Option<&project_model::SearchBaselinePolicyConfig>,
) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    if let Some(parent_snapshot_id) =
        explicit_parent_snapshot_id.map(str::trim).filter(|value| !value.is_empty())
    {
        return Ok(Some(parent_snapshot_id.to_owned()));
    }

    let mut snapshot_groups = Vec::new();
    if let Some(branch) = branch.map(str::trim).filter(|value| !value.is_empty()) {
        let mut branch_candidates = vec![branch.to_owned()];
        if matches!(corpus, bsl_search::CorpusId::WorkspaceCode)
            && workspace_policy
                .is_some_and(project_model::SearchBaselinePolicyConfig::is_configured)
        {
            if let Some(policy) = workspace_policy {
                if let Some(selection) =
                    project_model::resolve_workspace_branch_policy(policy, Some(branch))
                {
                    for candidate in selection.candidate_branches() {
                        if branch_candidates.iter().all(|existing| existing != &candidate) {
                            branch_candidates.push(candidate);
                        }
                    }
                }
            }
        }

        for branch_candidate in branch_candidates {
            snapshot_groups.push(adapter.list_snapshots(
                Some(corpus.as_str()),
                Some(branch_candidate.as_str()),
                None,
                2,
            )?);
        }

        return Ok(select_parent_snapshot_id_from_groups(current_snapshot_id, &snapshot_groups));
    }

    let snapshots = adapter.list_snapshots(Some(corpus.as_str()), branch, None, 2)?;
    Ok(select_parent_snapshot_id(current_snapshot_id, &snapshots))
}

fn select_parent_snapshot_id_from_groups(
    current_snapshot_id: &str,
    snapshot_groups: &[Vec<bsl_search::BaselineSnapshotRecord>],
) -> Option<String> {
    snapshot_groups
        .iter()
        .find_map(|snapshots| select_parent_snapshot_id(current_snapshot_id, snapshots))
}

fn select_parent_snapshot_id(
    current_snapshot_id: &str,
    snapshots: &[bsl_search::BaselineSnapshotRecord],
) -> Option<String> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.snapshot_id != current_snapshot_id)
        .map(|snapshot| snapshot.snapshot_id.clone())
}

fn resolve_project_postgres_url(
    postgres: &project_model::SearchPostgresConfig,
    mode: project_model::PostgresAccessMode,
) -> Result<project_model::ResolvedPostgresUrl, project_model::ResolvePostgresUrlError> {
    project_model::resolve_postgres_url(postgres, mode)
}

fn build_project_postgres_adapter(
    source_dir: &Path,
    mode: project_model::PostgresAccessMode,
) -> Result<bsl_search::ExternalBaselineAdapter, Box<dyn Error + Send + Sync>> {
    let project = project_model::Project::new(source_dir);
    let resolved = resolve_project_postgres_url(&project.config.search.baseline.postgres, mode)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("failed to resolve PostgreSQL {} credentials: {error}", mode.as_str()),
            )
        })?;
    build_postgres_baseline_adapter(
        &resolved.url,
        project.config.search.baseline.postgres.schema.as_deref(),
    )
}

fn build_postgres_baseline_adapter(
    pg_url: &str,
    pg_schema: Option<&str>,
) -> Result<bsl_search::ExternalBaselineAdapter, Box<dyn Error + Send + Sync>> {
    let mut config = bsl_search::ExternalBaselineConfig::postgres(pg_url.to_owned());
    if let Some(schema) = pg_schema {
        config = config.with_schema(schema.to_owned());
    }
    Ok(bsl_search::ExternalBaselineAdapter::new(config)?)
}

fn embedding_config_from_env() -> Option<bsl_search::EmbedderConfig> {
    let base_url = env::var("EMBEDDING_URL").ok()?;
    let model =
        env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "Qwen/Qwen3-Embedding-0.6B".to_owned());
    let dim = env::var("EMBEDDING_DIM").ok().and_then(|value| value.parse().ok()).or(Some(1024));

    Some(bsl_search::EmbedderConfig {
        base_url,
        model,
        dim,
        api_key: env::var("EMBEDDING_API_KEY").ok(),
        provider: env::var("EMBEDDING_PROVIDER").ok(),
    })
}

fn embedding_execution_policy_from_env() -> bsl_search::EmbeddingExecutionPolicy {
    bsl_search::EmbeddingExecutionPolicy {
        batch_size: env::var("EMBEDDING_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(32),
        concurrency: env::var("EMBEDDING_CONCURRENCY")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10),
        progress_interval: env::var("EMBEDDING_PROGRESS_INTERVAL")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(20),
    }
}

fn search_baseline_corpus_cli_to_domain(corpus: SearchBaselineCorpusCli) -> bsl_search::CorpusId {
    match corpus {
        SearchBaselineCorpusCli::WorkspaceCode => bsl_search::CorpusId::WorkspaceCode,
        SearchBaselineCorpusCli::Reference => bsl_search::CorpusId::Reference,
    }
}

fn shorten_fingerprint(fingerprint: &str) -> &str {
    fingerprint.get(..12).unwrap_or(fingerprint)
}

fn format_ratio(numerator: usize, denominator: usize) -> String {
    if denominator == 0 {
        return "0/0 (n/a)".to_owned();
    }

    let percent = (numerator as f64 / denominator as f64) * 100.0;
    format!("{numerator}/{denominator} ({percent:.1}%)")
}

fn pick_first_non_empty<'a>(
    candidates: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn resolve_publish_branch(
    project_branch: Option<&str>,
    project_root: &std::path::Path,
) -> Option<String> {
    let git_branch = project_model::current_git_branch(project_root);
    pick_first_non_empty([
        project_branch,
        env::var("CI_COMMIT_BRANCH").ok().as_deref(),
        env::var("CI_COMMIT_REF_NAME").ok().as_deref(),
        git_branch.as_deref(),
    ])
}

fn resolve_publish_commit(
    explicit_commit: Option<&str>,
    project_root: &std::path::Path,
) -> Option<String> {
    let git_commit = project_model::current_git_commit(project_root);
    pick_first_non_empty([
        explicit_commit,
        env::var("CI_COMMIT_SHA").ok().as_deref(),
        env::var("GITHUB_SHA").ok().as_deref(),
        git_commit.as_deref(),
    ])
}

fn validate_workspace_publish_policy(
    policy: &project_model::SearchBaselinePolicyConfig,
    branch: Option<&str>,
    allow_non_policy_branch: bool,
) -> Result<(), io::Error> {
    if !policy.is_configured() {
        return Ok(());
    }

    if policy.publish_branches.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace baseline policy is configured but 'workspaceCode.policy.publishBranches' is empty",
        ));
    }

    let Some(branch) = branch.map(str::trim).filter(|branch| !branch.is_empty()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace baseline publish policy requires a branch; pass --branch, set CI_COMMIT_BRANCH, or run inside a git repository",
        ));
    };

    if project_model::is_publish_branch_allowed(policy, branch) || allow_non_policy_branch {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "branch '{branch}' is not allowed by workspace baseline publish policy; use --allow-non-policy-branch to override"
        ),
    ))
}

fn build_workspace_code_baseline_documents(
    source_path: &std::path::Path,
) -> Result<(usize, Vec<bsl_search::IndexedDocument>), Box<dyn Error + Send + Sync>> {
    use bsl_search::SearchEngine;

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("baseline-sync.db");
    let mut engine = SearchEngine::fts_only(&db_path)?;
    let indexed_files = engine.index_directory_fts(source_path)?;
    let documents = engine.load_indexed_documents(Some("code"))?;
    Ok((indexed_files, documents))
}

fn build_reference_baseline_documents(
) -> Result<(usize, Vec<bsl_search::IndexedDocument>), Box<dyn Error + Send + Sync>> {
    use bsl_search::SearchEngine;

    let documents = build_reference_source_documents();

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("reference-baseline-sync.db");
    let mut engine = SearchEngine::fts_only(&db_path)?;
    let indexed_files = engine.index_documents(
        "platform",
        "platform://docs",
        env!("CARGO_PKG_VERSION").as_bytes(),
        &documents,
        None,
    )?;
    let indexed_documents = engine.load_indexed_documents(Some("platform"))?;
    Ok((indexed_files, indexed_documents))
}

fn build_reference_source_documents() -> Vec<bsl_search::Document> {
    use bsl_platform::PlatformDataInner;
    use bsl_search::Document;

    let platform = PlatformDataInner::instance();
    let mut documents = Vec::new();

    for ty in platform.all_types() {
        let methods = platform.get_type_methods(&ty.name);
        let method_list: String = methods
            .iter()
            .map(|method| format!("{} / {}", method.name, method.english_name))
            .collect::<Vec<_>>()
            .join(", ");

        documents.push(Document {
            title: format!("{} / {}", ty.name, ty.english_name),
            body: format!("Тип: {} / {}\nМетоды: {method_list}", ty.name, ty.english_name),
            kind: "type".to_owned(),
        });
    }

    for method in platform.all_methods() {
        let mut body = format!(
            "Тип: {}\nМетод: {} / {}\n",
            method.type_name, method.name, method.english_name
        );
        if let Some(ref ret) = method.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_method_docs(method.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
            for example in &docs.examples {
                body.push_str(&format!("Пример: {}\n", example.code));
            }
        }
        documents.push(Document {
            title: format!(
                "{}.{} / {}.{}",
                method.type_name, method.name, method.type_name, method.english_name
            ),
            body,
            kind: "method".to_owned(),
        });
    }

    for func in platform.all_global_functions() {
        let mut body = format!("Глобальная функция: {} / {}\n", func.name, func.english_name);
        if let Some(ref ret) = func.return_type {
            body.push_str(&format!("Возвращает: {ret}\n"));
        }
        if let Some(docs) = platform.get_global_function_docs(func.id) {
            if !docs.syntax.is_empty() {
                body.push_str(&format!("Синтаксис: {}\n", docs.syntax));
            }
            if !docs.description.is_empty() {
                body.push_str(&format!("Описание: {}\n", docs.description));
            }
            for param in &docs.params {
                body.push_str(&format!("Параметр {}: {}\n", param.name, param.description));
            }
        }
        documents.push(Document {
            title: format!("{} / {}", func.name, func.english_name),
            body,
            kind: "global_function".to_owned(),
        });
    }

    documents
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

fn run_rules_command(command: RulesCommands) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::{all_diagnostic_codes, docs, get_metadata};

    match command {
        RulesCommands::Export { format, lang, output } => {
            let rules = export_rules(&lang, &format);

            let json = serde_json::to_string_pretty(&rules)?;

            match output {
                Some(path) => {
                    fs::write(&path, &json)?;
                    eprintln!("Rules exported to: {:?}", path);
                }
                None => {
                    println!("{}", json);
                }
            }
        }
        RulesCommands::List => {
            println!("Available diagnostic codes:\n");
            for code in all_diagnostic_codes() {
                let docs = docs::get_docs(code);
                let name = if lang_is_russian() { docs.name_ru } else { docs.name_en };
                let status = if let Some(meta) = get_metadata(code) {
                    if meta.activated_by_default {
                        "enabled"
                    } else {
                        "disabled"
                    }
                } else {
                    "unknown"
                };
                println!("  {:40} [{}] {}", format!("{:?}", code), status, name);
            }
        }
    }

    Ok(())
}

fn run_extension_command(command: ExtensionCommands) -> Result<(), Box<dyn Error + Send + Sync>> {
    match command {
        ExtensionCommands::Export { output } => {
            static EXTENSION_ZIP: &[u8] =
                include_bytes!(concat!(env!("OUT_DIR"), "/extension.zip"));

            let cursor = std::io::Cursor::new(EXTENSION_ZIP);
            let mut archive = zip::ZipArchive::new(cursor)?;

            let mut count = 0;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i)?;
                if entry.is_dir() {
                    continue;
                }
                let dest = output.join(entry.name());
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut out_file = fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut out_file)?;
                count += 1;
            }

            eprintln!("Extension exported to: {}", output.display());
            eprintln!("Files: {count}");
            eprintln!();
            eprintln!("To install into 1C infobase:");
            eprintln!(
                "  rtools config extension import -d <database> -e BSL_Analyzer -i {}",
                output.display()
            );
            eprintln!("  rtools config extension apply -d <database> -e BSL_Analyzer");
        }
    }

    Ok(())
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

/// Decode password: if prefixed with "base64:", decode from base64.
/// Otherwise return as-is. Not encryption — just obfuscation for .mcp.json.
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

fn lang_is_russian() -> bool {
    std::env::var("LANG").map(|l| l.starts_with("ru")).unwrap_or(true)
}

fn export_rules(lang: &str, format: &RulesFormat) -> serde_json::Value {
    use ide::{
        all_diagnostic_codes, docs, get_metadata, CleanCodeAttribute, DiagnosticSeverityLevel,
        DiagnosticType, ImpactSeverity, SoftwareQuality,
    };

    let is_ru = lang == "ru";

    // Includes deprecated rules (e.g. `MissingCommonModuleMethod` —
    // superseded by `UnresolvedMethodCall` in Phase 2 of the
    // qualified-call refactor). The rule entries are kept in the
    // SonarQube export so existing downstream profiles keep validating;
    // the rules themselves never fire any more. Full removal lands in
    // Phase 4.
    let rules: Vec<serde_json::Value> = all_diagnostic_codes()
        .filter_map(|code| {
            let metadata = get_metadata(code)?;
            let docs = docs::get_docs(code);

            let name = if is_ru { docs.name_ru } else { docs.name_en };
            let description = if is_ru { docs.description_ru } else { docs.description_en };

            // Convert to SonarQube format
            let sonar_type = match metadata.diagnostic_type {
                DiagnosticType::Error => "BUG",
                DiagnosticType::CodeSmell => "CODE_SMELL",
                DiagnosticType::Vulnerability => "VULNERABILITY",
                DiagnosticType::SecurityHotspot => "SECURITY_HOTSPOT",
            };

            let sonar_severity = match metadata.severity {
                DiagnosticSeverityLevel::Blocker => "BLOCKER",
                DiagnosticSeverityLevel::Critical => "CRITICAL",
                DiagnosticSeverityLevel::Major => "MAJOR",
                DiagnosticSeverityLevel::Minor => "MINOR",
                DiagnosticSeverityLevel::Info => "INFO",
            };

            // Convert Clean Code attribute to SonarQube format (valid enum values)
            let clean_code_attribute = match metadata.clean_code_attribute {
                CleanCodeAttribute::Consistent => "CONVENTIONAL",
                CleanCodeAttribute::Intentional => "CLEAR",
                CleanCodeAttribute::Adaptable => "FOCUSED",
                CleanCodeAttribute::Responsible => "TRUSTWORTHY",
            };

            // Convert impacts to SonarQube format
            let impacts: Vec<serde_json::Value> = metadata
                .impacts
                .iter()
                .map(|impact| {
                    let software_quality = match impact.software_quality {
                        SoftwareQuality::Maintainability => "MAINTAINABILITY",
                        SoftwareQuality::Reliability => "RELIABILITY",
                        SoftwareQuality::Security => "SECURITY",
                    };
                    let severity = match impact.severity {
                        ImpactSeverity::Low => "LOW",
                        ImpactSeverity::Medium => "MEDIUM",
                        ImpactSeverity::High => "HIGH",
                    };
                    serde_json::json!({
                        "softwareQuality": software_quality,
                        "severity": severity
                    })
                })
                .collect();

            // Convert description from Markdown to HTML (simple conversion)
            let html_description = markdown_to_html(description);

            // Collect tags
            let tags: Vec<&str> = metadata.tags.iter().map(|t| tag_to_str(t)).collect();

            Some(serde_json::json!({
                "code": format!("{:?}", code),
                "name": if name.is_empty() { format!("{:?}", code) } else { name.to_string() },
                "description": html_description,
                "type": sonar_type,
                "severity": sonar_severity,
                "cleanCodeAttribute": clean_code_attribute,
                "impacts": impacts,
                "active": metadata.activated_by_default,
                "effortMinutes": metadata.minutes_to_fix,
                "tags": tags
            }))
        })
        .collect();

    match format {
        RulesFormat::Sonarqube => serde_json::json!({ "rules": rules }),
        RulesFormat::Json => serde_json::json!(rules),
    }
}

fn tag_to_str(tag: &ide::MetadataTag) -> &'static str {
    use ide::MetadataTag;
    match tag {
        MetadataTag::Standard => "standard",
        MetadataTag::Lockinos => "lockinos",
        MetadataTag::Sql => "sql",
        MetadataTag::Performance => "performance",
        MetadataTag::Brainoverload => "brainoverload",
        MetadataTag::Badpractice => "badpractice",
        MetadataTag::Clumsy => "clumsy",
        MetadataTag::Design => "design",
        MetadataTag::Suspicious => "suspicious",
        MetadataTag::Unpredictable => "unpredictable",
        MetadataTag::Deprecated => "deprecated",
        MetadataTag::Unused => "unused",
        MetadataTag::Error => "error",
        MetadataTag::Localize => "localize",
    }
}

/// Simple Markdown to HTML conversion for SonarQube descriptions.
fn markdown_to_html(md: &str) -> String {
    if md.is_empty() {
        return String::new();
    }

    let mut html = String::new();
    let mut in_code_block = false;
    let mut in_list = false;
    let mut current_paragraph = String::new();

    for line in md.lines() {
        // Skip metadata comments
        if line.starts_with("<!--") || line.ends_with("-->") {
            continue;
        }

        // Code blocks
        if line.starts_with("```") {
            if in_code_block {
                html.push_str("</code></pre>\n");
                in_code_block = false;
            } else {
                flush_paragraph(&mut html, &mut current_paragraph);
                html.push_str("<pre><code>");
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            // Escape HTML in code blocks
            html.push_str(&line.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
            html.push('\n');
            continue;
        }

        let trimmed = line.trim();

        // Empty line - close paragraph
        if trimmed.is_empty() {
            flush_paragraph(&mut html, &mut current_paragraph);
            if in_list {
                html.push_str("</ul>\n");
                in_list = false;
            }
            continue;
        }

        // Headers
        if let Some(header) = trimmed.strip_prefix("# ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            // Skip the main title (first header)
            if !html.is_empty() {
                html.push_str(&format!("<h3>{}</h3>\n", escape_html(header)));
            }
            continue;
        }
        if let Some(header) = trimmed.strip_prefix("## ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            html.push_str(&format!("<h4>{}</h4>\n", escape_html(header)));
            continue;
        }

        // List items
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            let item = &trimmed[2..];
            html.push_str(&format!("<li>{}</li>\n", escape_html(item)));
            continue;
        }

        // Regular text - accumulate into paragraph
        if !current_paragraph.is_empty() {
            current_paragraph.push(' ');
        }
        current_paragraph.push_str(trimmed);
    }

    // Flush remaining content
    flush_paragraph(&mut html, &mut current_paragraph);
    if in_list {
        html.push_str("</ul>\n");
    }
    if in_code_block {
        html.push_str("</code></pre>\n");
    }

    html.trim().to_string()
}

fn flush_paragraph(html: &mut String, paragraph: &mut String) {
    if !paragraph.is_empty() {
        html.push_str(&format!("<p>{}</p>\n", escape_html(paragraph)));
        paragraph.clear();
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
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

    // Keep one clone alive until after the Tokio runtime is dropped.
    // This ensures sync PostgreSQL baseline clients/pools are destroyed
    // outside the active runtime context and avoids nested-runtime panics
    // during MCP stdio shutdown.
    drop(rt);
    shutdown_guard.shutdown();
    drop(shutdown_guard);

    serve_result?;
    Ok(())
}

fn run_dap_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Starting DAP debug adapter");
    bsl_debug::dap::run_dap_stdio();
    Ok(())
}

fn run_lsp_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    use lsp_server::Connection;

    tracing::info!("Starting BSL Analyzer LSP server");

    // Create LSP connection over stdio
    let (connection, io_threads) = Connection::stdio();
    tracing::info!("LSP connection established via stdio");

    // Run the main loop
    tracing::info!("Entering main loop");
    if let Err(e) = bsl_analyzer::main_loop(connection) {
        tracing::error!("Main loop error: {}", e);
        tracing::error!("Error chain: {:?}", e);
        return Err(e.into());
    }

    // Join IO threads - ignore errors during shutdown
    // (client may close connection before we finish sending)
    tracing::info!("Joining IO threads");
    if let Err(e) = io_threads.join() {
        tracing::debug!("IO threads join error (expected during shutdown): {}", e);
    }

    tracing::info!("LSP server shut down cleanly");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn analyze(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    _incremental: bool,
    _changed_files: Option<Vec<PathBuf>>,
    _git_diff: Option<String>,
    salsa: bool,
    workers: Option<usize>,
    format: OutputFormat,
    only_diagnostic: Option<String>,
    diff_filter_path: Option<PathBuf>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Load diff filter if provided
    let diff_filter = if let Some(ref path) = diff_filter_path {
        Some(bsl_analyzer::diff_filter::DiffFilter::load(path)?)
    } else {
        None
    };

    // CLI defaults to streaming mode; --salsa flag switches to Salsa (for testing)
    if salsa {
        analyze_salsa(
            source_dir,
            workspace_dir,
            output_dir,
            config_path,
            reporters,
            quiet,
            only_diagnostic,
            diff_filter,
        )
    } else {
        analyze_streaming(
            source_dir,
            workspace_dir,
            output_dir,
            config_path,
            reporters,
            quiet,
            workers,
            format,
            only_diagnostic,
            diff_filter,
        )
    }
}

/// Timing result for a single file (used for profiling).
struct FileTiming {
    path: PathBuf,
    duration: std::time::Duration,
}

/// Aggregated profiling statistics for --only-diagnostic mode.
struct ProfilingStats {
    diagnostic_name: String,
    total: std::time::Duration,
    count: usize,
    max: std::time::Duration,
    max_file: PathBuf,
}

impl ProfilingStats {
    fn from_timings(diagnostic_name: String, timings: Vec<FileTiming>) -> Option<Self> {
        if timings.is_empty() {
            return None;
        }

        let total: std::time::Duration = timings.iter().map(|t| t.duration).sum();
        let count = timings.len();
        let max_timing = timings.iter().max_by_key(|t| t.duration)?;

        Some(Self {
            diagnostic_name,
            total,
            count,
            max: max_timing.duration,
            max_file: max_timing.path.clone(),
        })
    }

    fn average(&self) -> std::time::Duration {
        if self.count == 0 {
            std::time::Duration::ZERO
        } else {
            self.total / self.count as u32
        }
    }

    fn print(&self) {
        println!();
        println!("=== Diagnostic Profiling: {} ===", self.diagnostic_name);
        println!("Files analyzed: {}", self.count);
        println!("Total time:     {:.3}ms", self.total.as_secs_f64() * 1000.0);
        println!("Average time:   {:.3}ms", self.average().as_secs_f64() * 1000.0);
        println!("Max time:       {:.3}ms", self.max.as_secs_f64() * 1000.0);
        println!("Max file:       {}", self.max_file.display());
    }
}

#[allow(clippy::too_many_arguments)]
fn analyze_salsa(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    only_diagnostic: Option<String>,
    diff_filter: Option<bsl_analyzer::diff_filter::DiffFilter>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use base_db::SourceDatabase;
    use ide::DiagnosticsContext;
    use ide::{DiagnosticsConfig, RootDatabaseImpl};
    use indicatif::{ProgressBar, ProgressStyle};
    use rayon::prelude::*;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;
    use vfs::FileId;
    use walkdir::WalkDir;

    use bsl_analyzer::reporters::{AnalysisResults, FileAnalysis, ReporterRegistry};

    let _span = tracing::info_span!("cli_analyze").entered();

    // Profiling mode enabled when --only-diagnostic is set
    let profiling_enabled = only_diagnostic.is_some();

    tracing::info!("Analyzing project: {:?}", source_dir);
    tracing::info!("Reporters: {:?}", reporters);
    tracing::info!("Quiet mode: {}", quiet);
    if let Some(ref diag) = only_diagnostic {
        tracing::info!("Profiling diagnostic: {}", diag);
    }
    if diff_filter.is_some() {
        tracing::info!("Diff filter enabled");
    }

    let start = Instant::now();

    // Determine workspace and output directories
    // workspace_dir defaults to source_dir
    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    // output_dir defaults to current directory "."
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    // Load project configuration
    tracing::info!("Loading project configuration");
    let proj_config = if let Some(ref cfg) = config_path {
        // -c flag passes file path directly, use load_from_file
        project_model::ProjectConfig::load_from_file(cfg).unwrap_or_default()
    } else {
        // No explicit config - search in source_dir
        project_model::ProjectConfig::load(&source_dir).unwrap_or_default()
    };

    // Load metadata if available (for validation/logging, Salsa loads per-file)
    let _metadata = proj_config.load_metadata(&source_dir);
    let configuration_path = proj_config.configuration_path(&source_dir);

    // Create database
    tracing::info!("Creating database");
    let mut db = RootDatabaseImpl::default();

    // Find all BSL files first
    tracing::info!("Finding BSL files in {:?}", source_dir);
    let mut bsl_files = Vec::new();
    for entry in WalkDir::new(&source_dir).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "bsl" {
                    bsl_files.push(entry.path().to_path_buf());
                }
            }
        }
    }

    tracing::info!("Found {} BSL files", bsl_files.len());

    // Build FileSet from ALL files (needed for cross-module resolution)
    tracing::info!("Loading files into database");
    let mut file_set = vfs::FileSet::new();
    let mut all_file_ids: Vec<(FileId, PathBuf)> = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        all_file_ids.push((file_id, path.clone()));
    }

    // Filter to determine which files need diagnostics
    let file_ids: Vec<(FileId, PathBuf)> = if let Some(ref filter) = diff_filter {
        let filtered: Vec<_> = all_file_ids
            .iter()
            .filter(|(_, path)| {
                let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                filter.should_analyze(rel_path)
            })
            .cloned()
            .collect();
        tracing::info!(
            "After diff filter: {} files to analyze (from {} total in VFS)",
            filtered.len(),
            all_file_ids.len()
        );
        filtered
    } else {
        all_file_ids.clone()
    };

    // Setup source root with FileSet for path resolution
    // (used by sdbl_hir_in_file_query to find configuration root)
    let file_set_arc = Arc::new(file_set);
    let source_root_id = base_db::SourceRootId(0);
    let source_root = base_db::SourceRoot::new_local((*file_set_arc).clone());
    db.set_source_root(source_root_id, source_root);

    // Load file contents into database
    // Load ALL files into database (needed for cross-module resolution)
    for (file_id, path) in &all_file_ids {
        let content = fs::read_to_string(path)?;
        db.set_file_source_root(*file_id, source_root_id);
        db.set_file_text(*file_id, &content);
    }

    tracing::info!(
        "Files loaded, starting parallel diagnostics (threads: {})",
        rayon::current_num_threads()
    );

    // Setup progress bar
    let progress = if !quiet {
        let pb = ProgressBar::new(file_ids.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // CRITICAL: Create ConfigurationPathInput ONCE for Salsa caching!
    // If we create a new input for each diagnostic, Salsa won't cache the metadata load
    let config_path_input = configuration_path
        .as_ref()
        .map(|path| metadata::intern_configuration_path(&db, &path.to_string_lossy(), 0));

    // Parallel diagnostics execution WITHOUT Mutex!
    // Use map_with to clone database for each thread worker
    // map_with calls db.clone() once per thread (not per file!)
    // Salsa's clone creates snapshot with new ZalsaLocal (per-thread state)

    // Load diagnostics config from project config (unified with streaming mode)
    let mut config: DiagnosticsConfig =
        serde_json::from_value(proj_config.diagnostics.clone()).unwrap_or_default();
    // Project-level `[output] display_language` only takes effect once
    // explicitly applied to the typed config — JSON deserialization carries
    // no locale signal. CLI mode has no LSP `InitializeParams.locale`, so the
    // resolved value (or the analyzer default) is the only locale source.
    config.locale = proj_config.output.resolve_locale().unwrap_or_default();

    // Apply --only-diagnostic filter (enables profiling mode)
    if let Some(ref diag_name) = only_diagnostic {
        config.apply_cli_filters(std::slice::from_ref(diag_name), &[]);
    }

    tracing::info!(
        disabled = config.disabled.len(),
        only_enabled = ?config.only_enabled.as_ref().map(|v| v.len()),
        params = config.parameters.len(),
        locale = ?config.locale,
        "Loaded DiagnosticsConfig"
    );
    let config = Arc::new(config);
    let processed = Arc::new(AtomicUsize::new(0));
    let progress_arc = Arc::new(progress);
    let workspace_dir_arc = Arc::new(workspace_dir.clone());
    let source_dir_arc = Arc::new(source_dir.clone());
    let diff_filter_arc = Arc::new(diff_filter);
    // file_set_arc is created above and stored in SourceRoot

    // Result type: (Option<FileAnalysis>, Option<FileTiming>)
    let results: Vec<(Option<FileAnalysis>, Option<FileTiming>)> = file_ids
        .par_iter()
        .map_with(db.clone(), |db_snapshot, (file_id, path)| {
            let provider = ide_db::SalsaProvider::with_file_set(
                db_snapshot,
                config_path_input,
                Some(&file_set_arc),
            );
            let ctx = DiagnosticsContext::new(&config, *file_id, &provider);

            // Catch panics to continue analyzing other files if one fails
            let file_start = std::time::Instant::now();
            let diagnostics =
                match catch_unwind(AssertUnwindSafe(|| ide::compute_diagnostics(&ctx))) {
                    Ok(diags) => diags,
                    Err(e) => {
                        tracing::error!("Panic analyzing {:?}: {:?}", path, e);
                        return (None, None);
                    }
                };
            let elapsed = file_start.elapsed();

            // Collect timing for profiling (only when --only-diagnostic is set)
            let timing = if profiling_enabled {
                Some(FileTiming { path: path.clone(), duration: elapsed })
            } else {
                None
            };

            // Update progress (lock-free atomic counter)
            let count = processed.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(ref pb) = &*progress_arc {
                pb.set_position(count as u64);
                pb.set_message(format!(
                    "{:.0} files/sec",
                    count as f64 / start.elapsed().as_secs_f64()
                ));
            }

            // Build file analysis if diagnostics found
            let file_analysis = if !diagnostics.is_empty() {
                // Convert Diagnostic → DiagnosticOutput for reporters
                // Read file text for LineIndex in to_output()
                let file_text = match fs::read_to_string(path) {
                    Ok(text) => text,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read file {:?} for diagnostic conversion: {}",
                            path,
                            e
                        );
                        return (None, timing);
                    }
                };

                // Convert diagnostics to output format.
                // Build the line index once per file — per-diagnostic
                // `to_output()` rebuilt it on every call, dominating the CLI
                // `analyze` profile at ~43% self time on a 25k-file workspace.
                let file_line_index = line_index::LineIndex::new(&file_text);
                let mut diagnostic_outputs: Vec<_> = diagnostics
                    .iter()
                    .map(|d| d.to_output_with_index(&file_text, &file_line_index))
                    .collect();

                // Filter diagnostics by diff hunks if enabled
                if let Some(ref filter) = *diff_filter_arc {
                    let rel_path = path.strip_prefix(&*source_dir_arc).unwrap_or(path);
                    diagnostic_outputs.retain(|d| {
                        filter.diagnostic_in_diff(rel_path, d.start_line as u32, d.end_line as u32)
                    });
                }

                if diagnostic_outputs.is_empty() {
                    None
                } else {
                    Some(FileAnalysis {
                        path: path.clone(),
                        relative_path: path
                            .strip_prefix(&*workspace_dir_arc)
                            .unwrap_or(path)
                            .to_path_buf(),
                        diagnostics: diagnostic_outputs,
                    })
                }
            } else {
                None
            };

            (file_analysis, timing)
        })
        .collect();

    // Finish progress bar
    if let Some(ref pb) = &*progress_arc {
        pb.finish_with_message("Analysis complete");
    }

    let elapsed = start.elapsed();

    // Separate diagnostics and timings
    let (file_analyses, timings): (Vec<_>, Vec<_>) = results.into_iter().unzip();
    let all_diagnostics: Vec<FileAnalysis> = file_analyses.into_iter().flatten().collect();
    let all_timings: Vec<FileTiming> = timings.into_iter().flatten().collect();

    // Create analysis results
    let total_diagnostics: usize = all_diagnostics.iter().map(|f| f.diagnostics.len()).sum();

    let analysis_results = AnalysisResults {
        files_analyzed: bsl_files.len(),
        files_with_issues: all_diagnostics.len(),
        total_diagnostics,
        elapsed_secs: elapsed.as_secs_f64(),
        diagnostics: all_diagnostics,
        source_dir: source_dir.clone(),
        workspace_dir: workspace_dir.clone(),
    };

    // Create output directory if needed
    std::fs::create_dir_all(&output_dir)?;

    // Run reporters
    let registry = ReporterRegistry::new();
    let reporter_keys = if reporters.is_empty() { vec!["console".to_string()] } else { reporters };

    for key in &reporter_keys {
        match registry.get(key) {
            Some(reporter) => {
                if let Err(e) = reporter.report(&analysis_results, &output_dir) {
                    tracing::error!("Reporter '{}' failed: {}", key, e);
                    eprintln!("Error: Reporter '{}' failed: {}", key, e);
                }
            }
            None => {
                eprintln!("Error: Unknown reporter '{}'", key);
                eprintln!("Valid reporters: {}", registry.keys().join(", "));
                return Err(format!("Unknown reporter: {}", key).into());
            }
        }
    }

    // Print profiling stats if --only-diagnostic was used
    if let Some(diag_name) = only_diagnostic {
        if let Some(stats) = ProfilingStats::from_timings(diag_name, all_timings) {
            stats.print();
        }
    }

    tracing::info!("Analysis complete");

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn analyze_streaming(
    source_dir: PathBuf,
    workspace_dir: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    reporters: Vec<String>,
    quiet: bool,
    workers: Option<usize>,
    format: OutputFormat,
    only_diagnostic: Option<String>,
    diff_filter: Option<bsl_analyzer::diff_filter::DiffFilter>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::{streaming::AnalysisOrchestrator, DiagnosticsConfig};
    use vfs::FileId;
    use walkdir::WalkDir;

    let _span = tracing::info_span!("cli_analyze_streaming").entered();

    // Canonicalize source_dir to avoid double-join in FileReader::from_disk
    let source_dir = std::fs::canonicalize(&source_dir).unwrap_or(source_dir);

    // Profiling mode enabled when --only-diagnostic is set
    let profiling_enabled = only_diagnostic.is_some();
    if let Some(ref diag) = only_diagnostic {
        tracing::info!("Profiling diagnostic (streaming mode): {}", diag);
    }
    if diff_filter.is_some() {
        tracing::info!("Diff filter enabled (streaming mode)");
    }

    tracing::info!("Analyzing project (streaming mode): {:?}", source_dir);
    tracing::info!("Workers: {:?}", workers);
    tracing::info!("Format: {:?}", format);

    // Determine workspace and output directories
    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    // Load project configuration
    let proj_config = if let Some(ref cfg) = config_path {
        // -c flag passes file path directly, use load_from_file
        project_model::ProjectConfig::load_from_file(cfg).unwrap_or_default()
    } else {
        // No explicit config - search in source_dir
        project_model::ProjectConfig::load(&source_dir).unwrap_or_default()
    };

    // Load diagnostics config from project config and apply CLI filters
    let mut diag_config: DiagnosticsConfig =
        serde_json::from_value(proj_config.diagnostics.clone()).unwrap_or_default();
    // Apply `[output] display_language` (CLI streaming has no LSP locale).
    diag_config.locale = proj_config.output.resolve_locale().unwrap_or_default();

    if let Some(ref diag_name) = only_diagnostic {
        diag_config.apply_cli_filters(std::slice::from_ref(diag_name), &[]);
    }

    tracing::info!(
        disabled = diag_config.disabled.len(),
        only_enabled = ?diag_config.only_enabled.as_ref().map(|v| v.len()),
        params = diag_config.parameters.len(),
        locale = ?diag_config.locale,
        "Loaded DiagnosticsConfig (streaming mode)"
    );

    // Find all BSL files
    tracing::info!("Finding BSL files in {:?}", source_dir);
    let mut bsl_files = Vec::new();
    for entry in WalkDir::new(&source_dir).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "bsl" {
                    bsl_files.push(entry.path().to_path_buf());
                }
            }
        }
    }

    tracing::info!("Found {} BSL files", bsl_files.len());

    if bsl_files.is_empty() {
        tracing::warn!("No BSL files found in {:?}", source_dir);
        if !matches!(format, OutputFormat::Jsonl) {
            println!("No BSL files found!");
        }
        return Ok(());
    }

    // Build FileSet from ALL files (needed for cross-module resolution)
    let mut file_set = vfs::FileSet::new();
    let mut all_file_ids: Vec<(FileId, PathBuf)> = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        all_file_ids.push((file_id, path.clone()));
    }

    // Filter to determine which files need diagnostics
    let file_ids: Vec<(FileId, PathBuf)> = if let Some(ref filter) = diff_filter {
        let filtered: Vec<_> = all_file_ids
            .iter()
            .filter(|(_, path)| {
                let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                filter.should_analyze(rel_path)
            })
            .cloned()
            .collect();
        tracing::info!(
            "After diff filter: {} files to analyze (from {} total in VFS)",
            filtered.len(),
            all_file_ids.len()
        );
        filtered
    } else {
        all_file_ids.clone()
    };

    // Create orchestrator with diagnostics config
    let mut builder =
        AnalysisOrchestrator::builder().workspace_root(&source_dir).diagnostics_config(diag_config);

    if let Some(w) = workers {
        builder = builder.num_workers(w);
    }

    if let Some(ref cfg) = config_path {
        builder = builder.configuration_path(cfg);
    }

    let orchestrator = builder.build()?;

    // Route to format-specific output
    match format {
        OutputFormat::Jsonl if diff_filter.is_some() => {
            // JSONL with diff filtering: collect results, filter, then output JSONL
            use std::time::Instant;

            use ide::streaming::{DoneEvent, FileEvent, StartEvent};

            let start = Instant::now();
            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let total_files = file_id_vec.len();

            // Print start event
            let start_event = StartEvent::new(total_files);
            println!("{}", serde_json::to_string(&start_event).unwrap());

            let streaming_results =
                orchestrator.analyze_with_progress(file_id_vec, file_set, |_, _| {})?;

            let filter = diff_filter.as_ref().unwrap();
            let mut total_diagnostics = 0;

            for file_result in &streaming_results.file_results {
                if let Some((_, path)) = file_ids.iter().find(|(id, _)| *id == file_result.file_id)
                {
                    let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                    let filtered: Vec<_> = file_result
                        .diagnostics
                        .iter()
                        .filter(|d| {
                            filter.diagnostic_in_diff(
                                rel_path,
                                d.start_line as u32,
                                d.end_line as u32,
                            )
                        })
                        .cloned()
                        .collect();

                    total_diagnostics += filtered.len();
                    let file_event = FileEvent::new(
                        path.display().to_string(),
                        filtered,
                        file_result.metrics.clone(),
                        file_result.error.as_ref().map(|e| e.to_string()),
                    );
                    println!("{}", serde_json::to_string(&file_event).unwrap());
                }
            }

            // Print done event
            let done_event = DoneEvent::new(
                start.elapsed().as_secs_f64(),
                streaming_results.file_results.len(),
                total_diagnostics,
                streaming_results.file_results.iter().filter(|r| r.error.is_some()).count(),
            );
            println!("{}", serde_json::to_string(&done_event).unwrap());

            tracing::info!("JSONL streaming analysis complete (with diff filter)");
        }
        OutputFormat::Jsonl => {
            // JSONL streaming mode - output directly to stdout (no diff filter)
            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let _summary = orchestrator.analyze_jsonl(file_id_vec, file_set)?;
            tracing::info!("JSONL streaming analysis complete");
        }
        OutputFormat::Console => {
            // Standard reporter-based output
            use std::time::Instant;

            use bsl_analyzer::reporters::{AnalysisResults, FileAnalysis, ReporterRegistry};
            use indicatif::{ProgressBar, ProgressStyle};

            let start = Instant::now();
            let total_files = file_ids.len();

            // Setup progress bar (same style as Salsa mode)
            let progress = if !quiet {
                let pb = ProgressBar::new(total_files as u64);
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template(
                            "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}",
                        )
                        .unwrap()
                        .progress_chars("#>-"),
                );
                Some(pb)
            } else {
                None
            };

            // Clone progress for closure
            let progress_clone = progress.clone();
            let start_clone = start;

            // Run analysis with progress callback
            let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
            let streaming_results = orchestrator.analyze_with_progress(
                file_id_vec,
                file_set,
                |processed, _total| {
                    if let Some(ref pb) = progress_clone {
                        pb.set_position(processed as u64);
                        let elapsed_secs = start_clone.elapsed().as_secs_f64();
                        if elapsed_secs > 0.0 {
                            pb.set_message(format!(
                                "{:.0} files/sec",
                                processed as f64 / elapsed_secs
                            ));
                        }
                    }
                },
            )?;

            // Finish progress bar
            if let Some(ref pb) = progress {
                pb.finish_with_message("Analysis complete");
            }

            let elapsed = start.elapsed();

            // Convert streaming results to reporter format
            let mut all_diagnostics = Vec::new();

            for file_result in &streaming_results.file_results {
                if !file_result.diagnostics.is_empty() {
                    if let Some((_, path)) =
                        file_ids.iter().find(|(id, _)| *id == file_result.file_id)
                    {
                        // Filter diagnostics by diff hunks if enabled
                        let filtered_diagnostics = if let Some(ref filter) = diff_filter {
                            let rel_path = path.strip_prefix(&source_dir).unwrap_or(path);
                            file_result
                                .diagnostics
                                .iter()
                                .filter(|d| {
                                    filter.diagnostic_in_diff(
                                        rel_path,
                                        d.start_line as u32,
                                        d.end_line as u32,
                                    )
                                })
                                .cloned()
                                .collect()
                        } else {
                            file_result.diagnostics.clone()
                        };

                        if !filtered_diagnostics.is_empty() {
                            all_diagnostics.push(FileAnalysis {
                                path: path.clone(),
                                relative_path: path
                                    .strip_prefix(&workspace_dir)
                                    .unwrap_or(path)
                                    .to_path_buf(),
                                diagnostics: filtered_diagnostics,
                            });
                        }
                    }
                }
            }

            let results = AnalysisResults {
                files_analyzed: bsl_files.len(),
                files_with_issues: all_diagnostics.len(),
                total_diagnostics: streaming_results.total_diagnostics,
                elapsed_secs: elapsed.as_secs_f64(),
                diagnostics: all_diagnostics,
                source_dir: source_dir.clone(),
                workspace_dir: workspace_dir.clone(),
            };

            // Create output directory if needed
            std::fs::create_dir_all(&output_dir)?;

            // Run reporters
            let registry = ReporterRegistry::new();
            let reporter_keys =
                if reporters.is_empty() { vec!["console".to_string()] } else { reporters };

            for key in &reporter_keys {
                match registry.get(key) {
                    Some(reporter) => {
                        if let Err(e) = reporter.report(&results, &output_dir) {
                            tracing::error!("Reporter '{}' failed: {}", key, e);
                            eprintln!("Error: Reporter '{}' failed: {}", key, e);
                        }
                    }
                    None => {
                        eprintln!("Error: Unknown reporter '{}'", key);
                        eprintln!("Valid reporters: {}", registry.keys().join(", "));
                        return Err(format!("Unknown reporter: {}", key).into());
                    }
                }
            }

            // Print profiling stats if --only-diagnostic was used
            if profiling_enabled {
                if let Some(diag_name) = only_diagnostic {
                    // Collect timings from file_results
                    let timings: Vec<FileTiming> = streaming_results
                        .file_results
                        .iter()
                        .filter_map(|fr| {
                            file_ids.iter().find(|(id, _)| *id == fr.file_id).map(|(_, path)| {
                                FileTiming { path: path.clone(), duration: fr.duration }
                            })
                        })
                        .collect();

                    if let Some(stats) = ProfilingStats::from_timings(diag_name, timings) {
                        stats.print();
                    }
                }
            }

            tracing::info!("Streaming analysis complete");
        }
    }

    Ok(())
}

fn run_format(
    file: PathBuf,
    write: bool,
    spaces: bool,
    indent_size: u32,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::formatting::{format_file, FormattingConfig};
    use std::time::Instant;

    let content = fs::read_to_string(&file)?;
    let file_size = content.len();
    let line_count = content.lines().count();

    eprintln!("Formatting: {:?}", file);
    eprintln!("File size: {} bytes, {} lines", file_size, line_count);

    let start = Instant::now();
    let parsed = parser::parse(&content);
    let parse_time = start.elapsed();
    eprintln!("Parse time: {:?}", parse_time);

    let root = parsed.syntax_node();

    let config = if spaces {
        FormattingConfig::with_spaces(indent_size)
    } else {
        FormattingConfig::default()
    };

    let start = Instant::now();
    let result = format_file(&root, &config);
    let format_time = start.elapsed();
    eprintln!("Format time: {:?}", format_time);
    eprintln!("Total time: {:?}", parse_time + format_time);

    if write {
        fs::write(&file, &result.text)?;
        eprintln!("Written to: {:?}", file);
    } else {
        print!("{}", result.text);
    }

    Ok(())
}

fn check_config(config: std::path::PathBuf) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Checking configuration: {:?}", config);

    let project_config = project_model::ProjectConfig::load_from_file(&config).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse configuration file '{}'; expected a valid bsl-analyzer.toml, .bsl-analyzer.json, or .bsl-language-server.json",
                config.display()
            ),
        )
    })?;
    let diagnostics_config = diagnostics_config_from_project(&project_config)?;
    let diagnostics =
        mcp_server::resolve_project_baseline_diagnostics(config.parent(), &project_config);
    let report =
        build_check_config_report(&config, &project_config, &diagnostics_config, &diagnostics);

    print!("{report}");

    if baseline_diagnostics_have_issues(&diagnostics) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration is invalid: search baseline diagnostics reported errors",
        )
        .into());
    }

    Ok(())
}

fn diagnostics_config_from_project(
    project_config: &project_model::ProjectConfig,
) -> Result<ide::DiagnosticsConfig, Box<dyn Error + Send + Sync>> {
    let locale = project_config.output.resolve_locale().unwrap_or_default();

    if project_config.diagnostics.is_null() {
        return Ok(ide::DiagnosticsConfig { locale, ..Default::default() });
    }

    let mut cfg: ide::DiagnosticsConfig = serde_json::from_value(
        project_config.diagnostics.clone(),
    )
    .map_err(|error| -> Box<dyn Error + Send + Sync> {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse diagnostics section: {error}"),
        )
        .into()
    })?;
    cfg.locale = locale;
    Ok(cfg)
}

fn baseline_diagnostics_have_issues(
    baseline_diagnostics: &mcp_server::BaselineConfigDiagnostics,
) -> bool {
    baseline_diagnostics.workspace.issue.is_some() || baseline_diagnostics.reference.issue.is_some()
}

fn build_check_config_report(
    config_path: &std::path::Path,
    project_config: &project_model::ProjectConfig,
    diagnostics_config: &ide::DiagnosticsConfig,
    baseline_diagnostics: &mcp_server::BaselineConfigDiagnostics,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Configuration is {}.",
        if baseline_diagnostics_have_issues(baseline_diagnostics) { "invalid" } else { "valid" }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Config file: {}", config_path.display());
    let _ = writeln!(
        out,
        "Format: {}",
        match config_path.extension().and_then(|ext| ext.to_str()) {
            Some("toml") => "TOML",
            Some("json") => "JSON",
            _ => "auto",
        }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Project:");
    let _ = writeln!(
        out,
        "  Source root: {}",
        project_config.configuration_root.as_deref().unwrap_or("auto-discovery")
    );
    let _ = writeln!(
        out,
        "  Extensions:  {}",
        if project_config.extensions.is_empty() {
            "none".to_owned()
        } else {
            project_config.extensions.join(", ")
        }
    );
    let _ =
        writeln!(out, "  Language:    {}", project_config.language.as_deref().unwrap_or("default"));
    let _ = writeln!(out);
    let _ = writeln!(out, "Diagnostics:");
    let _ = writeln!(out, "  ordinaryAppSupport: {}", diagnostics_config.ordinary_app_support);
    let _ =
        writeln!(out, "  dataflowMaxIterations: {}", diagnostics_config.dataflow_max_iterations);
    let _ = writeln!(out, "  Disabled:   {}", diagnostics_config.disabled.len());
    let _ = writeln!(out, "  Enabled:    {}", diagnostics_config.enabled.len());
    let _ = writeln!(out, "  Parameters: {}", diagnostics_config.parameters.len());
    let _ = writeln!(
        out,
        "  Disabled codes: {}",
        summarize_diagnostic_codes(diagnostics_config.disabled.iter().map(ToString::to_string))
    );
    let _ = writeln!(
        out,
        "  Explicitly enabled: {}",
        summarize_diagnostic_codes(diagnostics_config.enabled.iter().map(ToString::to_string))
    );
    let _ = writeln!(
        out,
        "  Parameterized: {}",
        summarize_diagnostic_codes(diagnostics_config.parameters.keys().map(ToString::to_string))
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Code lens:");
    let _ = writeln!(
        out,
        "  Cognitive complexity: {}",
        on_off(project_config.code_lens.show_cognitive_complexity)
    );
    let _ = writeln!(
        out,
        "  Cyclomatic complexity: {}",
        on_off(project_config.code_lens.show_cyclomatic_complexity)
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Formatting:");
    let _ = writeln!(out, "  Indent: {}", formatting_summary(&project_config.formatting));
    let _ = writeln!(out);
    let _ = writeln!(out, "Search baseline:");
    append_baseline_summary(&mut out, "Workspace", &baseline_diagnostics.workspace);
    append_baseline_summary(&mut out, "Reference", &baseline_diagnostics.reference);
    out
}

fn append_baseline_summary(
    out: &mut String,
    label: &str,
    summary: &mcp_server::BaselineResolutionSummary,
) {
    let _ = writeln!(out, "  {label}:");
    let _ = writeln!(out, "    Backend: {}", summary.backend);
    let _ = writeln!(out, "    Select:  {}", summary.selection);
    let _ = writeln!(out, "    Status:  {}", summary.issue.as_deref().unwrap_or("ready"));
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn formatting_summary(config: &project_model::FormattingConfig) -> String {
    if config.indent_size == 0 && !config.use_tabs {
        "not configured".to_owned()
    } else if config.use_tabs {
        format!("tabs x{}", config.indent_size)
    } else {
        format!("spaces x{}", config.indent_size)
    }
}

fn summarize_diagnostic_codes(codes: impl Iterator<Item = String>) -> String {
    let mut codes: Vec<String> = codes.collect();
    if codes.is_empty() {
        return "none".to_owned();
    }

    codes.sort();
    const LIMIT: usize = 8;
    if codes.len() <= LIMIT {
        return codes.join(", ");
    }

    let remaining = codes.len() - LIMIT;
    format!("{}, … (+{remaining} more)", codes[..LIMIT].join(", "))
}

fn setup_logging(
    log_file: Option<PathBuf>,
    profile_filter: Option<String>,
    json_profile_filter: Option<String>,
) -> anyhow::Result<()> {
    use tracing_subscriber::fmt::writer::BoxMakeWriter;

    let writer: BoxMakeWriter = match log_file {
        Some(file) => BoxMakeWriter::new(Arc::new(fs::File::create(&file)?)),
        None => BoxMakeWriter::new(io::stderr),
    };

    // Build filter: user filter + suppress noisy Salsa internal logs
    // Salsa logs "new_revision: R85 -> R86" at INFO level on every input change
    let user_filter = env::var("BSL_LOG").ok().unwrap_or_else(|| "warn".to_owned());
    let filter = format!("{},salsa=warn", user_filter);

    bsl_analyzer::tracing::Config { writer, filter, profile_filter, json_profile_filter }.init()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_snapshot_retention, build_check_config_report, check_config,
        diagnostics_config_from_project, pick_first_non_empty, resolve_publish_branch,
        resolve_publish_commit, resolve_snapshot_id, select_parent_snapshot_id,
        select_parent_snapshot_id_from_groups, validate_workspace_publish_policy,
        SnapshotRetentionStatus,
    };
    use bsl_search::{BaselineSnapshotRecord, CorpusId};
    use chrono::{TimeZone, Utc};
    use std::{env, fs};
    use tempfile::tempdir;

    #[test]
    fn explicit_snapshot_id_has_priority() {
        let snapshot_id = resolve_snapshot_id(
            &CorpusId::WorkspaceCode,
            Some("manual-snapshot"),
            Some("main"),
            Some("abc123"),
        )
        .unwrap();

        assert_eq!(snapshot_id, "manual-snapshot");
    }

    #[test]
    fn snapshot_id_is_derived_from_branch_and_commit() {
        let snapshot_id =
            resolve_snapshot_id(&CorpusId::WorkspaceCode, None, Some("main"), Some("abc123"))
                .unwrap();

        assert_eq!(snapshot_id, "workspace-code:main@abc123");
    }

    #[test]
    fn snapshot_id_is_derived_from_commit_when_branch_is_missing() {
        let snapshot_id =
            resolve_snapshot_id(&CorpusId::WorkspaceCode, None, None, Some("abc123")).unwrap();

        assert_eq!(snapshot_id, "workspace-code:abc123");
    }

    #[test]
    fn snapshot_id_from_branch_alone() {
        let snapshot_id =
            resolve_snapshot_id(&CorpusId::WorkspaceCode, None, Some("main"), None).unwrap();

        assert_eq!(snapshot_id, "workspace-code:main");
    }

    #[test]
    fn snapshot_id_requires_branch_or_commit() {
        let error = resolve_snapshot_id(&CorpusId::WorkspaceCode, None, None, None).unwrap_err();

        assert!(error
            .to_string()
            .contains("--snapshot-id is required unless --branch or --commit is provided"));
    }

    #[test]
    fn reference_snapshot_id_defaults_to_package_version() {
        let snapshot_id = resolve_snapshot_id(&CorpusId::Reference, None, None, None).unwrap();

        assert_eq!(snapshot_id, format!("reference:{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn pick_first_non_empty_skips_blank_values() {
        let value = pick_first_non_empty([Some(""), Some("  "), None, Some("main")]);

        assert_eq!(value.as_deref(), Some("main"));
    }

    #[test]
    fn resolve_publish_branch_uses_git_when_cli_and_ci_are_missing() {
        // Clear CI env vars that take priority over git in resolve_publish_branch
        let saved_branch = env::var("CI_COMMIT_BRANCH").ok();
        let saved_ref = env::var("CI_COMMIT_REF_NAME").ok();
        env::remove_var("CI_COMMIT_BRANCH");
        env::remove_var("CI_COMMIT_REF_NAME");

        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();

        let branch = resolve_publish_branch(None, dir.path());

        // Restore env vars
        if let Some(v) = saved_branch {
            env::set_var("CI_COMMIT_BRANCH", v);
        }
        if let Some(v) = saved_ref {
            env::set_var("CI_COMMIT_REF_NAME", v);
        }

        assert_eq!(branch.as_deref(), Some("feature/demo"));
    }

    #[test]
    fn resolve_publish_commit_uses_git_when_cli_and_ci_are_missing() {
        let saved_ci = env::var("CI_COMMIT_SHA").ok();
        let saved_gha = env::var("GITHUB_SHA").ok();
        env::remove_var("CI_COMMIT_SHA");
        env::remove_var("GITHUB_SHA");

        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        let ref_file = git_dir.join("refs/heads/feature/demo");
        fs::create_dir_all(ref_file.parent().unwrap()).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();
        fs::write(&ref_file, "0123456789abcdef\n").unwrap();

        let commit = resolve_publish_commit(None, dir.path());

        if let Some(v) = saved_ci {
            env::set_var("CI_COMMIT_SHA", v);
        }
        if let Some(v) = saved_gha {
            env::set_var("GITHUB_SHA", v);
        }

        assert_eq!(commit.as_deref(), Some("0123456789abcdef"));
    }

    #[test]
    fn workspace_publish_policy_blocks_branch_outside_allowlist() {
        let policy: project_model::SearchBaselinePolicyConfig =
            serde_json::from_value(serde_json::json!({
                "publishBranches": ["vendor", "develop"],
                "branches": [
                    { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
                ]
            }))
            .unwrap();

        let error =
            validate_workspace_publish_policy(&policy, Some("feature/demo"), false).unwrap_err();

        assert!(error.to_string().contains("not allowed by workspace baseline publish policy"));
    }

    #[test]
    fn workspace_publish_policy_allows_override_flag() {
        let policy: project_model::SearchBaselinePolicyConfig =
            serde_json::from_value(serde_json::json!({
                "publishBranches": ["vendor", "develop"],
                "branches": [
                    { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
                ]
            }))
            .unwrap();

        validate_workspace_publish_policy(&policy, Some("feature/demo"), true).unwrap();
    }

    #[test]
    fn check_config_accepts_toml_project_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(
            &config,
            r#"
[source]
root = "src/cf"

[search.baseline]
backend = "postgres"

[search.baseline.postgres]
host = "localhost"
port = 5432
dbname = "bsl_search"
schema = "bsl_search"
vault_role_base = "prod/search/bsl-analyzer"

[search.baseline.postgres.credential_helper]
program = "python3"
args = [
  "-c",
  "import sys; sys.stdin.readline(); sys.stdout.write(sys.argv[1])",
  '{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://user:pass@localhost:5432/bsl_search"}'
]

[search.baseline.workspace_code.policy]
publish_branches = ["develop"]

[[search.baseline.workspace_code.policy.branches]]
match = "*"
select_branch = "develop"
"#,
        )
        .unwrap();

        check_config(config).unwrap();
    }

    #[test]
    fn check_config_accepts_helper_based_json_project_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(".bsl-analyzer.json");
        fs::write(
            &config,
            r#"{
                "search": {
                    "baseline": {
                        "backend": "postgres",
                        "postgres": {
                            "host": "localhost",
                            "port": 5432,
                            "dbname": "bsl_search",
                            "schema": "bsl_search",
                            "vaultRoleBase": "prod/search/bsl-analyzer",
                            "credentialHelper": {
                                "program": "python3",
                                "args": [
                                    "-c",
                                    "import sys; sys.stdin.readline(); sys.stdout.write(sys.argv[1])",
                                    "{\"protocol\":\"bsl-analyzer.postgres-helper.v1\",\"ok\":true,\"url\":\"postgres://user:pass@localhost:5432/bsl_search\"}"
                                ]
                            }
                        },
                        "workspaceCode": {
                            "policy": {
                                "publishBranches": ["develop"],
                                "branches": [
                                    { "match": "*", "selectBranch": "develop" }
                                ]
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        check_config(config).unwrap();
    }

    #[test]
    fn check_config_rejects_invalid_toml() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(&config, "invalid {{{ toml").unwrap();

        let error = check_config(config).unwrap_err();

        assert!(error.to_string().contains("failed to parse configuration file"));
    }

    #[test]
    fn check_config_rejects_invalid_postgres_baseline_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(
            &config,
            r#"
[search.baseline]
backend = "postgres"
"#,
        )
        .unwrap();

        let error = check_config(config).unwrap_err();

        assert!(error.to_string().contains("search baseline diagnostics reported errors"));
    }

    #[test]
    fn diagnostics_section_must_be_object_when_present() {
        let project_config: project_model::ProjectConfig = serde_json::from_str(
            r#"{
                "diagnostics": []
            }"#,
        )
        .unwrap();

        let error = diagnostics_config_from_project(&project_config).unwrap_err();

        assert!(error.to_string().contains("failed to parse diagnostics section"));
    }

    #[test]
    fn check_config_report_contains_project_and_diagnostics_summary() {
        let project_config: project_model::ProjectConfig = serde_json::from_str(
            r#"{
                "configurationRoot": "src/cf",
                "extensions": ["src/cfe/ExtA"],
                "formatting": { "use_tabs": true, "indent_size": 1 },
                "codeLens": {
                    "showCognitiveComplexity": true,
                    "showCyclomaticComplexity": false
                },
                "diagnostics": {
                    "ordinaryAppSupport": true,
                    "dataflowMaxIterations": 20000,
                    "parameters": {
                        "CommentedCode": false,
                        "BadWords": true,
                        "CyclomaticComplexity": { "complexityThreshold": 15 }
                    }
                },
                "search": {
                    "baseline": {
                        "backend": "postgres",
                        "workspaceCode": {
                            "policy": {
                                "publishBranches": ["develop"],
                                "branches": [
                                    { "match": "*", "selectBranch": "develop" }
                                ]
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let diag_config = diagnostics_config_from_project(&project_config).unwrap();
        let baseline = mcp_server::resolve_project_baseline_diagnostics(
            Some(std::path::Path::new(".")),
            &project_config,
        );

        let report = build_check_config_report(
            std::path::Path::new("bsl-analyzer.toml"),
            &project_config,
            &diag_config,
            &baseline,
        );

        assert!(report.contains("Configuration is invalid."));
        assert!(report.contains("Project:"));
        assert!(report.contains("Source root: src/cf"));
        assert!(report.contains("Extensions:  src/cfe/ExtA"));
        assert!(report.contains("ordinaryAppSupport: true"));
        assert!(report.contains("dataflowMaxIterations: 20000"));
        assert!(report.contains("Disabled codes: CommentedCode"));
        assert!(report.contains("Explicitly enabled: BadWords"));
        assert!(report.contains("Parameterized: CyclomaticComplexity"));
        assert!(report.contains("Code lens:"));
        assert!(report.contains("Cognitive complexity: on"));
        assert!(report.contains("Formatting:"));
        assert!(report.contains("Indent: tabs x1"));
        assert!(report.contains("Search baseline:"));
    }

    #[test]
    fn select_parent_snapshot_id_picks_latest_different_snapshot() {
        let snapshots = vec![
            baseline_snapshot_record("workspace-code:main@new"),
            baseline_snapshot_record("workspace-code:main@old"),
        ];

        let parent = select_parent_snapshot_id("workspace-code:main@new", &snapshots);

        assert_eq!(parent.as_deref(), Some("workspace-code:main@old"));
    }

    #[test]
    fn select_parent_snapshot_id_uses_latest_when_current_is_not_published_yet() {
        let snapshots = vec![baseline_snapshot_record("workspace-code:main@old")];

        let parent = select_parent_snapshot_id("workspace-code:main@new", &snapshots);

        assert_eq!(parent.as_deref(), Some("workspace-code:main@old"));
    }

    #[test]
    fn select_parent_snapshot_id_returns_none_when_only_self_exists() {
        let snapshots = vec![baseline_snapshot_record("workspace-code:main@same")];

        let parent = select_parent_snapshot_id("workspace-code:main@same", &snapshots);

        assert_eq!(parent, None);
    }

    #[test]
    fn select_parent_snapshot_id_uses_fallback_branch_group() {
        let feature_group = vec![baseline_snapshot_record("workspace-code:feature@same")];
        let develop_group = vec![baseline_snapshot_record("workspace-code:develop@old")];
        let vendor_group = vec![baseline_snapshot_record("workspace-code:vendor@old")];

        let parent = select_parent_snapshot_id_from_groups(
            "workspace-code:feature@same",
            &[feature_group, develop_group, vendor_group],
        );

        assert_eq!(parent.as_deref(), Some("workspace-code:develop@old"));
    }

    #[test]
    fn analyze_snapshot_retention_marks_vendor_safety_and_descendant_protection() {
        let policy: project_model::SearchBaselinePolicyConfig = serde_json::from_value(serde_json::json!({
            "publishBranches": ["vendor", "develop"],
            "branches": [{ "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }],
            "retention": { "developRetentionDays": 30, "vendorKeepHeads": 2, "minSnapshotsPerBranch": 1 }
        }))
        .unwrap();
        let snapshots = vec![
            snapshot_with_branch(
                "workspace-code:vendor@new",
                "vendor",
                "2026-04-01T00:00:00Z",
                None,
            ),
            snapshot_with_branch(
                "workspace-code:vendor@old",
                "vendor",
                "2026-03-01T00:00:00Z",
                Some("workspace-code:vendor@ancestor"),
            ),
            snapshot_with_branch(
                "workspace-code:vendor@ancestor",
                "vendor",
                "2026-02-01T00:00:00Z",
                None,
            ),
        ];

        let assessments = analyze_snapshot_retention(
            &policy,
            &snapshots,
            Utc.with_ymd_and_hms(2026, 4, 2, 0, 0, 0).unwrap(),
        );

        let new = assessments
            .iter()
            .find(|item| item.snapshot_id == "workspace-code:vendor@new")
            .unwrap();
        assert_eq!(new.status, SnapshotRetentionStatus::ActiveHead);

        let old = assessments
            .iter()
            .find(|item| item.snapshot_id == "workspace-code:vendor@old")
            .unwrap();
        assert_eq!(old.status, SnapshotRetentionStatus::SafetyHead);

        let ancestor = assessments
            .iter()
            .find(|item| item.snapshot_id == "workspace-code:vendor@ancestor")
            .unwrap();
        assert_eq!(ancestor.status, SnapshotRetentionStatus::ExpiredCandidate);
        assert!(ancestor.protections.iter().any(|value| value == "has-descendants"));
    }

    #[test]
    fn analyze_snapshot_retention_marks_recent_develop_snapshot_within_window() {
        let policy: project_model::SearchBaselinePolicyConfig = serde_json::from_value(serde_json::json!({
            "publishBranches": ["vendor", "develop"],
            "branches": [{ "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }],
            "retention": { "developRetentionDays": 30, "vendorKeepHeads": 2, "minSnapshotsPerBranch": 1 }
        }))
        .unwrap();
        let snapshots = vec![
            snapshot_with_branch(
                "workspace-code:develop@new",
                "develop",
                "2026-04-01T00:00:00Z",
                None,
            ),
            snapshot_with_branch(
                "workspace-code:develop@recent",
                "develop",
                "2026-03-20T00:00:00Z",
                None,
            ),
            snapshot_with_branch(
                "workspace-code:develop@old",
                "develop",
                "2026-01-01T00:00:00Z",
                None,
            ),
        ];

        let assessments = analyze_snapshot_retention(
            &policy,
            &snapshots,
            Utc.with_ymd_and_hms(2026, 4, 2, 0, 0, 0).unwrap(),
        );

        let recent = assessments
            .iter()
            .find(|item| item.snapshot_id == "workspace-code:develop@recent")
            .unwrap();
        assert_eq!(recent.status, SnapshotRetentionStatus::WithinWindow);

        let old = assessments
            .iter()
            .find(|item| item.snapshot_id == "workspace-code:develop@old")
            .unwrap();
        assert_eq!(old.status, SnapshotRetentionStatus::ExpiredCandidate);
    }

    fn baseline_snapshot_record(snapshot_id: &str) -> BaselineSnapshotRecord {
        BaselineSnapshotRecord {
            snapshot_id: snapshot_id.to_owned(),
            corpus: "workspace-code".to_owned(),
            fingerprint: None,
            parent_snapshot_id: None,
            branch: Some("main".to_owned()),
            commit: None,
            created_at: "2026-04-02T00:00:00Z".to_owned(),
            files: 0,
            documents: 0,
        }
    }

    fn snapshot_with_branch(
        snapshot_id: &str,
        branch: &str,
        created_at: &str,
        parent_snapshot_id: Option<&str>,
    ) -> BaselineSnapshotRecord {
        BaselineSnapshotRecord {
            snapshot_id: snapshot_id.to_owned(),
            corpus: "workspace-code".to_owned(),
            fingerprint: None,
            parent_snapshot_id: parent_snapshot_id.map(ToOwned::to_owned),
            branch: Some(branch.to_owned()),
            commit: None,
            created_at: created_at.to_owned(),
            files: 0,
            documents: 0,
        }
    }
}
