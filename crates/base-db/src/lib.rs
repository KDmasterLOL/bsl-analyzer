//! Base database for bsl-analyzer.
//!
//! This crate provides the foundation for incremental computation using Salsa.
//! It defines the core database traits and types for managing source files and parsing.
//!
//! Note: This is an initial implementation with simplified caching.
//! Full Salsa 0.25.2 integration (with tracked functions and proper ingredient registration)
//! will be completed in a later iteration.

use std::hash::BuildHasherDefault;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHasher;
use vfs::{FileId, VfsPath};

mod change;
mod input;

pub use change::FileChange;
pub use input::{FileSourceRootInput, FileTextInput, SourceRoot, SourceRootId, SourceRootInput};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInfo {
    pub name: String,
    pub range: syntax::TextRange,
}

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

/// Salsa tracked query for parsing.
///
/// This query automatically depends on the FileTextInput and is cached with LRU (512 entries).
/// When file text changes, Salsa automatically invalidates this query.
#[salsa::tracked(lru = 512)]
pub fn parse_query(
    db: &dyn salsa::Database,
    input: FileTextInput,
) -> syntax::Parse<syntax::SyntaxNode> {
    let _span = tracing::info_span!("parse").entered();

    let text = input.text(db);
    parser::parse(&text)
}

/// Map from method source ranges to parent API region names.
///
/// This query identifies all methods (procedures/functions) that are inside
/// API regions (ПрограммныйИнтерфейс, Public, СлужебныйПрограммныйИнтерфейс, Internal)
/// and returns a mapping from their TextRange to the root region name.
///
/// # Performance
/// - LRU cache: 256 files (region analysis is inexpensive)
/// - Depends on: parse_query (automatic invalidation via Salsa)
/// - Recomputed only when file content changes
/// - Shared by region-based diagnostics (no redundant tree walking)
///
/// # Root Region Lookup
/// For nested regions, always returns the TOP-LEVEL (root) region name,
/// not the immediate parent. This matches bsl-language-server behavior.
///
/// Example:
/// ```bsl
/// #Region Public
///     #Region Internal
///         Procedure MyProc()  // Maps to "Public" (root), not "Internal"
///         EndProcedure
///     #EndRegion
/// #EndRegion
/// ```
#[salsa::tracked(lru = 256)]
pub fn method_regions(
    db: &dyn salsa::Database,
    input: FileTextInput,
) -> Arc<std::collections::HashMap<syntax::TextRange, String>> {
    let _span = tracing::info_span!("method_regions").entered();

    let parse = parse_query(db, input);
    let root = parse.syntax_node();

    let mut map = std::collections::HashMap::new();
    collect_methods_in_regions(&root, &mut Vec::new(), &mut map);

    tracing::debug!(count = map.len(), "Collected methods in API regions");

    Arc::new(map)
}

/// Recursively collect methods in API regions.
///
/// Tracks region stack to identify root (first) region for nested structures.
fn collect_methods_in_regions(
    node: &syntax::SyntaxNode,
    region_stack: &mut Vec<String>,
    map: &mut std::collections::HashMap<syntax::TextRange, String>,
) {
    use syntax::{
        ast::{self, AstNode},
        SyntaxKind,
    };

    for child in node.children() {
        match child.kind() {
            SyntaxKind::PRE_REGION_DIR => {
                if let Some(region) = ast::PreRegionDir::cast(child.clone()) {
                    if region.is_start() {
                        if let Some(name) = region.name() {
                            region_stack.push(name);
                            collect_methods_in_regions(region.syntax(), region_stack, map);
                            region_stack.pop();
                        }
                    }
                }
            }
            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                if let Some(root_region) = region_stack.first() {
                    if is_api_region(root_region) {
                        let range = child.text_range();
                        map.insert(range, root_region.clone());
                    }
                }
            }
            _ => {
                if child.kind() != SyntaxKind::PRE_REGION_DIR {
                    collect_methods_in_regions(&child, region_stack, map);
                }
            }
        }
    }
}

/// Check if a region name is an API region.
///
/// API regions (case-insensitive):
/// - ПрограммныйИнтерфейс / Public
/// - СлужебныйПрограммныйИнтерфейс / Internal
fn is_api_region(name: &str) -> bool {
    const API_REGIONS: &[&str] =
        &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];
    API_REGIONS.contains(&name.to_lowercase().as_str())
}

/// Extract all module-level region names and their text ranges.
///
/// This query collects information about all top-level regions in a BSL file.
/// Only module-level regions are collected (nested regions inside methods are excluded).
///
/// # Performance
/// - LRU cache: 256 files (region collection is inexpensive)
/// - Depends on: parse_query (automatic invalidation via Salsa)
/// - Recomputed only when file content changes
/// - Shared by ALL region-based diagnostics (NonStandardRegion, DuplicateRegion, EmptyRegion, etc.)
///
/// # Returns
/// Vector of RegionInfo containing region names and their text ranges (first line only).
/// Ranges point to the #Область/#Region directive line.
#[salsa::tracked(lru = 256)]
pub fn module_level_regions(
    db: &dyn salsa::Database,
    input: FileTextInput,
) -> Arc<Vec<RegionInfo>> {
    let _span = tracing::info_span!("module_level_regions").entered();

    let parse = parse_query(db, input);
    let root = parse.syntax_node();

    let regions = collect_module_level_regions(&root);

    tracing::debug!(count = regions.len(), "Collected module-level regions");

    Arc::new(regions)
}

