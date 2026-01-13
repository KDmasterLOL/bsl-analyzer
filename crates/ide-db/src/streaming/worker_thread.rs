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

use crate::provider::AnalysisProvider;

use super::file_processor::{FileProcessor, FileResult};
use super::shared_state::SharedState;

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
/// - `results_tx`: Channel for sending results back to orchestrator
///
/// ## Behavior
///
/// 1. Loop: claim next file from SharedState
/// 2. If Some(file_id): process file with FileProcessor
/// 3. Send result via channel (ignore send errors - orchestrator may have stopped)
/// 4. If None: all files processed, exit gracefully
/// 5. Catch panics and convert to error results
///
/// ## Example
///
/// ```ignore
/// let handle = thread::spawn(move || {
///     worker_main(
///         worker_id,
///         shared_state,
///         provider,
///         results_tx,
///     );
/// });
/// ```
pub fn worker_main(
    worker_id: usize,
    shared_state: Arc<SharedState>,
    provider: Arc<dyn AnalysisProvider + Send + Sync>,
    results_tx: Sender<FileResult>,
) {
    let _span = tracing::info_span!("worker", worker_id).entered();
    info!("Worker started");

    let mut files_processed = 0;

    // Create FileProcessor for this worker
    let processor = FileProcessor::new(&*provider, &shared_state);

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

                FileResult { file_id, diagnostics: vec![], error: Some(error_msg) }
            }
        };

        files_processed += 1;

        // Send result (ignore errors - orchestrator may have stopped)
        if results_tx.send(file_result).is_err() {
            warn!("Results channel closed, stopping worker");
            break;
        }
    }

    info!(files_processed, "Worker exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{FileReader, GlobalContext, StreamingProvider};
    use crossbeam_channel::unbounded;
    use rustc_hash::FxHashMap;
    use std::thread;
    use vfs::{file_set::FileSet, FileId, VfsPath};

    type TestSetup = (
        Arc<StreamingProvider>,
        Arc<SharedState>,
        Vec<FileId>,
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
            workspace_symbols: Arc::new(hir_def::WorkspaceSymbols::default()),
            module_index: Arc::new(hir_def::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files.clone()),
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files: Vec<FileId> = files.keys().copied().collect();
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files.clone());

        let (tx, rx) = unbounded();

        (provider, shared_state, sorted_files, tx, rx)
    }

    #[test]
    fn test_worker_single_file() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files);

        // Run worker in thread
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, tx);
        });

        // Collect results
        let results: Vec<FileResult> = rx.iter().collect();

        handle.join().expect("Worker thread panicked");

        // Verify
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_id, file_id);
        assert!(results[0].error.is_none());
        assert_eq!(results[0].diagnostics.len(), 0); // Phase 2 not implemented yet
    }

    #[test]
    fn test_worker_multiple_files() {
        let mut files = FxHashMap::default();
        for i in 0..5 {
            let file_id = FileId(i);
            files.insert(file_id, format!("Процедура Тест{}() КонецПроцедуры", i));
        }

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files.clone());

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, tx);
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

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files.clone());

        // Spawn 3 workers
        let mut handles = vec![];
        for worker_id in 0..3 {
            let provider = Arc::clone(&provider);
            let shared_state = Arc::clone(&shared_state);
            let tx = tx.clone();

            let handle = thread::spawn(move || {
                worker_main(worker_id, shared_state, provider, tx);
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

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files);

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, tx);
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

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files);

        // Run worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, tx);
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

        let (provider, shared_state, _sorted_files, tx, rx) = create_test_setup(files);

        // Start worker
        let handle = thread::spawn(move || {
            worker_main(0, shared_state, provider, tx);
        });

        // Close receiver immediately
        drop(rx);

        // Worker should detect closed channel and exit gracefully
        handle.join().expect("Worker thread panicked");

        // Test passes if worker exits without panic
    }
}
