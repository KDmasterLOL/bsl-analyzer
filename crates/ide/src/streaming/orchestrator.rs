//! Analysis orchestrator for coordinating batch file analysis.
//!
//! This module provides `AnalysisOrchestrator` - the high-level coordinator
//! for parallel file analysis in streaming mode. It:
//!
//! 1. Initializes GlobalContext (loads configuration, builds symbol trees)
//! 2. Creates SharedState for worker coordination
//! 3. Spawns worker pool for parallel processing
//! 4. Aggregates results from all workers
//! 5. Reports progress to caller
//!
//! ## Architecture
//!
//! ```text
//! AnalysisOrchestrator
//!   │
//!   ├─> Phase 1: Initialization
//!   │   ├─> Load Configuration (.json)
//!   │   ├─> Build all SymbolTrees (Phase 1 only)
//!   │   ├─> Build WorkspaceSymbols
//!   │   └─> Build ModuleIndex
//!   │
//!   ├─> Phase 2: Worker Pool
//!   │   ├─> Create SharedState
//!   │   ├─> Sort files by priority
//!   │   └─> Spawn N workers
//!   │
//!   └─> Phase 3: Result Aggregation
//!       ├─> Collect FileResults via channel
//!       └─> Return AnalysisResults
//! ```
//!
//! ## Example
//!
//! ```ignore
//! let orchestrator = AnalysisOrchestrator::builder()
//!     .workspace_root("~/src/myproject")
//!     .configuration_path(".bsl-analyzer.json")
//!     .num_workers(4)
//!     .build()?;
//!
//! let results = orchestrator.analyze(files)?;
//! println!("Processed {} files, found {} diagnostics",
//!     results.total_files,
//!     results.total_diagnostics
//! );
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::unbounded;
use rustc_hash::FxHashMap;
use tracing::{debug, error, info, warn};
use vfs::{file_set::FileSet, FileId};

use hir::{ItemTree, ModuleId, SymbolTree, WorkspaceSymbols};

// Import from ide-db (infrastructure layer)
use ide_db::streaming::{FileReader, GlobalContext, SharedState, StreamingProvider};

// Import from ide-diagnostics
use ide_diagnostics::DiagnosticsConfig;

// Import from current crate (ide features layer)
use super::file_processor::FileResult;
use super::worker_thread::worker_main;

/// Results of batch analysis.
#[derive(Debug, Clone)]
pub struct AnalysisResults {
    /// Total number of files processed.
    pub total_files: usize,

    /// Total number of diagnostics found.
    pub total_diagnostics: usize,

    /// Number of files with errors.
    pub failed_files: usize,

    /// Per-file results.
    pub file_results: Vec<FileResult>,
}

/// Errors that can occur during orchestration.
#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Worker pool error: {0}")]
    WorkerPool(String),

    #[error("No files to analyze")]
    NoFiles,
}

/// Orchestrator for coordinating batch file analysis.
///
/// This is the main entry point for streaming mode analysis.
/// It manages the entire lifecycle:
///
/// 1. **Initialization**: Load configuration, build global data structures
/// 2. **Execution**: Spawn worker pool, process files in parallel
/// 3. **Aggregation**: Collect results, compute statistics
///
/// ## Memory Target
///
/// For 25K files, target peak memory: ~335 MB
/// - GlobalContext: ~335 MB (configuration + symbol trees)
/// - SharedState: ~1 MB (file statuses + coordination)
/// - Workers: minimal (streaming, no caching)
pub struct AnalysisOrchestrator {
    /// Number of worker threads to spawn.
    num_workers: usize,

    /// Workspace root directory.
    workspace_root: PathBuf,

    /// Optional path to configuration file (.bsl-analyzer.json).
    configuration_path: Option<PathBuf>,

    /// Pre-configured diagnostics config (overrides config file).
    /// Set via builder.diagnostics_config() method.
    diagnostics_config: Option<DiagnosticsConfig>,
}

impl AnalysisOrchestrator {
    /// Create a new orchestrator with builder pattern.
    pub fn builder() -> OrchestratorBuilder {
        OrchestratorBuilder::default()
    }

