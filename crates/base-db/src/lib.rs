//! Base database for bsl-analyzer.
//!
//! This crate provides the foundation for incremental computation using Salsa.
//! It defines the core database traits and types for managing source files and parsing.
//!
//! Note: This is an initial implementation with simplified caching.
//! Full Salsa 0.25.2 integration (with tracked functions and proper ingredient registration)
//! will be completed in a later iteration.

use std::hash::BuildHasherDefault;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHasher;
use vfs::{FileId, VfsPath};

mod change;
mod input;
mod queries;

pub use change::FileChange;
pub use input::{
    DiagnosticsConfigId, DiagnosticsConfigInput, FileIdInput, FileSourceRootInput, FileTextInput,
    SourceRoot, SourceRootId, SourceRootInput,
};
pub use queries::{
    method_regions_query, module_level_regions_query, parse_query, resolve_vfs_path_query,
    RegionInfo,
};

/// The main Salsa database trait for source file operations.
///
/// This trait provides access to file contents and source root information.
/// It uses Salsa for automatic dependency tracking and cache invalidation.
#[salsa::db]
pub trait SourceDatabase: salsa::Database {
    /// Get the Salsa input for file text.
    fn file_text_input(&self, file_id: FileId) -> FileTextInput;

    /// Get the Salsa input for source root.
    fn source_root_input(&self, source_root_id: SourceRootId) -> SourceRootInput;

    /// Get the Salsa input for file source root mapping.
    fn file_source_root_input(&self, file_id: FileId) -> FileSourceRootInput;

    // Convenience setters (implemented by databases with Files helper)

    /// Set file text (requires Files helper in implementation).
    fn set_file_text(&mut self, file_id: FileId, text: &str);

    /// Set file source root mapping (requires Files helper in implementation).
    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId);

    /// Set source root (requires Files helper in implementation).
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
/// - [`module_level_regions`](Self::module_level_regions) - All top-level regions (LRU: 256)
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

    /// Extract all module-level regions from a BSL file.
    ///
    /// Returns cached list of region names with their source ranges (first line only).
    /// Only top-level regions are collected (nested regions inside methods are excluded).
    ///
    /// # Performance
    /// - **LRU cache:** 256 files (region collection is inexpensive)
    /// - **Depends on:** [`parse`](Self::parse)
    /// - **Shared usage:** 4+ region diagnostics (NonStandardRegion, DuplicateRegion, etc.)
    ///
    /// # Usage Example
    ///
    /// ```ignore
    /// let regions = db.module_level_regions(file_id);
    ///
    /// for region in regions.iter() {
    ///     // region.name - region name (e.g., "Public", "Internal")
    ///     // region.range - TextRange of first line of #Область directive
    /// }
    /// ```
    ///
    /// # Implementation
    /// Should delegate to [`module_level_regions_query`].
    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<RegionInfo>>;
}

/// Helper structure for managing file state with concurrent access.
///
/// Uses DashMap for lock-free concurrent access to Salsa input structs.
/// Note: Like rust-analyzer, we keep Files as a DashMap-based helper outside Salsa.
#[derive(Debug, Default, Clone)]
pub struct Files {
    file_texts: Arc<DashMap<FileId, FileTextInput, BuildHasherDefault<FxHasher>>>,
    source_roots: Arc<DashMap<SourceRootId, SourceRootInput, BuildHasherDefault<FxHasher>>>,
    file_source_roots: Arc<DashMap<FileId, FileSourceRootInput, BuildHasherDefault<FxHasher>>>,
    // parse_cache removed - Salsa handles caching via parse_query tracked function
}

impl Files {
    /// Create a new empty Files collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the Salsa input for file text.
    ///
    /// # Panics
    ///
    /// Panics if the file has not been set.
    pub fn file_text(&self, file_id: FileId) -> FileTextInput {
        self.file_texts.get(&file_id).map(|entry| *entry.value()).expect("file text not set")
    }

    /// Try to get the Salsa input for file text.
    ///
    /// Returns None if the file has not been loaded yet.
    pub fn try_file_text(&self, file_id: FileId) -> Option<FileTextInput> {
        self.file_texts.get(&file_id).map(|entry| *entry.value())
    }

