//! Dependency resolver for recursive SymbolTree loading.
//!
//! This module provides functions for resolving cross-module dependencies during
//! parallel file processing. When a worker needs a SymbolTree from another module
//! (e.g., for diagnostics that reference external symbols), it uses this resolver
//! to either:
//! - Return already published SymbolTree (fast path)
//! - Claim and process the target file (recursive processing)
//! - Wait for another worker to finish processing (synchronization)
//!
//! ## Deadlock Prevention
//!
//! The key to avoiding deadlocks in cyclic dependencies (A ↔ B) is the **early
//! SymbolTree publish pattern**:
//!
//! 1. Phase 1 (SymbolTree) has NO external dependencies - only parses the file itself
//! 2. SymbolTree is published BEFORE Phase 2 (diagnostics)
//! 3. Phase 2 may recursively request other SymbolTrees, but they're already published
//!
//! This breaks the cycle: A can safely request B's SymbolTree even if B is requesting
//! A's SymbolTree, because both are published before diagnostics phase begins.

use std::sync::Arc;

use hir_def::{ItemTree, ModuleId, SymbolTree};
use syntax::Parse;
use vfs::FileId;

use crate::provider::AnalysisProvider;

use super::shared_state::{ClaimResult, ParsedFile, ProcessError, SharedState};

/// Get or process a SymbolTree for the target file.
///
/// This is the main entry point for resolving cross-module dependencies.
/// It handles three scenarios:
///
/// 1. **Already ready**: SymbolTree is published → return immediately (fast path)
/// 2. **Not claimed**: File not being processed → claim it and run Phase 1
/// 3. **Claimed by other**: Another worker is processing → wait for SymbolTree
///
/// ## Deadlock Safety
///
/// This function is deadlock-free even with cyclic dependencies because:
/// - We only process Phase 1 (SymbolTree building) recursively
/// - Phase 1 has no external dependencies
/// - SymbolTree is published atomically before any waits occur
///
/// ## Example
///
/// ```ignore
/// // In diagnostics collection for file A:
/// let module_b = module_index.lookup_name("CommonModuleB");
/// let symbol_tree_b = DependencyResolver::get_or_process_symbol_tree(
///     module_b.file_id,
///     provider,
///     shared_state,
/// );
/// // Now we can resolve symbols from module B
/// ```
pub fn get_or_process_symbol_tree(
    target_file: FileId,
    provider: &dyn AnalysisProvider,
    shared_state: &SharedState,
) -> Result<Arc<SymbolTree>, ProcessError> {
    let _span = tracing::debug_span!("get_or_process_symbol_tree", ?target_file).entered();

    // Fast path: already published?
    if shared_state.is_symbol_tree_ready(target_file) {
        tracing::trace!(target_file = ?target_file, "SymbolTree already ready (fast path)");
        return shared_state.get_symbol_tree(target_file).ok_or_else(|| {
            ProcessError::DependencyFailed(
                target_file,
                Arc::from("SymbolTree marked ready but not found"),
            )
        });
    }

    // Try to claim the file
    match shared_state.try_claim(target_file) {
        ClaimResult::ByUs => {
            tracing::debug!(target_file = ?target_file, "Claimed for recursive processing");
            // We claimed it - process Phase 1 only (SymbolTree)
            process_symbol_tree_only(target_file, provider, shared_state)?;
        }
        ClaimResult::ByOther | ClaimResult::NotReady => {
            tracing::debug!(target_file = ?target_file, "Waiting for other worker to finish");
            // Another worker is processing - wait for SymbolTree
            shared_state.wait_for_symbol_tree(target_file)?;
        }
        ClaimResult::AlreadyDone => {
            tracing::trace!(target_file = ?target_file, "Race condition: already done");
            // Race condition - another thread just finished
        }
    }

    // At this point, SymbolTree must be ready
    shared_state.get_symbol_tree(target_file).ok_or_else(|| {
        ProcessError::DependencyFailed(
            target_file,
            Arc::from("SymbolTree should be ready but not found"),
        )
    })
}

