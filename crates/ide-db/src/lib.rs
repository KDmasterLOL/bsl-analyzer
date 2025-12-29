//! IDE database for bsl-analyzer.
//!
//! This crate provides the database for IDE functionality with full DefDatabase implementation.

use std::hash::BuildHasherDefault;
use std::sync::Arc;

use base_db::{Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use dashmap::DashMap;
use hir_def::{DefDatabase, ItemTree, ModuleData, ModuleId};
use rustc_hash::FxHasher;
use vfs::FileId;

// Re-export commonly used types
pub use base_db;
pub use hir_def;
pub use syntax::TextRange;
pub use vfs;

// ========== Symbol types (TODO: full implementation in later iterations) ==========

/// Symbol kind (procedure, function, variable, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    // TODO: Add more symbol kinds as needed
}

/// Symbol information.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    // TODO: Add more fields as needed
}

/// The root database for IDE operations.
///
/// This database extends SourceDatabase and RootQueryDb with DefDatabase,
/// providing full HIR functionality with caching.
pub trait RootDatabase: SourceDatabase + RootQueryDb + DefDatabase {}

/// Default implementation of RootDatabase with caching.
///
/// Uses DashMap for thread-safe caching of all queries.
#[derive(Debug, Clone)]
pub struct RootDatabaseImpl {
    /// Base file storage
    files: Files,

    /// HIR caches
    item_tree_cache: Arc<DashMap<FileId, Arc<ItemTree>, BuildHasherDefault<FxHasher>>>,
    module_data_cache: Arc<DashMap<ModuleId, Arc<ModuleData>, BuildHasherDefault<FxHasher>>>,
}

impl Default for RootDatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl RootDatabaseImpl {
    /// Create a new empty database.
    pub fn new() -> Self {
        Self {
            files: Files::new(),
            item_tree_cache: Arc::new(DashMap::default()),
            module_data_cache: Arc::new(DashMap::default()),
        }
    }

    /// Invalidate HIR caches for a file.
    ///
    /// Called when file content changes.
    fn invalidate_file(&self, file_id: FileId) {
        self.item_tree_cache.remove(&file_id);
        self.module_data_cache.remove(&ModuleId::new(file_id));
    }
}

// ========== SourceDatabase ==========

impl SourceDatabase for RootDatabaseImpl {
    fn file_text(&self, file_id: FileId) -> Arc<str> {
        self.files.file_text(file_id)
    }

    fn file_source_root(&self, file_id: FileId) -> SourceRootId {
        self.files.file_source_root(file_id)
    }

    fn source_root(&self, id: SourceRootId) -> Arc<SourceRoot> {
        self.files.source_root(id)
    }

    fn set_file_text(&mut self, file_id: FileId, text: &str) {
        self.files.set_file_text(file_id, text);
        self.invalidate_file(file_id);
    }

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
        self.files.set_file_source_root(file_id, source_root_id);
    }

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: Arc<SourceRoot>) {
        self.files.set_source_root(source_root_id, source_root);
    }
}

// ========== RootQueryDb ==========

impl RootQueryDb for RootDatabaseImpl {
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
        self.files.parse(self, file_id)
    }
}

// ========== DefDatabase ==========

impl DefDatabase for RootDatabaseImpl {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        // Check cache first
        if let Some(cached) = self.item_tree_cache.get(&file_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("item_tree", ?file_id).entered();

        // Lower AST → ItemTree
        let tree = hir_def::item_tree::lower_file(self, file_id);

        // Cache the result
        self.item_tree_cache.insert(file_id, tree.clone());
        tree
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
        // Check cache first
        if let Some(cached) = self.module_data_cache.get(&module_id) {
            return cached.value().clone();
        }

        let _span = tracing::info_span!("module_data", ?module_id).entered();

        // Get ItemTree and convert to ModuleData
        let tree = self.item_tree(module_id.file_id);
        let data = Arc::new(ModuleData::from_item_tree(module_id, tree));

        // Cache the result
        self.module_data_cache.insert(module_id, data.clone());
        data
    }
}

// ========== RootDatabase ==========

impl RootDatabase for RootDatabaseImpl {}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::{file_set::FileSet, VfsPath};

    #[test]
    fn test_root_database_basic() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        // Test parse query
        let parse = db.parse(file_id);
        assert!(!parse.has_errors());

        // Test item_tree query
        let tree = db.item_tree(file_id);
        assert_eq!(tree.top_level_items().len(), 1);

        // Test module_data query
        let module_id = ModuleId::new(file_id);
        let module_data = db.module_data(module_id);
        assert_eq!(module_data.procedures.len(), 1);
        assert_eq!(module_data.functions.len(), 0);
        assert_eq!(module_data.variables.len(), 0);
    }

    #[test]
    fn test_incremental_item_tree() {
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
        db.set_file_source_root(file_id, SourceRootId(0));

        // Initial content
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let tree1 = db.item_tree(file_id);
        assert_eq!(tree1.top_level_items().len(), 1);

        // Change content - should invalidate cache
        db.set_file_text(
            file_id,
            r#"
Процедура Тест1() КонецПроцедуры
Функция Тест2() КонецФункции
        "#,
        );
        let tree2 = db.item_tree(file_id);
        assert_eq!(tree2.top_level_items().len(), 2);
    }
}
