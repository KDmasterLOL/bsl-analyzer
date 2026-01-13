//! Shared state for worker pool coordination.
//!
//! This module implements lock-free file claiming and synchronization
//! for parallel file processing with minimal blocking.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use crossbeam_utils::CachePadded;
use dashmap::DashMap;
use hir_def::{ModuleIndex, SymbolTree, WorkspaceSymbols};
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxBuildHasher;
use vfs::{file_set::FileSet, FileId};

use super::{FileReader, GlobalContext};

/// File processing status - single source of truth.
///
/// Transitions are monotonic: NotStarted → Parsing → SymbolTreeReady → Completed
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FileStatus {
    /// No worker has started processing this file.
    NotStarted = 0,

    /// Worker is parsing and building ItemTree/SymbolTree.
    /// SymbolTree NOT yet available.
    Parsing = 1,

    /// SymbolTree has been published to shared cache.
    /// Worker is now computing diagnostics.
    SymbolTreeReady = 2,

    /// File processing completely finished.
    /// All resources released.
    Completed = 3,
}

impl FileStatus {
    /// Convert from u8 (used with atomics).
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(FileStatus::NotStarted),
            1 => Some(FileStatus::Parsing),
            2 => Some(FileStatus::SymbolTreeReady),
            3 => Some(FileStatus::Completed),
            _ => None,
        }
    }
}

/// Result of attempting to claim a file for processing.
#[derive(Debug, PartialEq, Eq)]
pub enum ClaimResult {
    /// This worker successfully claimed the file.
    ByUs,

    /// Another worker is processing this file.
    ByOther,

    /// File already completed (race condition).
    AlreadyDone,
}

/// Error during file processing.
#[derive(Debug, Clone)]
pub enum ProcessError {
    /// Failed to parse file.
    ParseError(FileId, Arc<str>),

    /// Failed to build HIR.
    LoweringError(FileId, Arc<str>),

    /// Dependency failed to process.
    DependencyFailed(FileId, Arc<str>),

    /// Worker panic (caught).
    WorkerPanic(FileId, Arc<str>),

    /// I/O error reading file.
    IoError(FileId, Arc<str>),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::ParseError(file_id, msg) => {
                write!(f, "Parse error in {:?}: {}", file_id, msg)
            }
            ProcessError::LoweringError(file_id, msg) => {
                write!(f, "Lowering error in {:?}: {}", file_id, msg)
            }
            ProcessError::DependencyFailed(file_id, msg) => {
                write!(f, "Dependency {:?} failed: {}", file_id, msg)
            }
            ProcessError::WorkerPanic(file_id, msg) => {
                write!(f, "Worker panic in {:?}: {}", file_id, msg)
            }
            ProcessError::IoError(file_id, msg) => {
                write!(f, "I/O error in {:?}: {}", file_id, msg)
            }
        }
    }
}

impl std::error::Error for ProcessError {}

/// Shared state for worker pool coordination.
///
/// Design principles:
/// - Lock-free fast path for file claiming
/// - Minimal false sharing via cache line padding
/// - Early SymbolTree publish to avoid deadlocks
/// - Per-file condvars for precise wakeups
pub struct SharedState {
    // === FILE STATUS TRACKING (lock-free) ===
    /// Per-file status array (1 byte per file).
    /// Memory: ~25 KB for 25K files.
    /// Access pattern: frequent reads, occasional writes.
    /// Ordering: SeqCst for status transitions (ensures visibility across threads).
    file_statuses: Box<[AtomicU8]>,

    // === PUBLISHED SYMBOL TREES (concurrent hashmap) ===
    /// Published SymbolTrees indexed by FileId.
    /// Memory: ~292 MB for 25K files (ERP project).
    /// Concurrency: DashMap provides lock-free reads, striped locks for writes.
    /// Lifetime: Populated during Phase 1, kept until analysis completes.
    symbol_trees: Arc<DashMap<FileId, Arc<SymbolTree>, FxBuildHasher>>,

    // === WORK QUEUE (lock-free work stealing) ===
    /// Pre-sorted list of files to process.
    /// Order: CommonModules first (Server→CallServer→ClientServer→Client),
    ///        then ManagerModules, then ObjectModules, then FormModules.
    /// Rationale: Minimize recursive calls and waits.
    sorted_files: Arc<Vec<FileId>>,

    /// Next file index to claim (lock-free work stealing).
    /// Workers atomically increment this to get their next file.
    /// Padded to prevent false sharing with other hot atomics.
    next_file_idx: CachePadded<AtomicUsize>,

