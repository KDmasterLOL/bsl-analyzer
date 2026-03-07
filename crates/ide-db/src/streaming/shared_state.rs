//! Shared state for worker pool coordination.
//!
//! This module implements lock-free file claiming and synchronization
//! for parallel file processing with minimal blocking.

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crossbeam_utils::CachePadded;
use dashmap::DashMap;
use hir::{
    ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree, WorkspaceSymbols,
};
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxBuildHasher;
use syntax::{Parse, SyntaxNode};
use vfs::{file_set::FileSet, FileId};

use super::{FileReader, GlobalContext};

/// Cached parsed file data with lazy HIR/CFG computation.
///
/// Contains text, AST, ItemTree and lazily computed HIR/CFG for a file.
/// This cache eliminates redundant parsing and HIR lowering during:
/// - Phase 1: item_tree() no longer re-parses
/// - Phase 2: diagnostics collection reuses parsed data AND HIR/CFG
///
/// Memory: ~10-50 KB per file (base) + ~500 KB-1 MB if HIR computed
/// Lifecycle: Created in Phase 1, removed after Phase 2
pub struct ParsedFile {
    /// Original source text (needed for diagnostics output conversion).
    pub text: Arc<str>,

    /// Parsed AST (green tree + errors).
    pub parse: Arc<Parse<SyntaxNode>>,

    /// ItemTree (method signatures, no bodies).
    pub item_tree: Arc<ItemTree>,

    /// Module ID for this file.
    module_id: ModuleId,

    /// File path for metadata loading.
    file_path: Option<Arc<str>>,

    /// Lazily computed module bodies (HIR).
    /// Computed on first access during Phase 2.
    module_bodies: OnceLock<Arc<ModuleBodies>>,

    /// Lazily computed CFGs for all methods.
    /// Computed on first access during Phase 2 (requires module_bodies).
    module_cfgs: OnceLock<Arc<hir::cfg::ModuleCfgs>>,

    /// Lazily computed SDBL HIR for all queries.
    /// Computed on first access during Phase 2 (requires configuration for type inference).
    sdbl_hir: OnceLock<crate::SdblHirEntries>,

    /// Lazily computed module metadata (FormModule handlers, CommonModule context, etc.).
    /// Computed on first access during Phase 2 (requires configuration).
    module_metadata: OnceLock<Arc<ModuleMetadata>>,
}

impl std::fmt::Debug for ParsedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedFile")
            .field("text_len", &self.text.len())
            .field("module_id", &self.module_id)
            .field("has_bodies", &self.module_bodies.get().is_some())
            .field("has_cfgs", &self.module_cfgs.get().is_some())
            .field("has_sdbl_hir", &self.sdbl_hir.get().is_some())
            .field("has_metadata", &self.module_metadata.get().is_some())
            .finish()
    }
}

impl ParsedFile {
    /// Create a new ParsedFile with lazy HIR/CFG/metadata.
    pub fn new(
        text: Arc<str>,
        parse: Arc<Parse<SyntaxNode>>,
        item_tree: Arc<ItemTree>,
        module_id: ModuleId,
        file_path: Option<Arc<str>>,
    ) -> Self {
        Self {
            text,
            parse,
            item_tree,
            module_id,
            file_path,
            module_bodies: OnceLock::new(),
            module_cfgs: OnceLock::new(),
            sdbl_hir: OnceLock::new(),
            module_metadata: OnceLock::new(),
        }
    }

    /// Get or compute module bodies (HIR).
    ///
    /// Thread-safe: computed only once even with concurrent access.
    /// Uses `OnceLock` for lazy initialization.
    pub fn module_bodies(&self) -> Arc<ModuleBodies> {
        self.module_bodies
            .get_or_init(|| Arc::new(ModuleBodies::from_parse(&self.parse, self.module_id)))
            .clone()
    }

