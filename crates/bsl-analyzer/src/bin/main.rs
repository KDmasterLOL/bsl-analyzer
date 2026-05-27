//! BSL Analyzer CLI entry point.
//!
//! Command implementations live in `cli/*`; this file owns the top-level
//! Clap schema and dispatch.

#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod cli;

use std::{env, error::Error, fs, path::PathBuf};

use clap::{Parser, Subcommand};
use cli::{
    analyze::{analyze, OutputFormat},
    check_config::check_config,
    dap::run_dap_server,
    deps::{run_deps, DepsOutputFormat},
    extension::{self, ExtensionCommands},
    format::run_format,
    logging::setup_logging,
    lsp::run_lsp_server,
    mcp::{self, McpCommand},
    rules::{self, RulesCommands},
    search_baseline::{self, SearchCommand},
    smoke::run_smoke,
};

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
        #[arg(short = 'w', long, conflicts_with = "check")]
        write: bool,

        /// Use spaces instead of tabs (default: tabs)
        #[arg(long)]
        spaces: bool,

        /// Number of spaces per indent level (default: 4, only with --spaces)
        #[arg(long, default_value = "4")]
        indent_size: u32,

        /// Check whether the file is already formatted. Exits 0 if yes,
        /// 1 if it would be reformatted. Does not print or write output.
        #[arg(long)]
        check: bool,
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

    /// Measure transitive dependency closure of BSL modules (research tool).
    ///
    /// Loads the workspace exactly like `analyze --salsa` (without LSP / event
    /// loop) and runs a BFS from each sampled root file via
    /// `file_dependencies_query`, printing per-root level sizes and aggregate
    /// percentiles. Used to evaluate lazy/staged warm-up strategies.
    Deps {
        /// Source directory containing BSL files.
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        /// Maximum BFS depth.
        #[arg(short = 'd', long = "depth", default_value = "3")]
        depth: u32,

        /// Sample size; 0 = process every BSL file.
        #[arg(long = "sample", default_value = "200")]
        sample: usize,

        /// Output format.
        #[arg(long = "format", value_enum, default_value_t = DepsOutputFormat::Csv)]
        format: DepsOutputFormat,

        /// Suppress progress bar.
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,

        /// Include `closure_bytes` column (sum of file sizes in closure).
        #[arg(long = "bytes")]
        bytes: bool,

        /// Snapshot VmRSS (Linux only) before/after load and after BFS;
        /// prints to summary.
        #[arg(long = "report-mem")]
        report_mem: bool,

        /// Bench mode: skip BFS, time read+parse+item_tree+module_bodies
        /// for a single file and exit. Ignores `--sample`, `--depth`,
        /// `--format`, `--bytes`. Mutually exclusive with `--multi-open`
        /// and `--bench-index`.
        #[arg(long = "bench", conflicts_with_all = ["multi_open", "bench_index"])]
        bench: Option<PathBuf>,

        /// Replace stride sampling with the listed files (comma-separated).
        /// Per-root closures land in the normal stdout output; their
        /// deduplicated union is reported in the stderr summary.
        #[arg(long = "multi-open", value_delimiter = ',')]
        multi_open: Vec<PathBuf>,

        /// Bench a lexer-only identifier index build over every BSL file
        /// (no parser, no HIR); reports wall-clock, unique names,
        /// (name, file) pairs, and an estimated index size.
        /// Mutually exclusive with `--bench` / `--multi-open`.
        #[arg(long = "bench-index", conflicts_with_all = ["bench", "multi_open"])]
        bench_index: bool,

        /// Override rayon worker count for `--bench-index` parallelism.
        /// Default = rayon's global pool (typically num_cpus).
        #[arg(long = "index-workers")]
        index_workers: Option<usize>,
    },

    /// Cold-start and critical-path performance smoke checks.
    ///
    /// Loads a workspace and runs one or more scenarios (boot,
    /// first_paint, hover, deps), measuring wall-clock and RSS against
    /// configurable budgets.
    Smoke {
        /// Workspace root directory.
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        /// Scenarios to run (comma-separated). Supported names:
        /// `boot`, `first_paint`, `hover`, `deps`.
        #[arg(long = "scenarios", value_delimiter = ',', default_value = "boot")]
        scenarios: Vec<String>,

        /// Optional JSON file overriding the default budget table.
        #[arg(long = "budgets")]
        budgets: Option<PathBuf>,

        /// Emit the report as JSON on stdout instead of human-readable
        /// text. Useful for CI gate scripts.
        #[arg(long = "json")]
        json: bool,
    },
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let cli = Cli::parse();

    let log_file = env::var("BSL_LOG_FILE").ok().map(PathBuf::from);

    let profile_filter = cli.profile.clone().or_else(|| env::var("BSL_PROFILE").ok());
    let json_profile_filter =
        cli.profile_json.clone().or_else(|| env::var("BSL_PROFILE_JSON").ok());

    if let Err(e) = setup_logging(log_file.clone(), profile_filter, json_profile_filter) {
        eprintln!("Failed to setup logging: {}", e);
        if let Some(ref path) = log_file {
            let _ = fs::write(path, format!("ERROR: Failed to setup logging: {}\n", e));
        }
        return Err(e.into());
    }

    tracing::info!("BSL Analyzer starting (pid: {})", std::process::id());
    tracing::info!("Working directory: {:?}", env::current_dir().ok());
    tracing::info!("Command line args: {:?}", env::args().collect::<Vec<_>>());

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
        Some(Commands::Format { file, write, spaces, indent_size, check }) => {
            run_format(file, write, spaces, indent_size, check)
        }
        Some(Commands::Mcp { command }) => mcp::run(command),
        Some(Commands::Extension { command }) => extension::run(command),
        Some(Commands::Dap) => run_dap_server(),
        Some(Commands::Search { command }) => search_baseline::run(command),
        Some(Commands::Rules { command }) => rules::run(command),
        Some(Commands::Deps {
            source_dir,
            depth,
            sample,
            format,
            quiet,
            bytes,
            report_mem,
            bench,
            multi_open,
            bench_index,
            index_workers,
        }) => run_deps(
            source_dir,
            depth,
            sample,
            format,
            quiet,
            bytes,
            report_mem,
            bench,
            multi_open,
            bench_index,
            index_workers,
        ),
        Some(Commands::Smoke { source_dir, scenarios, budgets, json }) => {
            run_smoke(source_dir, scenarios, budgets, json)
        }
        Some(Commands::Lsp) | None => run_lsp_server(),
    }
}