    /// Perform batch analysis of all files.
    ///
    /// This is the main entry point for streaming mode.
    ///
    /// ## Phases
    ///
    /// 1. **Initialization** (~10-20% of time): Build GlobalContext
    /// 2. **Processing** (~70-80% of time): Parallel file processing
    /// 3. **Aggregation** (~1-5% of time): Collect and summarize results
    ///
    /// ## Parameters
    ///
    /// - `files`: List of FileIds to process
    /// - `file_set`: VFS file set with paths
    ///
    /// ## Returns
    ///
    /// `AnalysisResults` with per-file diagnostics and statistics.
    ///
    /// ## Errors
    ///
    /// Returns error if:
    /// - Configuration file not found or invalid
    /// - No files to analyze
    /// - Worker pool spawn fails
    pub fn analyze(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
    ) -> Result<AnalysisResults, OrchestratorError> {
        let _span = tracing::info_span!("orchestrator_analyze", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        info!(num_files = files.len(), num_workers = self.num_workers, "Starting batch analysis");

        // Phase 1: Initialization
        let global_context = self.initialize_global_context(&files, file_set)?;

        // Phase 2: Create SharedState and spawn workers
        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files.clone());
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        // Load diagnostics configuration from project config file
        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state, config, results_tx)?;

        // Phase 3: Collect results
        let results = self.collect_results(results_rx, workers, sorted_files.len());

        info!(
            total_files = results.total_files,
            total_diagnostics = results.total_diagnostics,
            failed_files = results.failed_files,
            "Batch analysis completed"
        );

