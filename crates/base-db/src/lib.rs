//! Base database for bsl-analyzer.
//!
//! This crate provides the foundation for incremental computation using Salsa.
//! It defines the core database traits and types for managing source files and parsing.

use std::hash::BuildHasherDefault;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHasher;
use vfs::{FileId, VfsPath};

mod change;
mod input;
mod locale;
mod queries;

pub use change::FileChange;
pub use input::{
    DiagnosticsConfigId, DiagnosticsConfigInput, FileIdInput, FileSourceRootInput, FileTextInput,
    SourceRoot, SourceRootId, SourceRootInput,
};
pub use locale::{Locale, UnknownLocale};
pub use queries::{method_regions_query, parse_query, resolve_vfs_path_query};

/// The main Salsa database trait for source file operations.
///
/// This trait provides access to file contents and source root information.
/// It uses Salsa for automatic dependency tracking and cache invalidation.
#[salsa::db]
pub trait SourceDatabase: salsa::Database {
    fn file_text_input(&self, file_id: FileId) -> FileTextInput;

    fn source_root_input(&self, source_root_id: SourceRootId) -> SourceRootInput;

    fn file_source_root_input(&self, file_id: FileId) -> FileSourceRootInput;

    fn set_file_text(&mut self, file_id: FileId, text: &str);

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId);

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot);

    /// Resolve a VfsPath to FileId within a SourceRoot.
    ///
    /// This is a convenience method that wraps resolve_vfs_path_query.
    /// Used by diagnostics to resolve metadata URIs to FileIds.
    fn resolve_vfs_path(&self, source_root_id: SourceRootId, vfs_path: &VfsPath) -> Option<FileId>;
}

/// Base-level queries for BSL parsing and region extraction.
///
/// # Query Group Organization
///
/// This trait defines the foundation layer of BSL analysis queries.
/// All queries are Salsa-cached and automatically invalidated when file content changes.
///
/// **Dependencies:** SourceDatabase (file inputs)
/// **Used by:** DefDatabase (HIR lowering)
///
/// # Query Categories
///
/// ## Parsing (Core)
/// - [`parse`](Self::parse) - BSL file → AST (LRU: 512)
///
/// ## Region Analysis
/// - [`method_regions`](Self::method_regions) - Methods in API regions (LRU: 256)
///
/// Module-level regions are exposed via `hir::RegionTree::module_level_regions`.
///
/// # Implementation Pattern
///
/// Implementations delegate to tracked query functions in the `queries` module:
///
/// ```ignore
/// impl RootQueryDb for MyDatabase {
///     fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
///         let input = self.file_text_input(file_id);
///         parse_query(self, input)
///     }
/// }
/// ```
#[salsa::db]
pub trait RootQueryDb: SourceDatabase {
    /// Parse a BSL file into a syntax tree.
    ///
    /// This is the foundational query for all BSL analysis. It converts raw BSL source text
    /// into a concrete syntax tree (CST) using the Rowan library.
    ///
    /// # Performance
    /// - **LRU cache:** 512 files (frequently accessed)
    /// - **Depends on:** `file_text_input` (Salsa input)
    /// - **Typical time:** 5-15ms for medium files (after parsing)
    ///
    /// # Invalidation
    /// Automatically invalidated by Salsa when file text changes.
    ///
    /// # Implementation
    /// Should delegate to [`parse_query`].
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode>;

    /// Map method source ranges to their parent API region names.
    ///
    /// Returns a HashMap mapping TextRange (of method definitions) to their
    /// parent API region name (ПрограммныйИнтерфейс, Public, СлужебныйПрограммныйИнтерфейс, Internal).
    ///
    /// Only methods inside API regions are included in the map.
    /// For nested regions, the root (top-level) region name is returned.
    ///
    /// # Performance
    /// - **LRU cache:** 256 files (region analysis is inexpensive)
    /// - **Depends on:** [`parse`](Self::parse)
    /// - **Shared usage:** Multiple region-based diagnostics
    ///
    /// # Usage Example
    ///
    /// ```ignore
    /// let method_regions = db.method_regions(file_id);
    /// let item_tree = db.item_tree(file_id);
    ///
    /// for (_, proc) in item_tree.procedures() {
    ///     if let Some(region_name) = method_regions.get(&proc.source_range) {
    ///         // proc is in an API region named region_name
    ///     }
    /// }
    /// ```
    ///
    /// # Implementation
    /// Should delegate to [`method_regions_query`].
    fn method_regions(
        &self,
        file_id: FileId,
    ) -> Arc<std::collections::HashMap<syntax::TextRange, String>>;
}