    // === SYNCHRONIZATION PRIMITIVES ===
    /// Per-file condvars for waiting on SymbolTree readiness.
    /// Memory: ~400 KB for 25K files (16 bytes per condvar).
    /// Strategy: One condvar per file for precise wakeups.
    condvars: Box<[Condvar]>,

    /// Per-file mutexes paired with condvars.
    /// Memory: ~200 KB for 25K files (8 bytes per mutex on x64).
    /// Note: parking_lot::Mutex is more compact than std::sync::Mutex.
    mutexes: Box<[Mutex<()>]>,

    // === GLOBAL CONTEXT (read-only after init) ===
    /// Configuration metadata (~31 MB for ERP).
    configuration: Option<Arc<bsl_metadata::Configuration>>,

    /// Module name → FileId index (~5 MB).
    module_index: Arc<ModuleIndex>,

    /// Workspace symbols index (~5 MB).
    workspace_symbols: Arc<WorkspaceSymbols>,

    /// FileSet for path resolution (~2 MB).
    file_set: Arc<FileSet>,

    /// File content reader (disk or in-memory).
    file_reader: FileReader,

    // === ERROR TRACKING ===
    /// Files that encountered errors during processing.
    /// Separate tracking to avoid blocking on failed files.
    /// Uses DashMap for concurrent access.
    failed_files: Arc<DashMap<FileId, Arc<str>, FxBuildHasher>>,
}

impl SharedState {
    /// Create a new SharedState from GlobalContext and sorted files.
    pub fn new(global: GlobalContext, sorted_files: Vec<FileId>) -> Arc<Self> {
        let num_files = sorted_files.len();

        Arc::new(Self {
            file_statuses: (0..num_files)
                .map(|_| AtomicU8::new(FileStatus::NotStarted as u8))
                .collect::<Vec<_>>()
                .into_boxed_slice(),

            symbol_trees: Arc::new(DashMap::with_hasher_and_shard_amount(
                FxBuildHasher,
                16, // Shard count for concurrent access
            )),

            sorted_files: Arc::new(sorted_files),
            next_file_idx: CachePadded::new(AtomicUsize::new(0)),

            condvars: (0..num_files).map(|_| Condvar::new()).collect::<Vec<_>>().into_boxed_slice(),

            mutexes: (0..num_files).map(|_| Mutex::new(())).collect::<Vec<_>>().into_boxed_slice(),

            configuration: global.configuration,
            module_index: global.module_index,
            workspace_symbols: global.workspace_symbols,
            file_set: global.file_set,
            file_reader: global.file_reader,

            failed_files: Arc::new(DashMap::with_hasher(FxBuildHasher)),
        })
    }

    // ========================================================================
    // Lock-Free File Claiming
    // ========================================================================

    /// Try to atomically claim a file for processing.
    ///
    /// Lock-free operation using compare-and-swap.
    ///
    /// Pre-conditions:
    /// - file_id must be valid (< sorted_files.len())
    ///
    /// Post-conditions on ClaimResult::ByUs:
    /// - file_statuses[idx] == FileStatus::Parsing
    /// - This worker has exclusive right to process file
    ///
    /// Memory ordering: SeqCst
    /// - Success: NotStarted → Parsing transition visible to all threads
    /// - Failure: Current status loaded with sequential consistency
    pub fn try_claim(&self, file_id: FileId) -> ClaimResult {
        let idx = file_id.index() as usize;

        // Atomic CAS: NotStarted → Parsing
        match self.file_statuses[idx].compare_exchange(
            FileStatus::NotStarted as u8,
            FileStatus::Parsing as u8,
            Ordering::SeqCst, // Success: establish happens-before with status reads
            Ordering::SeqCst, // Failure: ensure we see the actual current status
        ) {
            Ok(_) => ClaimResult::ByUs,
            Err(current) => {
                if current >= FileStatus::Completed as u8 {
                    ClaimResult::AlreadyDone
                } else {
                    ClaimResult::ByOther
                }
            }
        }
    }

    /// Atomically get next file to process (work stealing).
    ///
    /// Lock-free operation.
    /// Returns None when all files are claimed.
    pub fn claim_next_file(&self) -> Option<FileId> {
        loop {
            let idx = self.next_file_idx.fetch_add(1, Ordering::Relaxed);

            if idx >= self.sorted_files.len() {
                return None; // All files distributed
            }

            let file_id = self.sorted_files[idx];

            match self.try_claim(file_id) {
                ClaimResult::ByUs => return Some(file_id),
                ClaimResult::ByOther | ClaimResult::AlreadyDone => {
                    // Another worker got it first, try next file
                    continue;
                }
            }
        }
    }

    // ========================================================================
    // SymbolTree Publishing
    // ========================================================================