        Ok(results)
    }

    /// Perform batch analysis with progress callback.
    ///
    /// Similar to `analyze()`, but calls the progress callback after each file is processed.
    /// This allows showing a progress bar or other UI feedback.
    ///
    /// ## Parameters
    ///
    /// - `files`: List of FileIds to process
    /// - `file_set`: VFS file set with paths
    /// - `on_progress`: Callback called with (processed_count, total_count) after each file
    pub fn analyze_with_progress<F>(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
        mut on_progress: F,
    ) -> Result<AnalysisResults, OrchestratorError>
    where
        F: FnMut(usize, usize),
    {
        let _span = tracing::info_span!("orchestrator_analyze", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        let total_files = files.len();
        info!(num_files = total_files, num_workers = self.num_workers, "Starting batch analysis");

        // Phase 1: Initialization
        let global_context = self.initialize_global_context(&files, file_set)?;

        // Phase 2: Create SharedState and spawn workers
        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files.clone());
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        // Load diagnostics configuration from project config file
        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state, config, results_tx)?;

        // Phase 3: Collect results with progress callback
        let results =
            self.collect_results_with_progress(results_rx, workers, total_files, &mut on_progress);

        info!(
            total_files = results.total_files,
            total_diagnostics = results.total_diagnostics,
            failed_files = results.failed_files,
            "Batch analysis completed"
        );

        Ok(results)
    }

    /// Phase 1: Initialize GlobalContext.
    ///
    /// This phase loads configuration and builds all shared data structures:
    /// - Configuration (if present)
    /// - All SymbolTrees (Phase 1 only - no diagnostics yet)
    /// - WorkspaceSymbols index
    /// - ModuleIndex (name → FileId mapping)
    ///
    /// ## Memory Impact
    ///
    /// This is the largest memory allocation (~335 MB for 25K files):
    /// - Configuration: ~31 MB (1C metadata)
    /// - SymbolTrees: ~292 MB (all modules)
    /// - WorkspaceSymbols: ~5 MB
    /// - ModuleIndex: ~5 MB
    ///
    /// ## Performance
    ///
    /// Single-threaded for simplicity. Could be parallelized in future,
    /// but initialization is only ~10-20% of total time.
    fn initialize_global_context(
        &self,
        files: &[FileId],
        file_set: FileSet,
    ) -> Result<Arc<GlobalContext>, OrchestratorError> {
        let _span = tracing::info_span!("initialize_global_context").entered();

        info!(num_files = files.len(), "Building GlobalContext");

        // 1. Load 1C metadata (Configuration.xml, CommonModules, etc.)
        let (configuration, config_root) = self.load_metadata();

        // 2. Create FileReader
        let file_set_arc = Arc::new(file_set.clone());
        let file_reader = FileReader::from_disk(self.workspace_root.clone(), file_set_arc.clone());

        // 3. Build all SymbolTrees in parallel
        // Build for ALL BSL files in file_set (not just files to analyze),
        // because cross-module diagnostics need SymbolTrees from other modules.
        // Filter out non-BSL entries — feeding XML metadata files to the BSL
        // parser is wasted work and produces empty ItemTrees.
        use rayon::prelude::*;
        let all_file_ids: Vec<FileId> =
            file_set.iter().filter(|&file_id| hir::is_bsl_source(&file_set, file_id)).collect();
        info!(num_files = all_file_ids.len(), "Building SymbolTrees");

        let symbol_trees: FxHashMap<_, _> = all_file_ids
            .par_iter()
            .map(|&file_id| {
                let text = file_reader.read(file_id).unwrap_or_default();
                let parse = parser::parse(&text);
                let item_tree = ItemTree::from_parse(&parse);
                let module_id = ModuleId::new(file_id);
                let symbol_tree =
                    Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));
                (file_id, symbol_tree)
            })
            .collect();

        info!(num_symbol_trees = symbol_trees.len(), "SymbolTrees built successfully");

        // 4. Build WorkspaceSymbols
        // TODO: Implement proper WorkspaceSymbols building from symbol trees
        let workspace_symbols = Arc::new(WorkspaceSymbols::default());
        info!("WorkspaceSymbols built (empty for now)");

        // 5. Build ModuleIndex from file paths (no parsing required)
        // Use all files from file_set for complete cross-module resolution.
        let module_index = {
            let paths = all_file_ids.iter().filter_map(|&file_id| {
                let vfs_path = file_set.path_for_file(&file_id)?;
                let path_str = vfs_path.as_path().to_str()?;
                Some((file_id, path_str))
            });
            Arc::new(hir::ModuleIndex::build_from_paths(paths))
        };
        info!(
            common_modules = module_index.common_module_count(),
            managers = module_index.manager_count(),
            "ModuleIndex built"
        );

        Ok(Arc::new(GlobalContext {
            configuration,
            symbol_trees,
            workspace_symbols,
            module_index,
            file_set: file_set_arc,
            file_reader,
            config_root,
        }))
    }

    /// Sort files by priority to minimize wait times.
    ///
    /// ## Priority Order (highest to lowest)
    ///
    /// 1. **CommonModule (Server)** - Used by everything
    /// 2. **CommonModule (ServerCall)** - Used by client/server
    /// 3. **CommonModule (ClientServer)** - Used by both sides
    /// 4. **CommonModule (Client)** - Used by client modules
    /// 5. **ManagerModule** - Used by forms
    /// 6. **ObjectModule** - Minimal dependencies
    /// 7. **FormModule** - Depends on manager/object
    ///
    /// ## Impact
    ///
    /// With proper sorting:
    /// - <1% of files cause waits (most SymbolTrees already published)
    /// - Average wait time: <50μs
    ///
    /// Without sorting:
    /// - ~10% of files cause waits
    /// - More contention on condvars
    fn sort_files_by_priority(
        &self,
        files: Vec<FileId>,
        global_context: &Arc<GlobalContext>,
    ) -> Vec<FileId> {
        let _span =
            tracing::info_span!("sort_files_by_priority", num_files = files.len()).entered();

        let configuration = global_context.configuration.as_ref();
        let file_set = &global_context.file_set;
        let workspace_root = &self.workspace_root;

        let mut files_with_priority: Vec<(FileId, u8, u64)> = files
            .into_iter()
            .map(|file_id| {
                let vfs_path = file_set.path_for_file(&file_id);
                let priority = vfs_path
                    .map(|p| super::file_priority::compute_priority(p.as_path(), configuration))
                    .unwrap_or(super::file_priority::priority::OTHER);

                let file_size = vfs_path
                    .and_then(|p| {
                        let path = if p.as_path().is_absolute() {
                            p.as_path().to_path_buf()
                        } else {
                            workspace_root.join(p.as_path())
                        };
                        std::fs::metadata(&path).ok().map(|m| m.len())
                    })
                    .unwrap_or(0);

                (file_id, priority, file_size)
            })
            .collect();

        // Primary: priority ASC (module type), secondary: file size DESC (large files first)
        files_with_priority.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| b.2.cmp(&a.2)));

        if tracing::enabled!(tracing::Level::DEBUG) {
            let mut counts = [0usize; 8];
            for &(_, priority, _) in &files_with_priority {
                if (priority as usize) < counts.len() {
                    counts[priority as usize] += 1;
                }
            }
            debug!(
                server = counts[0],
                server_call = counts[1],
                client_server = counts[2],
                client = counts[3],
                manager = counts[4],
                object = counts[5],
                form = counts[6],
                other = counts[7],
                "file priority distribution"
            );
        }

        files_with_priority.into_iter().map(|(file_id, _, _)| file_id).collect()
    }

    /// Load 1C metadata via ProjectConfig.
    /// Returns (Configuration, config_root_path) tuple.
    fn load_metadata(&self) -> (Option<Arc<bsl_metadata::Configuration>>, Option<PathBuf>) {
        let proj_config = if let Some(ref path) = self.configuration_path {
            project_model::ProjectConfig::load_from_file(path)
        } else {
            project_model::ProjectConfig::load(&self.workspace_root)
        };

        let Some(proj_config) = proj_config else {
            return (None, None);
        };

        let config_root = proj_config.configuration_path(&self.workspace_root);
        let metadata = proj_config.load_metadata(&self.workspace_root).map(Arc::new);
        (metadata, config_root)
    }

    /// Load diagnostics configuration.
    ///
    /// If a pre-configured config was set via builder.diagnostics_config(),
    /// returns that. Otherwise loads from project config file.
    ///
    /// Uses `project_model::ProjectConfig` to load configuration,
    /// then deserializes the `diagnostics` field into `DiagnosticsConfig`.
    ///
    /// ## Config Format (bsl-language-server compatible)
    ///
    /// ```json
    /// {
    ///   "diagnostics": {
    ///     "ordinaryAppSupport": false,
    ///     "dataflowMaxIterations": 10000,
    ///     "parameters": {
    ///       "EmptyCodeBlock": false,
    ///       "LineLength": { "maxLength": 120 }
    ///     }
    ///   }
    /// }
    /// ```
    fn load_diagnostics_config(&self) -> DiagnosticsConfig {
        // Use pre-configured config if set (e.g., from CLI flags)
        if let Some(ref config) = self.diagnostics_config {
            info!("Using pre-configured DiagnosticsConfig");
            return config.clone();
        }

        // Load project config via project_model (unified entry point)
        let proj_config = if let Some(ref path) = self.configuration_path {
            project_model::ProjectConfig::load_from_file(path)
        } else {
            project_model::ProjectConfig::load(&self.workspace_root)
        };

        let Some(proj_config) = proj_config else {
            info!("No config file found, using default DiagnosticsConfig");
            return DiagnosticsConfig::default();
        };

        // Apply `[output] display_language` so CLI / streaming output is
        // localized identically to the LSP path. The streaming pipeline
        // does not see an LSP `InitializeParams.locale`, so the project
        // setting and the analyzer default are the only signals — with
        // no project preference, `Locale::default()` (= Ru) wins.
        let output_locale = proj_config.output.resolve_locale().unwrap_or_default();

        // Deserialize diagnostics field into DiagnosticsConfig
        match serde_json::from_value::<DiagnosticsConfig>(proj_config.diagnostics) {
            Ok(mut config) => {
                info!("Loaded diagnostics config from project file");
                config.locale = output_locale;
                config
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse diagnostics config, using default");
                DiagnosticsConfig { locale: output_locale, ..Default::default() }
            }
        }
    }

    /// Spawn worker pool.
    ///
    /// Spawns `num_workers` threads, each running `worker_main()`.
    ///
    /// ## Worker Configuration
    ///
    /// - Each worker gets its own thread
    /// - All workers share same SharedState (Arc)
    /// - All workers share same StreamingProvider (Arc)
    /// - Results sent via crossbeam channel
    ///
    /// ## Error Handling
    ///
    /// - Thread spawn failures are fatal (return error)
    /// - Worker panics are caught and converted to error results
    fn spawn_workers(
        &self,
        provider: Arc<StreamingProvider>,
        shared_state: Arc<SharedState>,
        config: Arc<DiagnosticsConfig>,
        results_tx: crossbeam_channel::Sender<FileResult>,
    ) -> Result<Vec<thread::JoinHandle<()>>, OrchestratorError> {
        let _span = tracing::info_span!("spawn_workers").entered();

        info!(num_workers = self.num_workers, "Spawning worker pool");

        let mut handles = Vec::with_capacity(self.num_workers);

        for worker_id in 0..self.num_workers {
            let provider = Arc::clone(&provider);
            let shared_state = Arc::clone(&shared_state);
            let config = Arc::clone(&config);
            let results_tx = results_tx.clone();

            let handle = thread::Builder::new()
                .name(format!("worker-{}", worker_id))
                .spawn(move || {
                    worker_main(worker_id, shared_state, provider, config, results_tx);
                })
                .map_err(|e| {
                    OrchestratorError::WorkerPool(format!(
                        "Failed to spawn worker {}: {}",
                        worker_id, e
                    ))
                })?;

            handles.push(handle);
        }

        Ok(handles)
    }

    /// Collect results from all workers.
    ///
    /// This function:
    /// 1. Iterates over results channel until all workers finish
    /// 2. Aggregates statistics (total diagnostics, failed files, etc.)
    /// 3. Waits for all worker threads to exit
    ///
    /// ## Termination
    ///
    /// Channel closes when:
    /// - All workers finish processing and drop their Sender
    /// - We explicitly drop our Sender before iteration
    ///
    /// Then we wait for all threads to exit gracefully.
    fn collect_results(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        expected_files: usize,
    ) -> AnalysisResults {
        let _span = tracing::info_span!("collect_results").entered();

        info!(expected_files, "Collecting results");

        let mut file_results = Vec::with_capacity(expected_files);
        let mut total_diagnostics = 0;
        let mut failed_files = 0;

        // Collect all results
        for result in results_rx {
            if result.error.is_some() {
                failed_files += 1;
            }

            total_diagnostics += result.diagnostics.len();
            file_results.push(result);
        }

        info!(collected_files = file_results.len(), expected_files, "All results collected");

        // Wait for all workers to exit
        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        AnalysisResults {
            total_files: file_results.len(),
            total_diagnostics,
            failed_files,
            file_results,
        }
    }

    /// Collect results with progress callback.
    ///
    /// Same as `collect_results`, but calls `on_progress(processed, total)` after each file.
    fn collect_results_with_progress<F>(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        expected_files: usize,
        on_progress: &mut F,
    ) -> AnalysisResults
    where
        F: FnMut(usize, usize),
    {
        let _span = tracing::info_span!("collect_results_with_progress").entered();

        info!(expected_files, "Collecting results with progress");

        let mut file_results = Vec::with_capacity(expected_files);
        let mut total_diagnostics = 0;
        let mut failed_files = 0;

        // Collect all results with progress callback
        for result in results_rx {
            if result.error.is_some() {
                failed_files += 1;
            }

            total_diagnostics += result.diagnostics.len();
            file_results.push(result);

            // Call progress callback
            on_progress(file_results.len(), expected_files);
        }

        info!(collected_files = file_results.len(), expected_files, "All results collected");

        // Wait for all workers to exit
        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        AnalysisResults {
            total_files: file_results.len(),
            total_diagnostics,
            failed_files,
            file_results,
        }
    }

    /// Perform batch analysis with JSON Lines streaming output.
    ///
    /// Unlike `analyze()`, this method streams results directly to stdout
    /// in JSON Lines format as they are received, without accumulating them
    /// in memory. This is designed for SonarQube integration and large codebases.
    ///
    /// ## Output Format
    ///
    /// ```jsonl
    /// {"type":"start","total_files":6540,"version":"0.2.0"}
    /// {"type":"file","path":"src/Module.bsl","diagnostics":[...],"metrics":{...}}
    /// {"type":"done","elapsed_secs":11.2,"total_files":6540,"total_diagnostics":1234,"failed_files":0}
    /// ```
    ///
    /// ## Parameters
    ///
    /// - `files`: List of FileIds to process
    /// - `file_set`: VFS file set with paths
    ///
    /// ## Returns
    ///
    /// `JsonlSummary` with statistics (for programmatic access).
    pub fn analyze_jsonl(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
    ) -> Result<super::jsonl::JsonlSummary, OrchestratorError> {
        use std::time::Instant;

        use super::jsonl::{DoneEvent, StartEvent};

        let _span =
            tracing::info_span!("orchestrator_analyze_jsonl", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        let start = Instant::now();
        info!(
            num_files = files.len(),
            num_workers = self.num_workers,
            "Starting JSONL streaming analysis"
        );

        // Print start event
        let start_event = StartEvent::new(files.len());
        println!(
            "{}",
            serde_json::to_string(&start_event).expect("StartEvent serialization failed")
        );

        // Phase 1: Initialization
        let global_context = self.initialize_global_context(&files, file_set.clone())?;

        // Phase 2: Create SharedState and spawn workers
        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files);
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        // Load diagnostics configuration from project config file
        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state.clone(), config, results_tx)?;

        // Phase 3: Stream results
        let summary = self.stream_jsonl_results(results_rx, workers, &file_set);

        // Print done event
        let done_event = DoneEvent::new(
            start.elapsed().as_secs_f64(),
            summary.total_files,
            summary.total_diagnostics,
            summary.failed_files,
        );
        println!("{}", serde_json::to_string(&done_event).expect("DoneEvent serialization failed"));

        info!(
            total_files = summary.total_files,
            total_diagnostics = summary.total_diagnostics,
            failed_files = summary.failed_files,
            elapsed_secs = start.elapsed().as_secs_f64(),
            "JSONL streaming analysis completed"
        );

        Ok(summary)
    }

    /// Stream results as JSON Lines.
    ///
    /// This method receives results from workers and immediately outputs them
    /// as JSON Lines to stdout without accumulating in memory.
    fn stream_jsonl_results(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        file_set: &FileSet,
    ) -> super::jsonl::JsonlSummary {
        use super::jsonl::{FileEvent, JsonlSummary};

        let _span = tracing::info_span!("stream_jsonl_results").entered();

        let mut total_diagnostics = 0;
        let mut total_files = 0;
        let mut failed_files = 0;

        // Stream results as they arrive
        for result in results_rx {
            // Get path from file_set
            let path = file_set
                .path_for_file(&result.file_id)
                .map(|p| p.as_path().display().to_string())
                .unwrap_or_else(|| format!("file:{}", result.file_id.index()));

            let file_event = FileEvent::new(
                path,
                result.diagnostics.clone(),
                result.metrics.clone(),
                result.error.as_ref().map(|e| e.to_string()),
            );

            // Output immediately
            println!(
                "{}",
                serde_json::to_string(&file_event).expect("FileEvent serialization failed")
            );

            total_diagnostics += result.diagnostics.len();
            total_files += 1;
            if result.error.is_some() {
                failed_files += 1;
            }
        }

        info!(total_files, "All JSONL results streamed");

        // Wait for all workers to exit
        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        JsonlSummary { total_files, total_diagnostics, failed_files }
    }
}

