use std::path::PathBuf;
use std::sync::Arc;

use bsl_metadata::Configuration;
use hir::{ModuleIndex, SymbolTree, WorkspaceSymbols};
use rustc_hash::FxHashMap;
use vfs::{file_set::FileSet, FileId};

use super::file_reader::FileReader;

#[derive(Debug, Clone)]
pub struct GlobalContext {
    pub configuration: Option<Arc<Configuration>>,
    pub symbol_trees: FxHashMap<FileId, Arc<SymbolTree>>,
    pub workspace_symbols: Arc<WorkspaceSymbols>,
    pub module_index: Arc<ModuleIndex>,
    pub file_set: Arc<FileSet>,
    pub file_reader: FileReader,
    pub config_root: Option<PathBuf>,
}

impl GlobalContext {
    pub fn new(
        configuration: Option<Arc<Configuration>>,
        symbol_trees: FxHashMap<FileId, Arc<SymbolTree>>,
        workspace_symbols: Arc<WorkspaceSymbols>,
        module_index: Arc<ModuleIndex>,
        file_set: Arc<FileSet>,
        file_reader: FileReader,
    ) -> Self {
        Self {
            configuration,
            symbol_trees,
            workspace_symbols,
            module_index,
            file_set,
            file_reader,
            config_root: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(FileSet::default()),
            file_reader: FileReader::empty(),
            config_root: None,
        }
    }
}
