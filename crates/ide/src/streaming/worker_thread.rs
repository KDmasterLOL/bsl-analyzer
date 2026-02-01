//! Worker thread for parallel file processing.
//!
//! This module implements the main worker loop for parallel file processing
//! in batch analysis mode. Each worker:
//!
//! 1. Claims files from SharedState (lock-free work stealing)
//! 2. Processes files using FileProcessor (3-phase processing)
//! 3. Sends results via channel to orchestrator
//! 4. Handles panics gracefully without crashing other workers
//!
//! ## Architecture
//!
//! Workers are spawned by AnalysisOrchestrator and run independently:
//!
//! ```text
//! ┌─────────────┐
//! │ Worker 1    │──┐
//! ├─────────────┤  │
//! │ Worker 2    │──┼──> SharedState (lock-free work queue)
//! ├─────────────┤  │
//! │ Worker N    │──┘
//! └─────────────┘
//!       │
//!       └──> results_tx (crossbeam channel)
//! ```
//!
//! ## Graceful Degradation
//!
//! Workers catch panics and convert them to error results. A panic in one
//! worker does not affect other workers or crash the application.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use crossbeam_channel::Sender;
use tracing::{error, info, warn};

// Import from ide-db (infrastructure layer)
use ide_db::provider::AnalysisProvider;
use ide_db::streaming::{ClaimResult, FileStatus, SharedState};

// Import from ide-diagnostics
use ide_diagnostics::DiagnosticsConfig;

// Import from current crate (ide features layer)
use super::file_processor::{FileProcessor, FileResult};