    /// Get or compute module CFGs.
    ///
    /// Thread-safe: computed only once even with concurrent access.
    /// Requires module_bodies (which will be computed if needed).
    pub fn module_cfgs(&self) -> Arc<hir::cfg::ModuleCfgs> {
        self.module_cfgs
            .get_or_init(|| {
                let bodies = self.module_bodies();
                let mut cfgs = rustc_hash::FxHashMap::default();

                for (local_id, body) in bodies.iter_bodies() {
                    let source_map = bodies.source_map(local_id);
                    let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(
                        body.body_stmts_typed(),
                        body,
                        source_map,
                    );
                    cfgs.insert(local_id, Arc::new(cfg));
                }

                Arc::new(hir::cfg::ModuleCfgs::new(cfgs))
            })
            .clone()
    }

    /// Get or compute SDBL HIR for all queries in file.
    ///
    /// Thread-safe: computed only once even with concurrent access.
    /// Requires configuration for metadata-based type inference.
    ///
    /// # Arguments
    /// * `configuration` - Optional 1C configuration metadata for type inference
    pub fn sdbl_hir(
        &self,
        configuration: Option<&Arc<bsl_metadata::Configuration>>,
    ) -> crate::SdblHirEntries {
        self.sdbl_hir
            .get_or_init(|| {
                let module_bodies = self.module_bodies();
                // Clone Arc (cheap) to pass to lower_sdbl_to_hir
                let config_arc = configuration.cloned();

                // Collect queries with position for sorting
                let mut queries_with_pos: Vec<_> = Vec::new();

                // Collect from method bodies
                for (local_id, body) in module_bodies.iter_bodies() {
                    for (expr_id, query_info) in body.sdbl_exprs() {
                        if let Some(ref sdbl_ast) = query_info.query_ast {
                            let pos = query_info.bsl_literal_range.start();
                            let sdbl_expr_id = hir::SdblExprId::from_method(local_id, expr_id);
                            queries_with_pos.push((pos, sdbl_expr_id, sdbl_ast.clone()));
                        }
                    }
                }

                // Collect from module-level code
                if let Some(module_code) = module_bodies.module_code() {
                    for (expr_id, query_info) in module_code.sdbl_exprs() {
                        if let Some(ref sdbl_ast) = query_info.query_ast {
                            let pos = query_info.bsl_literal_range.start();
                            let sdbl_expr_id = hir::SdblExprId::from_module_code(expr_id);
                            queries_with_pos.push((pos, sdbl_expr_id, sdbl_ast.clone()));
                        }
                    }
                }

                // Sort by position for deterministic output
                queries_with_pos.sort_by_key(|(pos, _, _)| *pos);

                // Lower to HIR (pass Arc directly to avoid cloning Configuration)
                let result: Vec<_> = queries_with_pos
                    .into_iter()
                    .map(|(_, sdbl_expr_id, sdbl_ast)| {
                        let sdbl_package =
                            sdbl_hir::lower_sdbl_to_hir(&sdbl_ast, config_arc.clone());
                        (sdbl_expr_id, Arc::new(sdbl_package))
                    })
                    .collect();

                Arc::new(result)
            })
            .clone()
    }

    /// Get or compute module metadata.
    ///
    /// Thread-safe: computed only once even with concurrent access.
    /// Loads form handlers (for FormModule), execution context (for CommonModule), etc.
    ///
    /// # Arguments
    /// * `configuration` - Optional 1C configuration metadata for module resolution
    pub fn module_metadata(
        &self,
        configuration: Option<&bsl_metadata::Configuration>,
    ) -> Arc<ModuleMetadata> {
        self.module_metadata
            .get_or_init(|| {
                let file_path = match &self.file_path {
                    Some(path) => std::path::Path::new(path.as_ref()),
                    None => {
                        return Arc::new(ModuleMetadata::unknown(
                            bsl_metadata::ModuleType::Unknown,
                        ));
                    }
                };
                Arc::new(crate::build_module_metadata(file_path, configuration))
            })
            .clone()
    }
}