    /// Set the text for a file.
    ///
    /// This creates or updates a Salsa input. Salsa automatically invalidates
    /// dependent queries when the text changes.
    pub fn set_file_text(&self, db: &mut dyn SourceDatabase, file_id: FileId, text: &str) {
        use dashmap::mapref::entry::Entry;
        use salsa::Setter;

        match self.file_texts.entry(file_id) {
            Entry::Occupied(mut occupied) => {
                // Update existing Salsa input
                occupied.get_mut().set_text(db).to(text.to_string());
            }
            Entry::Vacant(vacant) => {
                // Create new Salsa input
                let input = FileTextInput::new(db, text.to_string());
                vacant.insert(input);
            }
        }
        // No manual cache invalidation - Salsa handles it automatically!
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
        use dashmap::mapref::entry::Entry;
        use salsa::Setter;

        match self.file_texts.entry(file_id) {
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().set_text(db).with_durability(durability).to(text.to_string());
            }
            Entry::Vacant(vacant) => {
                let input = FileTextInput::builder(text.to_string()).durability(durability).new(db);
                vacant.insert(input);
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
        // Try to determine durability from source root
        if let Some(mapping) = self.file_source_roots.get(&file_id) {
            let source_root_id = mapping.source_root_id(db);
            if let Some(root_input) = self.source_roots.get(&source_root_id) {
                let root = root_input.root(db);
                let durability = root.durability();
                tracing::debug!(
                    ?file_id,
                    ?durability,
                    is_library = root.is_library,
                    "set_file_text_smart: determined durability from source root"
                );
                self.set_file_text_with_durability(db, file_id, text, durability);
                return;
            }
        }

        // Fallback to LOW durability if source root not set yet
        tracing::debug!(
            ?file_id,
            "set_file_text_smart: fallback to LOW durability (source root not set)"
        );
        self.set_file_text_with_durability(db, file_id, text, salsa::Durability::LOW);
    }

    /// Get the Salsa input for source root.
    pub fn source_root(&self, source_root_id: SourceRootId) -> SourceRootInput {
        self.source_roots
            .get(&source_root_id)
            .map(|entry| *entry.value())
            .expect("source root not set")
    }

    /// Set the source root.
    pub fn set_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        source_root_id: SourceRootId,
        source_root: SourceRoot,
    ) {
        use dashmap::mapref::entry::Entry;
        use salsa::Setter;

        match self.source_roots.entry(source_root_id) {
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().set_root(db).to(source_root);
            }
            Entry::Vacant(vacant) => {
                let input = SourceRootInput::new(db, source_root);
                vacant.insert(input);
            }
        }
    }

    /// Get the Salsa input for file source root mapping.
    pub fn file_source_root(&self, file_id: FileId) -> FileSourceRootInput {
        self.file_source_roots
            .get(&file_id)
            .map(|entry| *entry.value())
            .expect("file source root not set")
    }

    /// Set the file source root mapping.
    pub fn set_file_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        source_root_id: SourceRootId,
    ) {
        use dashmap::mapref::entry::Entry;
        use salsa::Setter;

        match self.file_source_roots.entry(file_id) {
            Entry::Occupied(mut occupied) => {
                occupied.get_mut().set_source_root_id(db).to(source_root_id);
            }
            Entry::Vacant(vacant) => {
                let input = FileSourceRootInput::new(db, source_root_id);
                vacant.insert(input);
            }
        }
    }

    // parse() method removed - replaced by parse_query tracked function below
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;

    // Test database implementation with full Salsa integration
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

        fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<RegionInfo>> {
            let input = self.file_text_input(file_id);
            module_level_regions_query(self, input)
        }
    }

    // Test query to verify FileIdInput (Salsa interned FileId) works with Salsa tracked functions.
    // This is critical for Phase 1 of the Salsa HIR integration plan.
    //
    // Note: FileId directly is NOT compatible with Salsa 0.25 (requires SalsaStructInDb trait).
    // We use FileIdInput wrapper (Salsa interned) as fallback solution.
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

        // Test basic usage
        let file_id = FileId(42);
        let file_id_input = FileIdInput::new(&db, file_id);

        // If this compiles and runs, FileIdInput works with Salsa
        let result = test_fileid_query(&db, file_id_input);
        assert_eq!(result, 42);

        // Test caching - should return the same value
        let result2 = test_fileid_query(&db, file_id_input);
        assert_eq!(result2, 42);

        // Test different FileId - should compute different value
        let file_id2 = FileId(100);
        let file_id_input2 = FileIdInput::new(&db, file_id2);
        let result3 = test_fileid_query(&db, file_id_input2);
        assert_eq!(result3, 100);

        // Test interning - same FileId should return same FileIdInput
        let file_id_input3 = FileIdInput::new(&db, file_id);
        assert_eq!(file_id_input, file_id_input3);
    }

    #[test]
    fn test_parse_query() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        // Parse the file
        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }

    #[test]
    fn test_incremental_reparse() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set initial content
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let parse1 = db.parse(file_id);
        assert!(!parse1.has_errors());

        // Set same content - should return cached result
        // Note: While wrapper structs may differ, the underlying Rowan GreenNode is shared
        let parse2 = db.parse(file_id);
        assert!(!parse2.has_errors());
        assert_eq!(parse1.syntax_node().text(), parse2.syntax_node().text());

        // Change content - should reparse
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

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        change.set_roots(vec![source_root]);

        change.apply(&mut db);

        // Should be able to parse now
        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }

    // SDBL tests migrated to ide-db (all_sdbl_in_file API)
}
