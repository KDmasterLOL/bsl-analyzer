//! BSL Analyzer - Language Server for 1C:Enterprise BSL.
//!
//! This is the main entry point for the LSP server.

use std::{env, error::Error, fs, io, path::PathBuf, process::Command, sync::Arc};

use clap::{Parser, Subcommand};
use ide_db::metadata;

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
    #[arg(long, global = true)]
    profile: Option<String>,

    /// Enable JSON profiling output (filter syntax: pattern)
    /// Example: '*' outputs timing for all spans as JSON to stderr
    #[arg(long, global = true)]
    profile_json: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run static analysis on a project
    Analyze {
        /// Source directory containing BSL files (default: current directory)
        /// Java-compatible aliases: --srcDir, --src, --project
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
        /// Java-compatible alias: --workspaceDir
        #[arg(short = 'w', long = "workspace-dir", alias = "workspaceDir")]
        workspace_dir: Option<PathBuf>,

        /// Output directory for analysis reports (default: current directory)
        /// Java-compatible alias: --outputDir
        #[arg(short = 'o', long = "output-dir", alias = "outputDir")]
        output_dir: Option<PathBuf>,

        /// Configuration file path (Java-compatible alias: --configuration)
        #[arg(short = 'c', long = "config", alias = "configuration")]
        config: Option<PathBuf>,

        /// Reporters to use (can be specified multiple times or comma-separated)
        /// Valid values: console, json, sarif, tslint, junit, generic, code-quality
        /// (Java-compatible alias: --reporter)
        #[arg(short = 'r', long = "reporters", alias = "reporter", value_delimiter = ',')]
        reporters: Vec<String>,

        /// Silent mode - disable progress bar (Java-compatible alias: --silent)
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

        /// Use streaming mode (low memory, parallel file processing)
        /// Recommended for large projects (>1000 files)
        #[arg(long)]
        streaming: bool,

        /// Number of worker threads for streaming mode (default: CPU cores)
        #[arg(long, requires = "streaming")]
        workers: Option<usize>,
    },

    /// Check configuration file
    CheckConfig {
        /// Path to configuration file
        #[arg(short, long)]
        config: std::path::PathBuf,
    },

    /// Start LSP server (default)
    Lsp,
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
            streaming,
            workers,
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
        ),
        Some(Commands::CheckConfig { config }) => check_config(config),
        Some(Commands::Lsp) | None => run_lsp_server(),
    }
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

    // Join IO threads
    tracing::info!("Joining IO threads");
    io_threads.join()?;

    tracing::info!("LSP server shut down cleanly");
    Ok(())
}