/// File processing status - single source of truth.
///
/// Transitions are monotonic:
/// NotStarted → Parsing → SymbolTreeReady → DiagnosticsInProgress → Completed
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FileStatus {
    /// No worker has started processing this file.
    NotStarted = 0,

    /// Worker is parsing and building ItemTree/SymbolTree.
    /// SymbolTree NOT yet available.
    Parsing = 1,

    /// SymbolTree has been published to shared cache.
    /// File may need Phase 2 (diagnostics) if processed recursively.
    SymbolTreeReady = 2,

    /// Worker is computing diagnostics for this file.
    /// Used to prevent double-claiming during Phase 2 pass.
    DiagnosticsInProgress = 3,

    /// File processing completely finished.
    /// All resources released, cache cleared.
    Completed = 4,
}

impl FileStatus {
    /// Convert from u8 (used with atomics).
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(FileStatus::NotStarted),
            1 => Some(FileStatus::Parsing),
            2 => Some(FileStatus::SymbolTreeReady),
            3 => Some(FileStatus::DiagnosticsInProgress),
            4 => Some(FileStatus::Completed),
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

    /// File not ready for this operation (wrong status).
    NotReady,
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

    // === PARSED FILE CACHE (concurrent hashmap) ===
    /// Cache of parsed files for avoiding redundant parsing.
    /// Memory: ~10-50 KB per file (depends on file size).
    /// Lifecycle:
    /// - Populated during Phase 1 (build_and_publish_symbol_tree)
    /// - Consumed during Phase 2 (diagnostics collection)
    /// - Removed after Phase 2 completion (mark_completed)
    parsed_files: DashMap<FileId, Arc<ParsedFile>, FxBuildHasher>,

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

            parsed_files: DashMap::with_hasher_and_shard_amount(FxBuildHasher, 16),

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
                ClaimResult::ByOther | ClaimResult::AlreadyDone | ClaimResult::NotReady => {
                    // Another worker got it first or file not ready, try next file
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

        // Lock mutex FIRST to prevent lost wakeup race condition
        let _guard = self.mutexes[idx].lock();

        // Insert into concurrent hashmap (internally synchronized)
        self.symbol_trees.insert(file_id, tree);

        // Mark as ready (Parsing → SymbolTreeReady)
        // SeqCst ensures insert happens-before status change
        self.file_statuses[idx].store(FileStatus::SymbolTreeReady as u8, Ordering::SeqCst);

        // Wake all threads waiting for this SymbolTree
        // MUST be called under mutex to prevent lost wakeup
        self.condvars[idx].notify_all();

        // Drop guard (unlock)
    }