/// Builder for constructing `AnalysisOrchestrator`.
#[derive(Default)]
pub struct OrchestratorBuilder {
    num_workers: Option<usize>,
    workspace_root: Option<PathBuf>,
    configuration_path: Option<PathBuf>,
    diagnostics_config: Option<DiagnosticsConfig>,
}

impl OrchestratorBuilder {
    /// Set number of worker threads.
    ///
    /// Default: number of CPU cores.
    pub fn num_workers(mut self, n: usize) -> Self {
        self.num_workers = Some(n);
        self
    }

    /// Set workspace root directory.
    pub fn workspace_root<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.workspace_root = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set configuration file path.
    pub fn configuration_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.configuration_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set pre-configured diagnostics config.
    ///
    /// This overrides the config loaded from the configuration file.
    /// Useful for applying CLI filters (--only-diagnostic, --disable-diagnostic).
    pub fn diagnostics_config(mut self, config: DiagnosticsConfig) -> Self {
        self.diagnostics_config = Some(config);
        self
    }

    /// Build the orchestrator.
    pub fn build(self) -> Result<AnalysisOrchestrator, OrchestratorError> {
        let workspace_root = self
            .workspace_root
            .ok_or_else(|| OrchestratorError::Configuration("workspace_root is required".into()))?;

        let num_workers = self.num_workers.unwrap_or_else(|| {
            let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            info!(num_workers = cpus, "Using default worker count (CPU cores)");
            cpus
        });

        Ok(AnalysisOrchestrator {
            num_workers,
            workspace_root,
            configuration_path: self.configuration_path,
            diagnostics_config: self.diagnostics_config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use vfs::VfsPath;

    fn create_test_workspace(files: Vec<(&str, &str)>) -> (TempDir, Vec<FileId>, FileSet) {
        let temp_dir = TempDir::new().unwrap();

        let mut file_set = FileSet::default();
        let mut file_ids = Vec::new();

        for (idx, (name, content)) in files.iter().enumerate() {
            let file_path = temp_dir.path().join(name);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&file_path, content).unwrap();

            let file_id = FileId(idx as u32);
            let vfs_path = VfsPath::new(file_path.to_str().unwrap());
            file_set.insert(file_id, vfs_path);
            file_ids.push(file_id);
        }

        (temp_dir, file_ids, file_set)
    }

    #[test]
    fn test_orchestrator_builder() {
        let result =
            AnalysisOrchestrator::builder().workspace_root("/tmp/test").num_workers(2).build();

        assert!(result.is_ok());
        let orchestrator = result.unwrap();
        assert_eq!(orchestrator.num_workers, 2);
        assert_eq!(orchestrator.workspace_root, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_orchestrator_builder_default_workers() {
        let result = AnalysisOrchestrator::builder().workspace_root("/tmp/test").build();

        assert!(result.is_ok());
        let orchestrator = result.unwrap();
        // Should use CPU count
        assert!(orchestrator.num_workers > 0);
    }

    #[test]
    fn test_orchestrator_builder_missing_workspace() {
        let result = AnalysisOrchestrator::builder().num_workers(2).build();

        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::Configuration(_))));
    }

    #[test]
    fn test_analyze_empty_files() {
        let temp_dir = TempDir::new().unwrap();
        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let file_set = FileSet::default();
        let result = orchestrator.analyze(vec![], file_set);

        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::NoFiles)));
    }

    #[test]
    fn test_analyze_single_file() {
        let (_temp_dir, file_ids, file_set) =
            create_test_workspace(vec![("test.bsl", "Процедура Тест() КонецПроцедуры")]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 1);
        assert_eq!(results.failed_files, 0);
        // Diagnostics count depends on enabled rules - just verify analysis completed
    }

    #[test]
    fn test_analyze_multiple_files() {
        let (_temp_dir, file_ids, file_set) = create_test_workspace(vec![
            ("module1.bsl", "Процедура А() КонецПроцедуры"),
            ("module2.bsl", "Функция Б() КонецФункции"),
            ("module3.bsl", "Процедура В() КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(2)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 3);
        assert_eq!(results.failed_files, 0);
    }

    #[test]
    fn test_analyze_with_parse_errors() {
        let (_temp_dir, file_ids, file_set) = create_test_workspace(vec![
            ("valid.bsl", "Процедура Тест() КонецПроцедуры"),
            ("invalid.bsl", "Процедура Ошибка( КонецПроцедуры"), // Missing )
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        // Should succeed despite parse errors (graceful degradation)
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 2);
    }

    #[test]
    fn test_analyze_concurrent_workers() {
        // Create 10 files
        let files = (0..10)
            .map(|i| (format!("module{}.bsl", i), format!("Процедура Метод{}() КонецПроцедуры", i)))
            .collect::<Vec<_>>();

        let files_ref: Vec<_> = files.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();

        let (_temp_dir, file_ids, file_set) = create_test_workspace(files_ref);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(4) // 4 workers for 10 files
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 10);
        assert_eq!(results.failed_files, 0);
    }

    /// Regression test: when diff-filter is active, only a subset of files is
    /// passed for diagnostic analysis, but ALL files must be in file_set so that
    /// cross-module resolution (SymbolTrees, ModuleIndex) works correctly.
    ///
    /// This simulates the scenario where module A (in diff) calls a method from
    /// module B (not in diff). Module B must have its SymbolTree built so that
    /// cross-module diagnostics like MissedRequiredParameter can resolve it.
    #[test]
    fn test_analyze_subset_with_full_file_set() {
        // Create 3 files: all go into file_set, but only file 0 is analyzed
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("analyzed.bsl", "Процедура Тест() КонецПроцедуры"),
            ("dep1.bsl", "Процедура Зависимость1() Экспорт КонецПроцедуры"),
            ("dep2.bsl", "Функция Зависимость2() Экспорт Возврат 1; КонецФункции"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        // Only analyze first file, but file_set contains ALL files
        let files_to_analyze = vec![all_file_ids[0]];
        let result = orchestrator.analyze(files_to_analyze, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        // Only 1 file should be analyzed for diagnostics
        assert_eq!(results.total_files, 1);
        assert_eq!(results.failed_files, 0);
    }

    /// Regression test: GlobalContext must build SymbolTrees for ALL files
    /// in file_set, not just the files passed for analysis.
    /// Without this, cross-module diagnostics silently skip checks.
    #[test]
    fn test_global_context_builds_symbol_trees_for_all_files() {
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("analyzed.bsl", "Процедура Анализируемый() КонецПроцедуры"),
            ("dependency.bsl", "Процедура Зависимость(Парам1) Экспорт КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        // Only analyze first file
        let files_to_analyze = vec![all_file_ids[0]];

        // Use initialize_global_context directly to verify SymbolTrees
        let global_context =
            orchestrator.initialize_global_context(&files_to_analyze, file_set).unwrap();

        // SymbolTrees must be built for ALL files in file_set, not just analyzed files
        assert!(
            global_context.symbol_trees.contains_key(&all_file_ids[0]),
            "SymbolTree for analyzed file must exist"
        );
        assert!(
            global_context.symbol_trees.contains_key(&all_file_ids[1]),
            "SymbolTree for dependency file must exist (cross-module resolution)"
        );
        assert_eq!(
            global_context.symbol_trees.len(),
            2,
            "SymbolTrees must be built for ALL files in file_set"
        );
    }

    /// Regression test: ModuleIndex must include ALL files from file_set.
    #[test]
    fn test_module_index_includes_all_files() {
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("CommonModules/МодульА/Ext/Module.bsl", "Процедура МетодА() Экспорт КонецПроцедуры"),
            ("CommonModules/МодульБ/Ext/Module.bsl", "Процедура МетодБ() Экспорт КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        // Only analyze first module
        let files_to_analyze = vec![all_file_ids[0]];
        let global_context =
            orchestrator.initialize_global_context(&files_to_analyze, file_set).unwrap();

        // ModuleIndex must know about BOTH modules for cross-module resolution
        assert_eq!(
            global_context.module_index.common_module_count(),
            2,
            "ModuleIndex must include all CommonModules from file_set, not just analyzed files"
        );
    }
}