/// Helper structure for managing file state with concurrent access.
///
/// Uses DashMap for lock-free concurrent access to Salsa input structs.
/// `Files` stores Salsa input handles outside Salsa itself.
///
/// # Locking invariant (ABBA deadlock prevention)
///
/// Setters in this struct MUST NOT hold a DashMap shard guard across a Salsa
/// setter call. The generated `input.set_<field>(db)` acquires `zalsa_mut()`
/// internally, which blocks until every live database handle has been
/// dropped. If a worker thread is blocked acquiring a read-guard on the same
/// shard (via `file_text()` / `file_source_root()` / `source_root()`) while
/// the main thread holds the write-guard, the worker cannot release its
/// database handle and the main thread cannot finish the setter — classic
/// ABBA.
///
/// The fix pattern: look up the existing handle under a short `get()` guard,
/// copy the Salsa input handle (it is `Copy`), drop the guard, and only then
/// invoke the Salsa setter. Single-mutator invariant (only the LSP main loop
/// writes) makes the Vacant/Insert race harmless — `debug_assert!` in the
/// None branches acts as a tripwire if that invariant is ever violated.
///
/// Paired with an early `trigger_cancellation()` at the LSP `process_changes`
/// entry point (see `AnalysisHost::request_cancellation`) which forces any
/// long-lived database clones held by background workers to be dropped
/// before setters run.
#[derive(Debug, Default, Clone)]
pub struct Files {
    file_texts: Arc<DashMap<FileId, FileTextInput, BuildHasherDefault<FxHasher>>>,
    source_roots: Arc<DashMap<SourceRootId, SourceRootInput, BuildHasherDefault<FxHasher>>>,
    file_source_roots: Arc<DashMap<FileId, FileSourceRootInput, BuildHasherDefault<FxHasher>>>,
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    /// # Panics
    ///
    /// Panics if the file has not been set.
    pub fn file_text(&self, file_id: FileId) -> FileTextInput {
        self.file_texts.get(&file_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?file_id, "file text not set — this is a programming error, all files must be loaded before queries run");
            panic!("file text not set for {:?}", file_id)
        })
    }

    /// Returns None if the file has not been loaded yet.
    pub fn try_file_text(&self, file_id: FileId) -> Option<FileTextInput> {
        self.file_texts.get(&file_id).map(|entry| *entry.value())
    }

    /// Set the text for a file.
    ///
    /// This creates or updates a Salsa input. Salsa automatically invalidates
    /// dependent queries when the text changes.
    pub fn set_file_text(&self, db: &mut dyn SourceDatabase, file_id: FileId, text: &str) {
        use salsa::Setter;

        // Short-lock pattern: look up existing handle under a brief guard,
        // drop the guard, and only then invoke the Salsa setter. See the
        // `Files` doc-comment for why this is required.
        let existing = self.file_texts.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_text(db).to(text.to_string());
            }
            None => {
                let input = FileTextInput::new(db, text.to_string());
                let previous = self.file_texts.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_text violates single-mutator invariant"
                );
            }
        }
    }

    /// Set the text for a file with explicit durability.
    ///
    /// This allows setting durability levels for library vs source code.
    pub fn set_file_text_with_durability(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        text: &str,
        durability: salsa::Durability,
    ) {
        use salsa::Setter;

        let existing = self.file_texts.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_text(db).with_durability(durability).to(text.to_string());
            }
            None => {
                let input = FileTextInput::builder(text.to_string()).durability(durability).new(db);
                let previous = self.file_texts.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_text_with_durability violates single-mutator invariant"
                );
            }
        }
    }

    /// Set the text for a file with automatic durability detection.
    ///
    /// Automatically determines durability based on the file's source root:
    /// - Library files (is_library = true): HIGH durability (rarely change)
    /// - User code (is_library = false): LOW durability (changes frequently)
    ///
    /// This is the recommended method for setting file text in production.
    pub fn set_file_text_smart(&self, db: &mut dyn SourceDatabase, file_id: FileId, text: &str) {
        // Resolve durability without holding any DashMap guards across the
        // Salsa setter call below. Salsa input handles are `Copy`, so we
        // release the shard guards before reading via `source_root_id(db)` /
        // `root(db)` (those are independent Salsa getters).
        let mapping = self.file_source_roots.get(&file_id).map(|e| *e.value());
        let durability = mapping.and_then(|mapping| {
            let source_root_id = mapping.source_root_id(db);
            let root_input = self.source_roots.get(&source_root_id).map(|e| *e.value())?;
            Some(root_input.root(db).durability())
        });

        match durability {
            Some(d) => {
                tracing::debug!(
                    ?file_id,
                    durability = ?d,
                    "set_file_text_smart: determined durability from source root"
                );
                self.set_file_text_with_durability(db, file_id, text, d);
            }
            None => {
                tracing::debug!(
                    ?file_id,
                    "set_file_text_smart: fallback to LOW durability (source root not set)"
                );
                self.set_file_text_with_durability(db, file_id, text, salsa::Durability::LOW);
            }
        }
    }

    /// # Panics
    ///
    /// Panics if the source root has not been set.
    pub fn source_root(&self, source_root_id: SourceRootId) -> SourceRootInput {
        self.source_roots.get(&source_root_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?source_root_id, "source root not set — this is a programming error");
            panic!("source root not set for {:?}", source_root_id)
        })
    }

    pub fn set_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        source_root_id: SourceRootId,
        source_root: SourceRoot,
    ) {
        use salsa::Setter;

        let existing = self.source_roots.get(&source_root_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_root(db).to(source_root);
            }
            None => {
                let input = SourceRootInput::new(db, source_root);
                let previous = self.source_roots.insert(source_root_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_source_root violates single-mutator invariant"
                );
            }
        }
    }

    /// # Panics
    ///
    /// Panics if the file source root mapping has not been set.
    pub fn file_source_root(&self, file_id: FileId) -> FileSourceRootInput {
        self.file_source_roots.get(&file_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?file_id, "file source root not set — this is a programming error");
            panic!("file source root not set for {:?}", file_id)
        })
    }

    pub fn set_file_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        source_root_id: SourceRootId,
    ) {
        use salsa::Setter;

        let existing = self.file_source_roots.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_source_root_id(db).to(source_root_id);
            }
            None => {
                let input = FileSourceRootInput::new(db, source_root_id);
                let previous = self.file_source_roots.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_source_root violates single-mutator invariant"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;

    #[salsa::db]
    #[derive(Clone, Default)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
        files: Files,
    }

    #[salsa::db]
    impl salsa::Database for TestDatabase {}

    #[salsa::db]
    impl SourceDatabase for TestDatabase {
        fn file_text_input(&self, file_id: FileId) -> FileTextInput {
            self.files.file_text(file_id)
        }

        fn source_root_input(&self, source_root_id: SourceRootId) -> SourceRootInput {
            self.files.source_root(source_root_id)
        }

        fn file_source_root_input(&self, file_id: FileId) -> FileSourceRootInput {
            self.files.file_source_root(file_id)
        }

        fn set_file_text(&mut self, file_id: FileId, text: &str) {
            let files = self.files.clone();
            files.set_file_text(self, file_id, text);
        }

        fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
            let files = self.files.clone();
            files.set_file_source_root(self, file_id, source_root_id);
        }

        fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot) {
            let files = self.files.clone();
            files.set_source_root(self, source_root_id, source_root);
        }

        fn resolve_vfs_path(
            &self,
            source_root_id: SourceRootId,
            vfs_path: &VfsPath,
        ) -> Option<FileId> {
            let source_root_input = self.source_root_input(source_root_id);
            let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
            resolve_vfs_path_query(self, source_root_input, vfs_path_str)
        }
    }

    #[salsa::db]
    impl RootQueryDb for TestDatabase {
        fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
            let input = self.file_text_input(file_id);
            parse_query(self, input)
        }

        fn method_regions(
            &self,
            file_id: FileId,
        ) -> Arc<std::collections::HashMap<syntax::TextRange, String>> {
            let input = self.file_text_input(file_id);
            method_regions_query(self, input)
        }
    }

    #[salsa::tracked(lru = 10)]
    fn test_fileid_query<'db>(
        db: &'db dyn salsa::Database,
        file_id_input: FileIdInput<'db>,
    ) -> u32 {
        file_id_input.file_id(db).0
    }

    #[test]
    fn test_fileid_salsa_compatible() {
        let db = TestDatabase::default();

        let file_id = FileId(42);
        let file_id_input = FileIdInput::new(&db, file_id);

        let result = test_fileid_query(&db, file_id_input);
        assert_eq!(result, 42);

        let result2 = test_fileid_query(&db, file_id_input);
        assert_eq!(result2, 42);

        let file_id2 = FileId(100);
        let file_id_input2 = FileIdInput::new(&db, file_id2);
        let result3 = test_fileid_query(&db, file_id_input2);
        assert_eq!(result3, 100);

        let file_id_input3 = FileIdInput::new(&db, file_id);
        assert_eq!(file_id_input, file_id_input3);
    }

    #[test]
    fn test_parse_query() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }

    #[test]
    fn test_incremental_reparse() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let parse1 = db.parse(file_id);
        assert!(!parse1.has_errors());

        let parse2 = db.parse(file_id);
        assert!(!parse2.has_errors());
        assert_eq!(parse1.syntax_node().text(), parse2.syntax_node().text());

        db.set_file_text(file_id, "Процедура Тест2() КонецПроцедуры");
        let parse3 = db.parse(file_id);
        assert!(!parse3.has_errors());
        assert_ne!(parse1.syntax_node().text(), parse3.syntax_node().text());
    }

    #[test]
    fn test_file_change_apply() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut change = FileChange::new();
        change.change_file(file_id, Some(Arc::from("Процедура Тест() КонецПроцедуры")));

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        change.set_roots(vec![source_root]);

        change.apply(&mut db);

        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }
}