    /// Publish SymbolTree and immediately transition to DiagnosticsInProgress.
    ///
    /// This is used by full-cycle processing (process_file) to avoid the
    /// intermediate SymbolTreeReady state that could be grabbed by the second pass.
    ///
    /// Transitions: Parsing → DiagnosticsInProgress (skips SymbolTreeReady)
    ///
    /// The SymbolTree is still published and available for other threads waiting.
    pub fn publish_symbol_tree_and_start_diagnostics(
        &self,
        file_id: FileId,
        tree: Arc<SymbolTree>,
    ) {
        let idx = file_id.index() as usize;

        // Lock mutex FIRST to prevent lost wakeup race condition
        let _guard = self.mutexes[idx].lock();

        // Insert into concurrent hashmap (internally synchronized)
        self.symbol_trees.insert(file_id, tree);

        // Mark as DiagnosticsInProgress directly (Parsing → DiagnosticsInProgress)
        // This skips SymbolTreeReady to prevent second pass from grabbing it
        self.file_statuses[idx].store(FileStatus::DiagnosticsInProgress as u8, Ordering::SeqCst);

        // Wake all threads waiting for this SymbolTree
        // (is_symbol_tree_ready checks >= SymbolTreeReady, so DiagnosticsInProgress works)
        // MUST be called under mutex to prevent lost wakeup
        self.condvars[idx].notify_all();

        // Drop guard (unlock)
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
    // Parsed File Cache
    // ========================================================================

    /// Store parsed file data in cache.
    ///
    /// Called after Phase 1 parsing, before SymbolTree publish.
    /// Data will be used by Phase 2 and then removed.
    pub fn cache_parsed_file(&self, file_id: FileId, parsed: Arc<ParsedFile>) {
        self.parsed_files.insert(file_id, parsed);
    }

    /// Get cached parsed file data.
    ///
    /// Returns None if not cached (should not happen in normal flow).
    pub fn get_parsed_file(&self, file_id: FileId) -> Option<Arc<ParsedFile>> {
        self.parsed_files.get(&file_id).map(|r| r.clone())
    }

    /// Remove parsed file from cache.
    ///
    /// Called after Phase 2 completion to free memory.
    /// Critical for memory management - AST can be large.
    pub fn remove_parsed_file(&self, file_id: FileId) {
        self.parsed_files.remove(&file_id);
    }

    /// Get the number of cached ParsedFile entries.
    ///
    /// For testing: verify cache is cleared after processing.
    pub fn parsed_cache_len(&self) -> usize {
        self.parsed_files.len()
    }

    /// Check if a file has a cached ParsedFile.
    ///
    /// For testing: verify specific file is in/out of cache.
    pub fn has_parsed_file(&self, file_id: FileId) -> bool {
        self.parsed_files.contains_key(&file_id)
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

        // Lock mutex FIRST to prevent lost wakeup race condition
        let _guard = self.mutexes[idx].lock();

        // Record error
        self.failed_files.insert(file_id, error);

        // Move to Completed to unblock waiters
        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);

        // Wake waiting threads so they can detect failure
        // MUST be called under mutex to prevent lost wakeup
        self.condvars[idx].notify_all();

        // Drop guard (unlock)
    }

    // ========================================================================
    // Phase 2 (Diagnostics) Claiming
    // ========================================================================

    /// Try to claim a file for Phase 2 (diagnostics) processing.
    ///
    /// Only claims files in SymbolTreeReady status.
    /// Used during the second pass to process recursively-resolved files.
    ///
    /// Returns ClaimResult::ByUs if transition SymbolTreeReady → DiagnosticsInProgress succeeded.
    pub fn try_claim_for_diagnostics(&self, file_id: FileId) -> ClaimResult {
        let idx = file_id.index() as usize;

        match self.file_statuses[idx].compare_exchange(
            FileStatus::SymbolTreeReady as u8,
            FileStatus::DiagnosticsInProgress as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => ClaimResult::ByUs,
            Err(current) => {
                if current >= FileStatus::Completed as u8 {
                    ClaimResult::AlreadyDone
                } else if current == FileStatus::DiagnosticsInProgress as u8 {
                    ClaimResult::ByOther
                } else {
                    // NotStarted, Parsing - file not ready for Phase 2 yet
                    ClaimResult::NotReady
                }
            }
        }
    }

    /// Get number of files (for iteration in second pass).
    pub fn num_files(&self) -> usize {
        self.sorted_files.len()
    }

