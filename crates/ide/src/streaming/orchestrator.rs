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
use tracing::{error, info, warn};
use vfs::{file_set::FileSet, FileId};

use ide_db::hir_def::{ItemTree, ModuleId, SymbolTree, WorkspaceSymbols};

// Import from ide-db (infrastructure layer)
use ide_db::streaming::{FileReader, GlobalContext, SharedState, StreamingProvider};

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
        let sorted_files = self.sort_files_by_priority(files);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files.clone());
        let provider = Arc::new(StreamingProvider::new(global_context));

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state, results_tx)?;

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

        // 1. Load configuration (optional)
        let configuration = if let Some(ref config_path) = self.configuration_path {
            info!(path = ?config_path, "Loading configuration");
            // TODO: Load Configuration from JSON
            // For now, return None
            None
        } else {
            info!("No configuration specified");
            None
        };

        // 2. Create FileReader
        let file_set_arc = Arc::new(file_set.clone());
        let file_reader = FileReader::from_disk(self.workspace_root.clone(), file_set_arc.clone());

        // 3. Build all SymbolTrees (Phase 1 only)
        info!(num_files = files.len(), "Building SymbolTrees");
        let mut symbol_trees = FxHashMap::default();

        for &file_id in files {
            let text = file_reader.read(file_id).unwrap_or_default();

            // Parse
            let parse = parser::parse(&text);
            if parse.has_errors() {
                let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
                warn!(file_id = ?file_id, errors = ?errors, "Parse errors in initialization");
            }

            // Lower to ItemTree
            let item_tree = ItemTree::from_parse(&parse);

            // Build SymbolTree
            let module_id = ModuleId::new(file_id);
            let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));

            symbol_trees.insert(file_id, symbol_tree);
        }

        info!(num_symbol_trees = symbol_trees.len(), "SymbolTrees built successfully");

        // 4. Build WorkspaceSymbols
        // TODO: Implement proper WorkspaceSymbols building from symbol trees
        let workspace_symbols = Arc::new(WorkspaceSymbols::default());
        info!("WorkspaceSymbols built (empty for now)");

        // 5. Build ModuleIndex
        // TODO: Extract module names from file paths and build proper index
        let module_index = Arc::new(ide_db::hir_def::ModuleIndex::new());
        info!("ModuleIndex built (empty for now)");

        Ok(Arc::new(GlobalContext {
            configuration,
            symbol_trees,
            workspace_symbols,
            module_index,
            file_set: file_set_arc,
            file_reader,
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
    fn sort_files_by_priority(&self, files: Vec<FileId>) -> Vec<FileId> {
        // TODO: Implement actual priority sorting based on metadata
        // For now, just return files as-is
        info!(num_files = files.len(), "Sorting files by priority");

        // Placeholder: no sorting yet
        files
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
        results_tx: crossbeam_channel::Sender<FileResult>,
    ) -> Result<Vec<thread::JoinHandle<()>>, OrchestratorError> {
        let _span = tracing::info_span!("spawn_workers").entered();

        info!(num_workers = self.num_workers, "Spawning worker pool");

        let mut handles = Vec::with_capacity(self.num_workers);

        for worker_id in 0..self.num_workers {
            let provider = Arc::clone(&provider);
            let shared_state = Arc::clone(&shared_state);
            let results_tx = results_tx.clone();

            let handle = thread::Builder::new()
                .name(format!("worker-{}", worker_id))
                .spawn(move || {
                    worker_main(worker_id, shared_state, provider, results_tx);
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
}

/// Builder for constructing `AnalysisOrchestrator`.
#[derive(Default)]
pub struct OrchestratorBuilder {
    num_workers: Option<usize>,
    workspace_root: Option<PathBuf>,
    configuration_path: Option<PathBuf>,
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
        assert_eq!(results.total_diagnostics, 0); // Phase 2 not implemented
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
}