/// Collect module-level regions (not nested inside methods).
///
/// Walks root.children() (not descendants) to get only top-level regions.
/// For each region start directive, extracts name and range of first line.
fn collect_module_level_regions(root: &syntax::SyntaxNode) -> Vec<RegionInfo> {
    use syntax::{
        ast::{self, AstNode},
        SyntaxKind, TextRange, TextSize,
    };

    let mut regions = Vec::new();

    for child in root.children() {
        if child.kind() == SyntaxKind::PRE_REGION_DIR {
            if let Some(region) = ast::PreRegionDir::cast(child.clone()) {
                if region.is_start() {
                    if let Some(name) = region.name() {
                        let text = child.text().to_string();
                        let first_line = text.lines().next().unwrap_or(&text);
                        let first_line_len = first_line.len();

                        let start = child.text_range().start();
                        let end = start + TextSize::from(first_line_len as u32);
                        let range = TextRange::new(start, end);

                        regions.push(RegionInfo { name, range });
                    }
                }
            }
        }
    }

    regions
}

/// Resolve a VfsPath to FileId within a SourceRoot.
///
/// Searches the FileSet of a SourceRoot for a given VfsPath.
/// Used by diagnostics to resolve metadata URIs to FileIds.
///
/// # Performance
/// - O(1) FileSet lookup (HashMap)
/// - Cached by Salsa (LRU 256)
/// - Expected: < 1ms after first call
///
/// # Parameters
/// - `source_root_input`: The SourceRoot to search in
/// - `vfs_path_str`: String representation of VfsPath (PathBuf is not Hash)
///
/// # Returns
/// - `Some(FileId)` if path exists in FileSet
/// - `None` if path not found
#[salsa::tracked(lru = 256)]
pub fn resolve_vfs_path_query(
    db: &dyn salsa::Database,
    source_root_input: SourceRootInput,
    vfs_path_str: String,
) -> Option<FileId> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let vfs_path = VfsPath::new(PathBuf::from(vfs_path_str));
    file_set.file_for_path(&vfs_path).copied()
}

/// Higher-level database trait with derived queries.
///
/// This trait extends SourceDatabase with parse queries.
/// Implementations should delegate to parse_query Salsa tracked function.
#[salsa::db]
pub trait RootQueryDb: SourceDatabase {
    /// Parse a file into a syntax tree.
    ///
    /// This query is cached for incremental computation via Salsa.
    /// Implementations should call parse_query(self, file_text_input(file_id)).
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode>;

    /// Map from method source ranges to parent API region names.
    ///
    /// Returns a HashMap mapping TextRange (of method definitions) to their
    /// parent API region name (ПрограммныйИнтерфейс, Public, СлужебныйПрограммныйИнтерфейс, Internal).
    ///
    /// Only methods inside API regions are included in the map.
    /// For nested regions, the root (top-level) region name is returned.
    ///
    /// ## Performance Benefits
    ///
    /// - **Salsa-cached**: Recomputed only when file changes
    /// - **Shared across diagnostics**: Multiple region-based diagnostics reuse the same cache
    /// - **Auto-invalidation**: Salsa invalidates when file changes
    ///
    /// ## Usage in Diagnostics
    ///
    /// ```ignore
    /// pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    ///     let method_regions = ctx.db.method_regions(ctx.file_id);
    ///     let item_tree = ctx.db.item_tree(ctx.file_id);
    ///
    ///     for (_, proc) in item_tree.procedures() {
    ///         if let Some(region_name) = method_regions.get(&proc.source_range) {
    ///             // proc is in an API region named region_name
    ///         }
    ///     }
    /// }
    /// ```
    fn method_regions(
        &self,
        file_id: FileId,
    ) -> Arc<std::collections::HashMap<syntax::TextRange, String>>;

    /// Extract all module-level regions from a BSL file.
    ///
    /// Returns cached list of region names with their source ranges.
    /// Used by region-based diagnostics (NonStandardRegion, DuplicateRegion, EmptyRegion, CodeOutOfRegion).
    ///
    /// ## Performance Benefits
    ///
    /// - **Salsa-cached**: AST traversal happens once per file change (LRU 256)
    /// - **Shared across diagnostics**: 4+ region diagnostics reuse same cache
    /// - **Auto-invalidation**: Salsa invalidates when file changes
    ///
    /// ## Usage in Diagnostics
    ///
    /// ```ignore
    /// pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    ///     let regions = ctx.db.module_level_regions(ctx.file_id);
    ///
    ///     for region in regions.iter() {
    ///         // region.name contains region name
    ///         // region.range contains first line of #Область directive
    ///     }
    /// }
    /// ```
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
                self.set_file_text_with_durability(db, file_id, text, durability);
                return;
            }
        }

        // Fallback to LOW durability if source root not set yet
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
            method_regions(self, input)
        }

        fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<RegionInfo>> {
            let input = self.file_text_input(file_id);
            module_level_regions(self, input)
        }
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