    /// Get file ID by index (for iteration in second pass).
    pub fn file_id_at(&self, idx: usize) -> Option<FileId> {
        self.sorted_files.get(idx).copied()
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

        // SymbolTreeReady → DiagnosticsInProgress
        assert!(FileStatus::SymbolTreeReady < FileStatus::DiagnosticsInProgress);

        // DiagnosticsInProgress → Completed
        assert!(FileStatus::DiagnosticsInProgress < FileStatus::Completed);
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
        let module_id = hir::ModuleId::new(file_id);
        let text = "";
        let parse = parser::parse(text); // Empty file
        let item_tree = hir::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));

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
        let module_id = hir::ModuleId::new(file_id);
        let text = "";
        let parse = parser::parse(text); // Empty file
        let item_tree = hir::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));
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

    #[test]
    fn test_cache_parsed_file() {
        use hir::{ItemTree, ModuleId};
        use syntax::Parse;

        let state = create_test_state(10);
        let file_id = FileId(0);

        // Create minimal ParsedFile
        let text: Arc<str> = Arc::from("Процедура Тест() КонецПроцедуры");
        let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text));
        let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
        let module_id = ModuleId::new(file_id);

        let parsed = Arc::new(ParsedFile::new(text.clone(), parse, item_tree, module_id, None));

        // Cache it
        state.cache_parsed_file(file_id, Arc::clone(&parsed));

        // Get it back
        let retrieved = state.get_parsed_file(file_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().text.as_ref(), text.as_ref());

        // Remove it
        state.remove_parsed_file(file_id);
        assert!(state.get_parsed_file(file_id).is_none());
    }

    #[test]
    fn test_try_claim_for_diagnostics() {
        use hir::{ItemTree, ModuleId, SymbolTree};
        use syntax::Parse;

        let state = create_test_state(10);
        let file_id = FileId(0);

        // Cannot claim for diagnostics if NotStarted
        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::NotReady);

        // Claim and process to SymbolTreeReady
        state.try_claim(file_id);

        // Build minimal SymbolTree
        let text = "Процедура Тест() КонецПроцедуры";
        let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(text));
        let item_tree = ItemTree::from_parse(&parse);
        let module_id = ModuleId::new(file_id);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));

        state.publish_symbol_tree(file_id, symbol_tree);

        // Now we can claim for diagnostics
        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::ByUs);
        assert_eq!(state.file_status(file_id), FileStatus::DiagnosticsInProgress);

        // Second claim should fail
        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::ByOther);
    }

    #[test]
    fn test_num_files_and_file_id_at() {
        let state = create_test_state(5);

        assert_eq!(state.num_files(), 5);
        assert_eq!(state.file_id_at(0), Some(FileId(0)));
        assert_eq!(state.file_id_at(4), Some(FileId(4)));
        assert_eq!(state.file_id_at(5), None);
    }

    #[test]
    fn test_cache_lifecycle() {
        use hir::{ItemTree, ModuleId};
        use syntax::Parse;

        let state = create_test_state(3);
        let file_0 = FileId(0);
        let file_1 = FileId(1);

        // Initially cache is empty
        assert_eq!(state.parsed_cache_len(), 0);
        assert!(!state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));

        // Cache first file
        let text_0: Arc<str> = Arc::from("Процедура Тест0() КонецПроцедуры");
        let parse_0: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text_0));
        let item_tree_0: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse_0));
        let module_id_0 = ModuleId::new(file_0);
        let parsed_0 = Arc::new(ParsedFile::new(text_0, parse_0, item_tree_0, module_id_0, None));
        state.cache_parsed_file(file_0, parsed_0);

        // Verify cache state after first file
        assert_eq!(state.parsed_cache_len(), 1);
        assert!(state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));

        // Cache second file
        let text_1: Arc<str> = Arc::from("Процедура Тест1() КонецПроцедуры");
        let parse_1: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text_1));
        let item_tree_1: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse_1));
        let module_id_1 = ModuleId::new(file_1);
        let parsed_1 = Arc::new(ParsedFile::new(text_1, parse_1, item_tree_1, module_id_1, None));
        state.cache_parsed_file(file_1, parsed_1);

        // Verify cache state after second file
        assert_eq!(state.parsed_cache_len(), 2);
        assert!(state.has_parsed_file(file_0));
        assert!(state.has_parsed_file(file_1));

        // Remove first file from cache
        state.remove_parsed_file(file_0);
        assert_eq!(state.parsed_cache_len(), 1);
        assert!(!state.has_parsed_file(file_0));
        assert!(state.has_parsed_file(file_1));

        // Remove second file from cache
        state.remove_parsed_file(file_1);
        assert_eq!(state.parsed_cache_len(), 0);
        assert!(!state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));
    }
}