    /// Publish SymbolTree after Phase 1 parsing.
    ///
    /// Pre-conditions:
    /// - file_statuses[idx] == FileStatus::Parsing
    /// - SymbolTree built from ItemTree
    ///
    /// Post-conditions:
    /// - symbol_trees contains Arc<SymbolTree> for this file
    /// - file_statuses[idx] == FileStatus::SymbolTreeReady
    /// - All waiting threads are notified
    ///
    /// Memory ordering: SeqCst for status, Release semantics from DashMap insert
    pub fn publish_symbol_tree(&self, file_id: FileId, tree: Arc<SymbolTree>) {
        let idx = file_id.index() as usize;

        // Insert into concurrent hashmap (internally synchronized)
        self.symbol_trees.insert(file_id, tree);

        // Mark as ready (Parsing → SymbolTreeReady)
        // SeqCst ensures insert happens-before status change
        self.file_statuses[idx].store(FileStatus::SymbolTreeReady as u8, Ordering::SeqCst);

        // Wake all threads waiting for this SymbolTree
        self.condvars[idx].notify_all();
    }

    /// Get published SymbolTree if available.
    pub fn get_symbol_tree(&self, file_id: FileId) -> Option<Arc<SymbolTree>> {
        self.symbol_trees.get(&file_id).map(|r| r.clone())
    }

    /// Check if SymbolTree is ready.
    ///
    /// Fast lock-free check.
    #[inline]
    pub fn is_symbol_tree_ready(&self, file_id: FileId) -> bool {
        let idx = file_id.index() as usize;
        self.file_statuses[idx].load(Ordering::SeqCst) >= FileStatus::SymbolTreeReady as u8
    }

    // ========================================================================
    // Dependency Resolution with Wait
    // ========================================================================

    /// Wait for SymbolTree to become ready.
    ///
    /// Used when another worker is processing the file.
    /// Blocks current thread until SymbolTree published.
    ///
    /// Memory ordering: Condvar provides Acquire semantics.
    pub fn wait_for_symbol_tree(&self, file_id: FileId) -> Result<(), ProcessError> {
        let idx = file_id.index() as usize;

        // Double-checked locking pattern
        if self.is_symbol_tree_ready(file_id) {
            return Ok(()); // Ready between call and lock
        }

        let mut guard = self.mutexes[idx].lock();

        // parking_lot Condvar: no spurious wakeups
        while !self.is_symbol_tree_ready(file_id) {
            // Check for failure
            if let Some(error) = self.failed_files.get(&file_id) {
                return Err(ProcessError::DependencyFailed(file_id, error.clone()));
            }

            self.condvars[idx].wait(&mut guard);
        }

        Ok(())
    }

    // ========================================================================
    // File Completion and Error Tracking
    // ========================================================================

    /// Mark file processing as completed.
    ///
    /// Pre-conditions:
    /// - file_statuses[idx] == FileStatus::SymbolTreeReady
    /// - All diagnostics computed
    ///
    /// Post-conditions:
    /// - file_statuses[idx] == FileStatus::Completed
    pub fn mark_completed(&self, file_id: FileId) {
        let idx = file_id.index() as usize;

        // SymbolTreeReady → Completed
        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);