/// Main worker loop.
///
/// This function runs in a worker thread and processes files until all files
/// are done or the channel is closed.
///
/// ## Parameters
///
/// - `worker_id`: Unique identifier for this worker (for logging/debugging)
/// - `shared_state`: Coordination structure for claiming files
/// - `provider`: AnalysisProvider for accessing file data
/// - `config`: Diagnostics configuration (enabled/disabled rules, parameters)
/// - `results_tx`: Channel for sending results back to orchestrator
///
/// ## Behavior
///
/// 1. Loop: claim next file from SharedState
/// 2. If Some(file_id): process file with FileProcessor (includes diagnostics!)
/// 3. Send result via channel (ignore send errors - orchestrator may have stopped)
/// 4. If None: all files processed, exit gracefully
/// 5. Catch panics and convert to error results
///
/// ## Note
///
/// The dummy RootDatabase is created locally in FileProcessor since it's not
/// thread-safe (contains RefCell) and cannot be passed across threads.
///
/// ## Example
///
/// ```ignore
/// let handle = thread::spawn(move || {
///     worker_main(
///         worker_id,
///         shared_state,
///         provider,
///         config,
///         results_tx,
///     );
/// });
/// ```
pub fn worker_main(
    worker_id: usize,
    shared_state: Arc<SharedState>,
    provider: Arc<dyn AnalysisProvider + Send + Sync>,
    config: Arc<DiagnosticsConfig>,
    results_tx: Sender<FileResult>,
) {
    let _span = tracing::info_span!("worker", worker_id).entered();
    info!("Worker started");

    let mut files_processed = 0;

    // Create FileProcessor for this worker (with diagnostics support!)
    let processor = FileProcessor::new(&*provider, &shared_state, &config);

    loop {
        // Claim next file (lock-free work stealing)
        let file_id = match shared_state.claim_next_file() {
            Some(id) => id,
            None => {
                // All files processed
                info!(files_processed, "Worker finished - no more files");
                break;
            }
        };

        // Process file with panic protection
        let result = catch_unwind(AssertUnwindSafe(|| processor.process_file(file_id)));

        let file_result = match result {
            Ok(file_result) => {
                // Success
                if let Some(ref error) = file_result.error {
                    warn!(
                        file_id = ?file_id,
                        error = %error,
                        "File processed with error"
                    );
                } else {
                    tracing::debug!(
                        file_id = ?file_id,
                        num_diagnostics = file_result.diagnostics.len(),
                        "File processed successfully"
                    );
                }
                file_result
            }
            Err(panic) => {
                // Panic during processing
                let error_msg: Arc<str> = if let Some(s) = panic.downcast_ref::<&str>() {
                    Arc::from(*s)
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    Arc::from(s.as_str())
                } else {
                    Arc::from("Unknown panic")
                };

                error!(
                    file_id = ?file_id,
                    error = %error_msg,
                    "Worker panic caught"
                );

                // Mark file as failed in SharedState
                shared_state.mark_failed(file_id, error_msg.clone());

                FileResult { file_id, diagnostics: vec![], error: Some(error_msg), metrics: None }
            }
        };

        files_processed += 1;

        // Send result (ignore errors - orchestrator may have stopped)
        if results_tx.send(file_result).is_err() {
            warn!("Results channel closed, stopping worker");
            break;
        }
    }

    // === SECOND PASS: Process Phase 2 for recursively-resolved files ===
    // Files processed via dependency_resolver only had Phase 1 done.
    // Now we need to collect their diagnostics (Phase 2).

    let mut phase2_processed = 0;
    let num_files = shared_state.num_files();

    loop {
        // Find next file awaiting Phase 2 (status == SymbolTreeReady)
        let file_to_process = (0..num_files).find_map(|idx| {
            let file_id = shared_state.file_id_at(idx)?;

            // Only process files stuck in SymbolTreeReady (Phase 1 done, Phase 2 pending)
            if shared_state.file_status(file_id) != FileStatus::SymbolTreeReady {
                return None;
            }

            // Try to claim for Phase 2
            match shared_state.try_claim_for_diagnostics(file_id) {
                ClaimResult::ByUs => Some(file_id),
                _ => None,
            }
        });

        let file_id = match file_to_process {
            Some(id) => id,
            None => {
                // No more files awaiting Phase 2
                if phase2_processed > 0 {
                    info!(phase2_processed, "Second pass finished");
                }
                break;
            }
        };

        // Process Phase 2 only (diagnostics) with panic protection
        let result = catch_unwind(AssertUnwindSafe(|| processor.process_diagnostics_only(file_id)));

        let file_result = match result {
            Ok(file_result) => {
                tracing::debug!(
                    file_id = ?file_id,
                    num_diagnostics = file_result.diagnostics.len(),
                    "Phase 2 completed for recursive file"
                );
                file_result
            }
            Err(panic) => {
                let error_msg: Arc<str> = if let Some(s) = panic.downcast_ref::<&str>() {
                    Arc::from(*s)
                } else if let Some(s) = panic.downcast_ref::<String>() {
                    Arc::from(s.as_str())
                } else {
                    Arc::from("Unknown panic in Phase 2")
                };

                error!(
                    file_id = ?file_id,
                    error = %error_msg,
                    "Worker panic caught in Phase 2"
                );

                shared_state.mark_failed(file_id, error_msg.clone());
                FileResult { file_id, diagnostics: vec![], error: Some(error_msg), metrics: None }
            }
        };

        phase2_processed += 1;

        // Send result (ignore errors - orchestrator may have stopped)
        if results_tx.send(file_result).is_err() {
            warn!("Results channel closed during Phase 2, stopping worker");
            break;
        }
    }

    let total_processed = files_processed + phase2_processed;
    info!(files_processed, phase2_processed, total_processed, "Worker exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use ide_db::streaming::{FileReader, GlobalContext, StreamingProvider};
    use rustc_hash::FxHashMap;
    use std::thread;
    use vfs::{file_set::FileSet, FileId, VfsPath};

    type TestSetup = (
        Arc<StreamingProvider>,
        Arc<SharedState>,
        Vec<FileId>,
        Arc<DiagnosticsConfig>,
        Sender<FileResult>,
        crossbeam_channel::Receiver<FileResult>,
    );

    fn create_test_setup(files: FxHashMap<FileId, String>) -> TestSetup {
        let mut file_set = FileSet::default();
        for &file_id in files.keys() {
            file_set.insert(file_id, VfsPath::new(format!("/test{}.bsl", file_id.index())));
        }

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files.clone()),
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files: Vec<FileId> = files.keys().copied().collect();
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files.clone());

        // Create diagnostics config (all disabled for tests)
        let config = Arc::new(DiagnosticsConfig::default());

        let (tx, rx) = unbounded();

        (provider, shared_state, sorted_files, config, tx, rx)
    }

    #[test]
    fn test_worker_single_file() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, config, tx, rx) = create_test_setup(files);

        // Run worker in thread
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        // Collect results
        let results: Vec<FileResult> = rx.iter().collect();

        handle.join().expect("Worker thread panicked");

        // Verify
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, file_id);
        assert!(results[0].error.is_none());
        // Diagnostics collection is now implemented in Phase 2
    }

    #[test]
    fn test_worker_multiple_files() {
        let mut files = FxHashMap::default();
        for i in 0..5 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Процедура Тест{}() КонецПроцедуры", i));
        }

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_test_setup(files.clone());

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        // Collect results
        let results: Vec<FileResult> = rx.iter().collect();

        handle.join().expect("Worker thread panicked");

        // Verify all files processed
        assert_eq!(results.len(), 5);
        for result in &results {
            assert!(result.error.is_none());
        }
    }

    #[test]
    fn test_worker_with_multiple_workers() {
        // Create 10 files
        let mut files = FxHashMap::default();
        for i in 0..10 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Функция Функция{}() КонецФункции", i));
        }

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_test_setup(files.clone());

        // Spawn 3 workers
        let mut handles = vec![];
        for worker_id in 0..3 {
            let provider = Arc::clone(&provider);
            let shared_state = Arc::clone(&shared_state);
            let config = Arc::clone(&config);
            let tx = tx.clone();

            let handle = thread::spawn(move || {
                worker_main(worker_id, shared_state, provider, config, tx);
            });

            handles.push(handle);
        }

        // Drop original sender so rx.iter() finishes
        drop(tx);

        // Collect all results
        let results: Vec<FileResult> = rx.iter().collect();

        // Wait for all workers
        for handle in handles {
            handle.join().expect("Worker thread panicked");
        }

        // Verify all 10 files processed exactly once
        assert_eq!(results.len(), 10);

        let mut processed_files: Vec<u32> = results.iter().map(|r| r.file_id.index()).collect();
        processed_files.sort_unstable();

        let expected: Vec<u32> = (0..10).collect();
        assert_eq!(processed_files, expected);
    }

    #[test]
    fn test_worker_with_parse_errors() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        // Invalid BSL code
        files.insert(file_id, "Процедура Ошибка( КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, config, tx, rx) = create_test_setup(files);

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        // Collect results
        let results: Vec<FileResult> = rx.iter().collect();

        handle.join().expect("Worker thread panicked");

        // Should complete even with parse errors (graceful degradation)
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, file_id);
        // Parser handles errors gracefully, so no error should be set
    }

    #[test]
    fn test_worker_empty_queue() {
        let files = FxHashMap::default(); // Empty

        let (provider, shared_state, _sorted_files, config, tx, rx) = create_test_setup(files);

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        // Collect results
        let results: Vec<FileResult> = rx.iter().collect();

        handle.join().expect("Worker thread panicked");

        // Should exit immediately with no results
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_worker_channel_closed() {
        let mut files = FxHashMap::default();
        for i in 0..100 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Процедура Тест{}() КонецПроцедуры", i));
        }

        let (provider, shared_state, _sorted_files, config, tx, rx) = create_test_setup(files);

        // Start worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        // Close receiver immediately
        drop(rx);

        // Worker should detect closed channel and exit gracefully
        handle.join().expect("Worker thread panicked");

        // Test passes if worker exits without panic
    }

    // ========================================================================
    // Caching Integration Tests
    // ========================================================================

    type TrackingTestSetup = (
        Arc<StreamingProvider>,
        Arc<SharedState>,
        Vec<FileId>,
        Arc<DiagnosticsConfig>,
        Sender<FileResult>,
        crossbeam_channel::Receiver<FileResult>,
    );

    /// Create test setup with file read tracking.
    ///
    /// Uses SharedState with FileReader::in_memory_with_tracking()
    /// and StreamingProvider::with_shared_state() for proper cache integration.
    fn create_tracking_test_setup(files: FxHashMap<FileId, String>) -> TrackingTestSetup {
        let mut file_set = FileSet::default();
        for &file_id in files.keys() {
            file_set.insert(file_id, VfsPath::new(format!("/test{}.bsl", file_id.index())));
        }

        // Use tracking FileReader
        let file_reader = FileReader::in_memory_with_tracking(files.clone());

        let sorted_files: Vec<FileId> = {
            let mut v: Vec<_> = files.keys().copied().collect();
            v.sort_by_key(|f| f.index());
            v
        };

        // Create GlobalContext with tracking file reader
        let global_context = GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader,
        };

        // Create SharedState with the GlobalContext (contains file_reader for tracking)
        let shared_state = SharedState::new(global_context, sorted_files.clone());

        // Create provider WITH shared_state reference for cache access
        let provider_global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(ide_db::hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(ide_db::hir_def::ModuleIndex::new()),
            file_set: shared_state.file_set().clone(),
            file_reader: FileReader::empty(), // Provider uses SharedState's reader via cache
        });
        let provider = Arc::new(StreamingProvider::with_shared_state(
            provider_global,
            Arc::clone(&shared_state),
        ));

        let config = Arc::new(DiagnosticsConfig::default());
        let (tx, rx) = unbounded();

        (provider, shared_state, sorted_files, config, tx, rx)
    }

    #[test]
    fn test_cache_cleared_after_phase2() {
        // Setup: single file
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_tracking_test_setup(files);

        // Capture shared_state for verification
        let shared_state_check = Arc::clone(&shared_state);

        // Process file
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        let results: Vec<_> = rx.iter().collect();
        handle.join().expect("Worker panicked");

        // Verify results
        assert_eq!(results.len(), 1);
        assert!(results[0].error.is_none());

        // CRITICAL: Cache must be empty after processing
        assert_eq!(
            shared_state_check.parsed_cache_len(),
            0,
            "ParsedFile cache should be cleared after Phase 2"
        );
        assert!(
            !shared_state_check.has_parsed_file(file_id),
            "File should not be in cache after completion"
        );

        // SymbolTree should still be available (it's kept)
        assert!(
            shared_state_check.get_symbol_tree(file_id).is_some(),
            "SymbolTree should persist after processing"
        );
    }

    #[test]
    fn test_multiple_files_cache_cleared() {
        // Setup: 5 files
        let mut files = FxHashMap::default();
        for i in 0..5 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Процедура Тест{}() КонецПроцедуры", i));
        }

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_tracking_test_setup(files);

        let shared_state_check = Arc::clone(&shared_state);

        // Process files
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        let results: Vec<_> = rx.iter().collect();
        handle.join().expect("Worker panicked");

        // Verify all files processed
        assert_eq!(results.len(), 5);

        // CRITICAL: Cache must be empty after all files processed
        assert_eq!(
            shared_state_check.parsed_cache_len(),
            0,
            "ParsedFile cache should be empty after all files completed"
        );

        // All SymbolTrees should be available
        for i in 0..5 {
            assert!(
                shared_state_check.get_symbol_tree(FileId(i)).is_some(),
                "SymbolTree for file {} should persist",
                i
            );
        }
    }

    #[test]
    fn test_file_read_count_single_file() {
        // Setup: single file
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_tracking_test_setup(files);

        let shared_state_check = Arc::clone(&shared_state);

        // Process file
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        let results: Vec<_> = rx.iter().collect();
        handle.join().expect("Worker panicked");

        assert_eq!(results.len(), 1);

        // File should be read exactly once:
        // - Phase 1: read once for parsing
        // - Phase 2: uses cached ParsedFile (no read)
        let read_count = shared_state_check.file_reader().read_count_for(file_id);
        assert_eq!(read_count, 1, "File should be read exactly once (cached for Phase 2)");
    }

    #[test]
    fn test_multiple_files_each_read_once() {
        // Setup: 3 files
        let mut files = FxHashMap::default();
        for i in 0..3 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Процедура Тест{}() КонецПроцедуры", i));
        }

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_tracking_test_setup(files);

        let shared_state_check = Arc::clone(&shared_state);

        // Process all files
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        let results: Vec<_> = rx.iter().collect();
        handle.join().expect("Worker panicked");

        assert_eq!(results.len(), 3);

        // Each file should be read exactly once
        for i in 0..3 {
            let file_id = FileId(i);
            let read_count = shared_state_check.file_reader().read_count_for(file_id);
            assert_eq!(read_count, 1, "File {} should be read exactly once, got {}", i, read_count);
        }

        // Total reads should be exactly 3
        assert_eq!(
            shared_state_check.file_reader().total_read_count(),
            3,
            "Total file reads should be exactly 3"
        );

        // All SymbolTrees should be accessible (not re-built)
        for i in 0..3 {
            let file_id = FileId(i);
            assert!(
                shared_state_check.get_symbol_tree(file_id).is_some(),
                "SymbolTree for file {} should be available",
                i
            );
        }
    }

    #[test]
    fn test_symbol_tree_reused_across_files() {
        // Setup: 2 files, test that SymbolTree from file 0 is reusable
        // when accessed during processing of file 1
        let file_0 = FileId(0);
        let file_1 = FileId(1);
        let mut files = FxHashMap::default();
        files.insert(file_0, "Процедура Метод0() Экспорт КонецПроцедуры".to_string());
        files.insert(file_1, "Процедура Метод1() КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, config, tx, rx) =
            create_tracking_test_setup(files);

        let shared_state_check = Arc::clone(&shared_state);
        let provider_check = Arc::clone(&provider);

        // Process all files
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, config, tx);
        });

        let results: Vec<_> = rx.iter().collect();
        handle.join().expect("Worker panicked");

        assert_eq!(results.len(), 2);

        // After processing, SymbolTrees are in SharedState
        let tree_0 = shared_state_check.get_symbol_tree(file_0).unwrap();
        let tree_1 = shared_state_check.get_symbol_tree(file_1).unwrap();

        // Verify SymbolTrees have the methods
        assert!(
            tree_0.find_method(&ide_db::hir_def::Name::new("Метод0")).is_some(),
            "SymbolTree 0 should have Метод0"
        );
        assert!(
            tree_1.find_method(&ide_db::hir_def::Name::new("Метод1")).is_some(),
            "SymbolTree 1 should have Метод1"
        );

        // Provider should return the same SymbolTree instances (from SharedState)
        let module_0 = ide_db::hir_def::ModuleId::new(file_0);
        let provider_tree_0 = provider_check.symbol_tree(module_0);
        assert!(
            Arc::ptr_eq(&tree_0, &provider_tree_0),
            "Provider should return SharedState's SymbolTree (same Arc)"
        );

        // Files should each be read exactly once
        assert_eq!(shared_state_check.file_reader().read_count_for(file_0), 1);
        assert_eq!(shared_state_check.file_reader().read_count_for(file_1), 1);
    }
}