/// Process only Phase 1 (SymbolTree building) for a file.
///
/// This is a helper function called by `get_or_process_symbol_tree` when
/// it needs to recursively process a dependency.
///
/// ## Phases
///
/// Only Phase 1 is performed:
/// - Read file text
/// - Parse to AST
/// - Lower to ItemTree
/// - Build SymbolTree from ItemTree
/// - Publish to SharedState
///
/// Phase 2 (diagnostics) and Phase 3 (cleanup) are NOT performed here.
/// The file remains claimed for the original worker to complete later.
///
/// ## Pre-conditions
///
/// - File must be claimed via `SharedState::try_claim()`
///
/// ## Post-conditions
///
/// - SymbolTree published to SharedState
/// - File status is `FileStatus::SymbolTreeReady`
fn process_symbol_tree_only(
    file_id: FileId,
    provider: &dyn AnalysisProvider,
    shared_state: &SharedState,
) -> Result<(), ProcessError> {
    let _span = tracing::debug_span!("process_symbol_tree_only", ?file_id).entered();

    // Read file text ONCE
    let text: Arc<str> = Arc::from(provider.file_text(file_id));

    if text.is_empty() {
        tracing::warn!(file_id = ?file_id, "Empty file in recursive processing");
    }

    // Parse to AST ONCE
    let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text));
    if parse.has_errors() {
        let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
        tracing::warn!(file_id = ?file_id, errors = ?errors, "Parse errors in recursive processing");
        // Continue - we can still build ItemTree from partial AST
    }

    // Lower to ItemTree (signatures only, no dependencies) ONCE
    let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
    tracing::trace!(
        file_id = ?file_id,
        num_procedures = item_tree.procedures().count(),
        num_functions = item_tree.functions().count(),
        "ItemTree built in recursive processing"
    );

    // Build ModuleId and SymbolTree
    let module_id = ModuleId::new(file_id);
    let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));

    // Cache ParsedFile for Phase 2 (with module_id for lazy HIR/CFG)
    let parsed_file = Arc::new(ParsedFile::new(text, parse, Arc::clone(&item_tree), module_id));
    shared_state.cache_parsed_file(file_id, parsed_file);

    // Publish to SharedState (this atomically makes it visible to other threads)
    // Status stays SymbolTreeReady (not Completed) - Phase 2 still needed
    shared_state.publish_symbol_tree(file_id, symbol_tree);

    tracing::debug!(file_id = ?file_id, "SymbolTree published in recursive processing (Phase 2 pending)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{FileReader, GlobalContext, StreamingProvider};
    use rustc_hash::FxHashMap;
    use vfs::{file_set::FileSet, VfsPath};

    fn create_test_provider_with_files(
        files: FxHashMap<FileId, String>,
    ) -> (Arc<StreamingProvider>, Arc<SharedState>, Vec<FileId>) {
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

        (provider, shared_state, sorted_files)
    }

    #[test]
    fn test_fast_path_already_ready() {
        // Setup: file with SymbolTree already published
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        // Claim and publish SymbolTree
        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        let module_id = ModuleId::new(file_id);
        let item_tree = provider.item_tree(file_id);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));
        shared_state.publish_symbol_tree(file_id, symbol_tree.clone());

        // Test: get_or_process should return immediately (fast path)
        let result = get_or_process_symbol_tree(file_id, &*provider, &shared_state);
        assert!(result.is_ok());
        let returned_tree = result.unwrap();

        // Verify it's the same SymbolTree
        assert_eq!(returned_tree.methods().count(), symbol_tree.methods().count());
    }

    #[test]
    fn test_recursive_processing_not_claimed() {
        // Setup: file not yet claimed
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Функция ТестФункция() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        // Test: get_or_process should claim and process Phase 1
        let result = get_or_process_symbol_tree(file_id, &*provider, &shared_state);
        assert!(result.is_ok());
        let symbol_tree = result.unwrap();

        // Verify SymbolTree was built
        assert!(symbol_tree.find_method(&hir_def::Name::new("ТестФункция")).is_some());

        // Verify file status is SymbolTreeReady (not Completed)
        assert!(shared_state.is_symbol_tree_ready(file_id));
        assert_ne!(
            shared_state.file_status(file_id),
            super::super::shared_state::FileStatus::Completed
        );
    }

    #[test]
    fn test_process_symbol_tree_only() {
        // Setup: claim file first
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура А() КонецПроцедуры\nФункция Б() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        // Claim file
        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        // Test: process_symbol_tree_only
        let result = process_symbol_tree_only(file_id, &*provider, &shared_state);
        assert!(result.is_ok());

        // Verify SymbolTree published
        assert!(shared_state.is_symbol_tree_ready(file_id));
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();

        // Verify both methods present
        assert!(symbol_tree.find_method(&hir_def::Name::new("А")).is_some());
        assert!(symbol_tree.find_method(&hir_def::Name::new("Б")).is_some());
    }

    #[test]
    fn test_cyclic_dependency_simulation() {
        // Setup: Two files that could have cyclic dependency
        // File A references symbols from File B, and vice versa
        let file_a = FileId(0);
        let file_b = FileId(1);

        let mut files = FxHashMap::default();
        files.insert(file_a, "Процедура ИзА() КонецПроцедуры".to_string());
        files.insert(file_b, "Функция ИзБ() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        // Simulate worker processing file A
        assert_eq!(shared_state.try_claim(file_a), super::super::shared_state::ClaimResult::ByUs);

        // Worker A builds its SymbolTree
        process_symbol_tree_only(file_a, &*provider, &shared_state).unwrap();

        // Simulate worker processing file B
        assert_eq!(shared_state.try_claim(file_b), super::super::shared_state::ClaimResult::ByUs);

        // Worker B builds its SymbolTree
        process_symbol_tree_only(file_b, &*provider, &shared_state).unwrap();

        // Now simulate Phase 2: Worker A requests B's SymbolTree
        let result_b = get_or_process_symbol_tree(file_b, &*provider, &shared_state);
        assert!(result_b.is_ok());

        // Simulate Phase 2: Worker B requests A's SymbolTree
        let result_a = get_or_process_symbol_tree(file_a, &*provider, &shared_state);
        assert!(result_a.is_ok());

        // Both should succeed without deadlock
        let tree_a = result_a.unwrap();
        let tree_b = result_b.unwrap();

        assert!(tree_a.find_method(&hir_def::Name::new("ИзА")).is_some());
        assert!(tree_b.find_method(&hir_def::Name::new("ИзБ")).is_some());
    }

    #[test]
    fn test_error_propagation() {
        // Setup: file with invalid BSL code that will have parse errors
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        // Invalid BSL: missing closing parenthesis
        files.insert(file_id, "Процедура ОшибочнаяПроцедура( КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files) = create_test_provider_with_files(files);

        // Claim the file
        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        // Process - should succeed even with parse errors (partial AST)
        let result = process_symbol_tree_only(file_id, &*provider, &shared_state);

        // Should succeed with partial SymbolTree (graceful degradation)
        assert!(result.is_ok());
        assert!(shared_state.is_symbol_tree_ready(file_id));

        // Verify we can get the SymbolTree (may be empty or partial)
        let symbol_tree = shared_state.get_symbol_tree(file_id);
        assert!(symbol_tree.is_some());
    }

    #[test]
    fn test_wait_scenario_with_threads() {
        use std::sync::mpsc;
        use std::sync::Barrier;
        use std::thread;
        use std::time::Duration;

        // Setup: single file, two workers will race to process it
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура МногоРаботы() КонецПроцедуры".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        let provider = Arc::clone(&provider);
        let shared_state = Arc::clone(&shared_state);

        // Barrier to ensure both threads start at same time
        let barrier = Arc::new(Barrier::new(2));

        // Channels for timeout handling
        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let barrier1 = Arc::clone(&barrier);
        let provider1 = Arc::clone(&provider);
        let state1 = Arc::clone(&shared_state);

        let barrier2 = Arc::clone(&barrier);
        let provider2 = Arc::clone(&provider);
        let state2 = Arc::clone(&shared_state);

        // Thread 1: tries to get SymbolTree
        thread::spawn(move || {
            barrier1.wait();
            let result = get_or_process_symbol_tree(file_id, &*provider1, &state1);
            let _ = tx1.send(result);
        });

        // Thread 2: tries to get SymbolTree
        thread::spawn(move || {
            barrier2.wait();
            let result = get_or_process_symbol_tree(file_id, &*provider2, &state2);
            let _ = tx2.send(result);
        });

        // Wait for both threads with 5 second timeout
        let timeout = Duration::from_secs(5);
        let result1 = rx1
            .recv_timeout(timeout)
            .expect("Thread 1 timed out after 5 seconds - possible deadlock!");
        let result2 = rx2
            .recv_timeout(timeout)
            .expect("Thread 2 timed out after 5 seconds - possible deadlock!");

        assert!(result1.is_ok(), "Thread 1 failed: {:?}", result1.err());
        assert!(result2.is_ok(), "Thread 2 failed: {:?}", result2.err());

        // Both should get the same SymbolTree (by content)
        let tree1 = result1.unwrap();
        let tree2 = result2.unwrap();

        assert_eq!(tree1.methods().count(), tree2.methods().count());
    }
}
