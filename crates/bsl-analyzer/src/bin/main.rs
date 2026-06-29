#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// jemalloc tuning. Without a background purge thread jemalloc retains freed
// pages as dirty extents and idle RSS stays pinned at the analysis-burst
// high-water mark (measured ~5.7GB resident vs ~0.7GB live on ERP). A background
// thread plus a bounded dirty decay returns those pages to the OS so idle RSS
// tracks live memory. `dirty_decay_ms` is non-zero to keep page reuse cheap on
// hot allocation paths; `muzzy_decay_ms:0` returns muzzy pages promptly.
// `background_thread` is only enabled on linux-gnu, where jemalloc supports it.
#[cfg(not(target_os = "windows"))]
const MALLOC_CONF_BYTES: &[u8] = {
    #[cfg(all(target_os = "linux", not(target_env = "musl")))]
    {
        b"background_thread:true,dirty_decay_ms:1000,muzzy_decay_ms:0\0"
    }
    #[cfg(not(all(target_os = "linux", not(target_env = "musl"))))]
    {
        b"dirty_decay_ms:1000,muzzy_decay_ms:0\0"
    }
};

// jemalloc reads the `malloc_conf` symbol as a C `const char *`. It must be a
// thin pointer (`Option<&c_char>`), not a `&[u8]` fat slice; the union converts
// `&u8 -> &c_char` in const context. `tikv-jemallocator` links it prefixed.
#[cfg(not(target_os = "windows"))]
union MallocConfPtr {
    bytes: &'static u8,
    cchar: &'static core::ffi::c_char,
}

#[cfg(not(target_os = "windows"))]
#[allow(non_upper_case_globals)]
#[export_name = "_rjem_malloc_conf"]
pub static malloc_conf: Option<&'static core::ffi::c_char> =
    Some(unsafe { MallocConfPtr { bytes: &MALLOC_CONF_BYTES[0] }.cchar });

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
    #[arg(long)]
    stdio: bool,

    #[arg(long = "trace-profile", global = true)]
    profile: Option<String>,

    #[arg(long = "trace-profile-json", global = true)]
    profile_json: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant, reason = "CLI command enum should stay unboxed")]
enum Commands {
    Analyze {
        #[arg(
            short = 's',
            long = "source-dir",
            alias = "srcDir",
            alias = "src",
            alias = "project",
            default_value = "."
        )]
        source_dir: PathBuf,

        #[arg(short = 'w', long = "workspace-dir", alias = "workspaceDir")]
        workspace_dir: Option<PathBuf>,

        #[arg(short = 'o', long = "output-dir", alias = "outputDir")]
        output_dir: Option<PathBuf>,

        #[arg(short = 'c', long = "config", alias = "configuration")]
        config: Option<PathBuf>,

        #[arg(short = 'r', long = "reporters", alias = "reporter", value_delimiter = ',')]
        reporters: Vec<String>,

        #[arg(short = 'q', long = "quiet", alias = "silent")]
        quiet: bool,

        #[arg(long)]
        incremental: bool,

        #[arg(long, value_delimiter = ',', requires = "incremental")]
        changed_files: Option<Vec<PathBuf>>,

        #[arg(long, requires = "incremental", conflicts_with = "changed_files")]
        git_diff: Option<String>,

        /// DEPRECATED: run the legacy streaming analyzer instead of Salsa.
        /// Salsa is the default; this flag and the streaming path will be removed.
        #[arg(long)]
        streaming: bool,

        #[arg(long)]
        workers: Option<usize>,

        #[arg(long, value_enum, default_value_t = OutputFormat::Console)]
        format: OutputFormat,

        #[arg(long)]
        only_diagnostic: Option<String>,

        #[arg(long)]
        diff_filter: Option<PathBuf>,
    },

    CheckConfig {
        #[arg(short, long)]
        config: std::path::PathBuf,
    },

    Format {
        file: PathBuf,

        #[arg(short = 'w', long, conflicts_with = "check")]
        write: bool,

        #[arg(long)]
        spaces: bool,

        #[arg(long, default_value = "4")]
        indent_size: u32,

        #[arg(long)]
        check: bool,
    },

    Lsp,

    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },

    Extension {
        #[command(subcommand)]
        command: ExtensionCommands,
    },

    Dap,

    Search {
        #[command(subcommand)]
        command: SearchCommand,
    },

    Rules {
        #[command(subcommand)]
        command: RulesCommands,
    },

    Deps {
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        #[arg(short = 'd', long = "depth", default_value = "3")]
        depth: u32,

        #[arg(long = "sample", default_value = "200")]
        sample: usize,

        #[arg(long = "format", value_enum, default_value_t = DepsOutputFormat::Csv)]
        format: DepsOutputFormat,

        #[arg(short = 'q', long = "quiet")]
        quiet: bool,

        #[arg(long = "bytes")]
        bytes: bool,

        #[arg(long = "report-mem")]
        report_mem: bool,

        #[arg(long = "bench", conflicts_with_all = ["multi_open", "bench_index"])]
        bench: Option<PathBuf>,

        #[arg(long = "multi-open", value_delimiter = ',')]
        multi_open: Vec<PathBuf>,

        #[arg(long = "bench-index", conflicts_with_all = ["bench", "multi_open"])]
        bench_index: bool,

        #[arg(long = "index-workers")]
        index_workers: Option<usize>,
    },

    Smoke {
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        #[arg(long = "scenarios", value_delimiter = ',', default_value = "boot")]
        scenarios: Vec<String>,

        #[arg(long = "budgets")]
        budgets: Option<PathBuf>,

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
            streaming,
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
            streaming,
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