#[allow(clippy::too_many_arguments)] // CLI compatibility with Java bsl-language-server
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
    streaming: bool,
    workers: Option<usize>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Route to streaming or Salsa mode
    if streaming {
        analyze_streaming(
            source_dir,
            workspace_dir,
            output_dir,
            config_path,
            reporters,
            quiet,
            workers,
        )
    } else {
        analyze_salsa(source_dir, workspace_dir, output_dir, config_path, reporters, quiet)
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use base_db::SourceDatabase;
    use ide::{DiagnosticsConfig, RootDatabaseImpl};
    use ide_diagnostics::DiagnosticsContext;
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

    tracing::info!("Analyzing project: {:?}", source_dir);
    tracing::info!("Reporters: {:?}", reporters);
    tracing::info!("Quiet mode: {}", quiet);

    let start = Instant::now();

    // Determine workspace and output directories
    // workspace_dir defaults to source_dir (Java behavior)
    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    // output_dir defaults to current directory "." (Java behavior)
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

    // Load project configuration
    tracing::info!("Loading project configuration");
    let proj_config = if let Some(ref cfg) = config_path {
        project_model::ProjectConfig::load(cfg).unwrap_or_default()
    } else {
        project_model::ProjectConfig::load(&source_dir).unwrap_or_default()
    };

    // Load metadata if available
    let configuration_path = proj_config.configuration_path(&source_dir);
    if let Some(ref cfg_path) = configuration_path {
        tracing::info!("Configuration root: {:?}", cfg_path);

        if cfg_path.exists() {
            tracing::info!("Loading metadata from {:?}", cfg_path);
            let metadata_start = Instant::now();
            match bsl_metadata::load_from_directory(cfg_path) {
                Ok(config) => {
                    let metadata_elapsed = metadata_start.elapsed();
                    tracing::info!(
                        "Metadata loaded in {:.2?} ({} common modules)",
                        metadata_elapsed,
                        config.common_modules().len()
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to load metadata: {}", e);
                }
            }
        } else {
            tracing::warn!("Configuration root not found: {:?}", cfg_path);
        }
    } else {
        tracing::info!("No configuration root specified");
    }

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

    // Build FileSet with all discovered files for VFS path resolution
    tracing::info!("Loading files into database");
    let mut file_set = vfs::FileSet::new();
    let mut file_ids = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        file_ids.push((file_id, path.clone()));
    }

    // Setup source root with FileSet for path resolution
    // (used by sdbl_hir_in_file_query to find configuration root)
    let file_set_arc = Arc::new(file_set);
    let source_root_id = base_db::SourceRootId(0);
    let source_root = base_db::SourceRoot::new_local((*file_set_arc).clone());
    db.set_source_root(source_root_id, source_root);

    // Load file contents into database
    for (file_id, path) in &file_ids {
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
    let config_path_input = configuration_path.as_ref().map(|path| {
        let path_str = path.to_string_lossy().to_string();
        metadata::ConfigurationPathInput::new(&db, path_str)
    });

    // Parallel diagnostics execution WITHOUT Mutex!
    // Use map_with to clone database for each thread worker
    // map_with calls db.clone() once per thread (not per file!)
    // Salsa's clone creates snapshot with new ZalsaLocal (per-thread state)
    let config = Arc::new(DiagnosticsConfig::default());
    let processed = Arc::new(AtomicUsize::new(0));
    let progress_arc = Arc::new(progress);
    let workspace_dir_arc = Arc::new(workspace_dir.clone());
    let source_dir_arc = Arc::new(source_dir.clone());
    let configuration_path_arc = Arc::new(configuration_path);
    // file_set_arc is created above and stored in SourceRoot

    let all_diagnostics: Vec<FileAnalysis> = file_ids
        .par_iter()
        .map_with(db.clone(), |db_snapshot, (file_id, path)| {
            let ctx = DiagnosticsContext {
                db: db_snapshot,
                config: &config,
                file_id: *file_id,
                provider: None,
                workspace_root: Some(&source_dir_arc),
                configuration_path: configuration_path_arc.as_deref(),
                configuration_path_input: config_path_input,
                file_set: Some(&file_set_arc),
            };

            // Catch panics to continue analyzing other files if one fails
            let file_start = std::time::Instant::now();
            let diagnostics =
                match catch_unwind(AssertUnwindSafe(|| ide_diagnostics::diagnostics(&ctx))) {
                    Ok(diags) => {
                        let elapsed = file_start.elapsed();
                        if elapsed.as_millis() > 100 && env::var("BSL_LOG_SLOW_FILES").is_ok() {
                            // Log slow files (only if BSL_LOG_SLOW_FILES is set)
                            tracing::warn!(
                                file = ?path,
                                elapsed_ms = elapsed.as_millis(),
                                "Slow file analysis"
                            );
                        }
                        diags
                    }
                    Err(e) => {
                        tracing::error!("Panic analyzing {:?}: {:?}", path, e);
                        return None;
                    }
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

            // Return only if diagnostics found
            if !diagnostics.is_empty() {
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
                        return None;
                    }
                };

                let diagnostic_outputs: Vec<_> =
                    diagnostics.iter().map(|d| d.to_output(&file_text)).collect();

                Some(FileAnalysis {
                    path: path.clone(),
                    relative_path: path
                        .strip_prefix(&*workspace_dir_arc)
                        .unwrap_or(path)
                        .to_path_buf(),
                    diagnostics: diagnostic_outputs,
                })
            } else {
                None
            }
        })
        .flatten() // flatten Option<FileAnalysis> from map_with
        .collect();

    // Finish progress bar
    if let Some(ref pb) = &*progress_arc {
        pb.finish_with_message("Analysis complete");
    }

    let elapsed = start.elapsed();

    // Create analysis results
    let total_diagnostics: usize = all_diagnostics.iter().map(|f| f.diagnostics.len()).sum();

    let results = AnalysisResults {
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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::streaming::AnalysisOrchestrator;
    use std::time::Instant;
    use vfs::FileId;
    use walkdir::WalkDir;

    use bsl_analyzer::reporters::{AnalysisResults, FileAnalysis, ReporterRegistry};

    let _span = tracing::info_span!("cli_analyze_streaming").entered();

    tracing::info!("Analyzing project (streaming mode): {:?}", source_dir);
    tracing::info!("Workers: {:?}", workers);
    tracing::info!("Reporters: {:?}", reporters);
    tracing::info!("Quiet mode: {}", quiet);

    let start = Instant::now();

    // Determine workspace and output directories
    let workspace_dir = workspace_dir.unwrap_or_else(|| source_dir.clone());
    let output_dir = output_dir.unwrap_or_else(|| PathBuf::from("."));

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
        println!("No BSL files found!");
        return Ok(());
    }

    // Build FileSet and file IDs
    let mut file_set = vfs::FileSet::new();
    let mut file_ids = Vec::new();

    for (idx, path) in bsl_files.iter().enumerate() {
        let file_id = FileId(idx as u32);
        let vfs_path = vfs::VfsPath::new(path.clone());
        file_set.insert(file_id, vfs_path);
        file_ids.push((file_id, path.clone()));
    }

    // Create orchestrator
    let mut builder = AnalysisOrchestrator::builder().workspace_root(&source_dir);

    if let Some(w) = workers {
        builder = builder.num_workers(w);
    }

    if let Some(ref cfg) = config_path {
        builder = builder.configuration_path(cfg);
    }

    let orchestrator = builder.build()?;

    // Show progress message
    if !quiet {
        println!("Analyzing {} files with streaming mode...", file_ids.len());
        if let Some(w) = workers {
            println!("Using {} worker threads", w);
        } else {
            println!(
                "Using {} worker threads (auto-detected)",
                std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
            );
        }
    }

    // Run analysis
    let file_id_vec: Vec<FileId> = file_ids.iter().map(|(id, _)| *id).collect();
    let streaming_results = orchestrator.analyze(file_id_vec, file_set)?;

    let elapsed = start.elapsed();

    if !quiet {
        println!("Analysis completed in {:.2?}", elapsed);
        println!("Processed: {} files", streaming_results.total_files);
        println!("Diagnostics: {}", streaming_results.total_diagnostics);
        println!("Failed: {} files", streaming_results.failed_files);
    }

    // Convert streaming results to reporter format
    // DiagnosticOutput is already in the correct format (DTO from domain layer)
    let mut all_diagnostics = Vec::new();

    for file_result in &streaming_results.file_results {
        if !file_result.diagnostics.is_empty() {
            // Find original path
            if let Some((_, path)) = file_ids.iter().find(|(id, _)| *id == file_result.file_id) {
                all_diagnostics.push(FileAnalysis {
                    path: path.clone(),
                    relative_path: path.strip_prefix(&workspace_dir).unwrap_or(path).to_path_buf(),
                    diagnostics: file_result.diagnostics.clone(),
                });
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
    let reporter_keys = if reporters.is_empty() { vec!["console".to_string()] } else { reporters };

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

    tracing::info!("Streaming analysis complete");

    Ok(())
}

/// Retrieves changed files from git diff.
#[allow(dead_code)] // Will be used in incremental mode implementation
fn get_changed_files_from_git(
    project_root: &PathBuf,
    git_ref: &str,
) -> Result<Vec<PathBuf>, Box<dyn Error + Send + Sync>> {
    let output = Command::new("git")
        .current_dir(project_root)
        .args(["diff", "--name-only", git_ref])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    let files: Vec<PathBuf> = stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter(|line| line.ends_with(".bsl")) // Only BSL files
        .map(|line| project_root.join(line))
        .collect();

    Ok(files)
}

fn check_config(config: std::path::PathBuf) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Checking configuration: {:?}", config);

    let content = std::fs::read_to_string(&config)?;
    let _config: project_model::ProjectConfig = serde_json::from_str(&content)?;

    println!("Configuration is valid!");

    Ok(())
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