        // Note: No notify needed - no one waits for Completed
    }

    /// Mark file as failed during processing.
    ///
    /// Allows waiting workers to detect failure and propagate error.
    pub fn mark_failed(&self, file_id: FileId, error: Arc<str>) {
        let idx = file_id.index() as usize;

        // Record error
        self.failed_files.insert(file_id, error);

        // Move to Completed to unblock waiters
        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);

        // Wake waiting threads so they can detect failure
        self.condvars[idx].notify_all();
    }

    // ========================================================================
    // Global Context Access
    // ========================================================================

    /// Get configuration metadata.
    pub fn configuration(&self) -> Option<&Arc<bsl_metadata::Configuration>> {
        self.configuration.as_ref()
    }

    /// Get module index.
    pub fn module_index(&self) -> &Arc<ModuleIndex> {
        &self.module_index
    }

    /// Get workspace symbols.
    pub fn workspace_symbols(&self) -> &Arc<WorkspaceSymbols> {
        &self.workspace_symbols
    }

    /// Get file set.
    pub fn file_set(&self) -> &Arc<FileSet> {
        &self.file_set
    }

    /// Get file reader.
    pub fn file_reader(&self) -> &FileReader {
        &self.file_reader
    }

    /// Get file status.
    pub fn file_status(&self, file_id: FileId) -> FileStatus {
        let idx = file_id.index() as usize;
        let status = self.file_statuses[idx].load(Ordering::SeqCst);
        FileStatus::from_u8(status).unwrap_or(FileStatus::NotStarted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn create_test_state(num_files: usize) -> Arc<SharedState> {
        let file_ids: Vec<FileId> = (0..num_files).map(|i| FileId(i as u32)).collect();
        let global = GlobalContext::empty();
        SharedState::new(global, file_ids)
    }

    #[test]
    fn test_file_status_transitions() {
        // NotStarted → Parsing
        assert_eq!(FileStatus::NotStarted as u8, 0);
        assert!(FileStatus::NotStarted < FileStatus::Parsing);

        // Parsing → SymbolTreeReady
        assert!(FileStatus::Parsing < FileStatus::SymbolTreeReady);

        // SymbolTreeReady → Completed
        assert!(FileStatus::SymbolTreeReady < FileStatus::Completed);
    }

    #[test]
    fn test_try_claim_by_us() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        // First claim should succeed
        assert_eq!(state.try_claim(file_id), ClaimResult::ByUs);
        assert_eq!(state.file_status(file_id), FileStatus::Parsing);

        // Second claim should fail (we already claimed it)
        assert_eq!(state.try_claim(file_id), ClaimResult::ByOther);
    }

    #[test]
    fn test_claim_next_file() {
        let state = create_test_state(5);

        // Claim files sequentially
        assert_eq!(state.claim_next_file(), Some(FileId(0)));
        assert_eq!(state.claim_next_file(), Some(FileId(1)));
        assert_eq!(state.claim_next_file(), Some(FileId(2)));
        assert_eq!(state.claim_next_file(), Some(FileId(3)));
        assert_eq!(state.claim_next_file(), Some(FileId(4)));

        // No more files
        assert_eq!(state.claim_next_file(), None);
    }

    #[test]
    fn test_publish_and_get_symbol_tree() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        // Claim file
        assert_eq!(state.try_claim(file_id), ClaimResult::ByUs);

        // Create minimal parse for SymbolTree
        let module_id = hir_def::ModuleId::new(file_id);
        let parse = parser::parse(""); // Empty file
        let item_tree = hir_def::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));

        // Publish symbol tree
        state.publish_symbol_tree(file_id, symbol_tree.clone());

        // Check status
        assert_eq!(state.file_status(file_id), FileStatus::SymbolTreeReady);
        assert!(state.is_symbol_tree_ready(file_id));

        // Get symbol tree
        let retrieved = state.get_symbol_tree(file_id).unwrap();
        assert!(Arc::ptr_eq(&symbol_tree, &retrieved));
    }

    #[test]
    fn test_mark_completed() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        // Claim and publish
        state.try_claim(file_id);
        let module_id = hir_def::ModuleId::new(file_id);
        let parse = parser::parse(""); // Empty file
        let item_tree = hir_def::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id));
        state.publish_symbol_tree(file_id, symbol_tree);

        // Mark completed
        state.mark_completed(file_id);
        assert_eq!(state.file_status(file_id), FileStatus::Completed);

        // Try to claim completed file
        assert_eq!(state.try_claim(file_id), ClaimResult::AlreadyDone);
    }

    #[test]
    fn test_concurrent_claim() {
        let state = create_test_state(100);
        let state_clone1 = Arc::clone(&state);
        let state_clone2 = Arc::clone(&state);

        // Spawn two threads claiming files
        let handle1 = thread::spawn(move || {
            let mut claimed = vec![];
            while let Some(file_id) = state_clone1.claim_next_file() {
                claimed.push(file_id);
            }
            claimed
        });

        let handle2 = thread::spawn(move || {
            let mut claimed = vec![];
            while let Some(file_id) = state_clone2.claim_next_file() {
                claimed.push(file_id);
            }
            claimed
        });

        let claimed1 = handle1.join().unwrap();
        let claimed2 = handle2.join().unwrap();

        // Total should be 100 files
        assert_eq!(claimed1.len() + claimed2.len(), 100);

        // No file should be claimed twice
        let mut all_claimed = claimed1;
        all_claimed.extend(claimed2);
        all_claimed.sort_by_key(|f| f.index());
        all_claimed.dedup();
        assert_eq!(all_claimed.len(), 100);
    }

    #[test]
    fn test_mark_failed() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        // Claim file
        state.try_claim(file_id);

        // Mark as failed
        let error: Arc<str> = Arc::from("Test error");
        state.mark_failed(file_id, error.clone());

        // Check status
        assert_eq!(state.file_status(file_id), FileStatus::Completed);

        // Check error recorded
        assert_eq!(state.failed_files.get(&file_id).unwrap().as_ref(), "Test error");
    }
}
