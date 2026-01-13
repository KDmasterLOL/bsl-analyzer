//! Global context for streaming analysis.

use std::sync::Arc;

use bsl_metadata::Configuration;
use hir_def::{ModuleIndex, SymbolTree, WorkspaceSymbols};
use rustc_hash::FxHashMap;
use vfs::{file_set::FileSet, FileId};

use super::file_reader::FileReader;

/// Global context shared across all files during streaming analysis.
///
/// This structure holds data that needs to be available for cross-module
/// validation and is kept in memory for the entire analysis session.
#[derive(Debug, Clone)]
pub struct GlobalContext {
    /// 1C Configuration metadata.
    pub configuration: Option<Arc<Configuration>>,

    /// Symbol trees for all modules (prebuilt during initialization).
    pub symbol_trees: FxHashMap<FileId, Arc<SymbolTree>>,

    /// Workspace symbols index for cross-module resolution.
    pub workspace_symbols: Arc<WorkspaceSymbols>,

    /// Module index (name → FileId).
    pub module_index: Arc<ModuleIndex>,

    /// File set for path resolution.
    pub file_set: Arc<FileSet>,

    /// File content provider.
    pub file_reader: FileReader,
}

impl GlobalContext {
    /// Create a new GlobalContext with the given components.
    pub fn new(
        configuration: Option<Arc<Configuration>>,
        symbol_trees: FxHashMap<FileId, Arc<SymbolTree>>,
        workspace_symbols: Arc<WorkspaceSymbols>,
        module_index: Arc<ModuleIndex>,
        file_set: Arc<FileSet>,
        file_reader: FileReader,
    ) -> Self {
        Self { configuration, symbol_trees, workspace_symbols, module_index, file_set, file_reader }
    }

    /// Create an empty GlobalContext (useful for testing).
    pub fn empty() -> Self {
        Self {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(FileSet::default()),
            file_reader: FileReader::empty(),
        }
    }
}
