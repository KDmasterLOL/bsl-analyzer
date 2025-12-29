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
use vfs::FileId;

mod change;
mod input;

pub use change::FileChange;
pub use input::{SourceRoot, SourceRootId};

// ========== Database Traits ==========

/// The main database trait for source file operations.
///
/// This trait provides access to file contents and source root information.
pub trait SourceDatabase {
    /// Get the text content of a file.
    fn file_text(&self, file_id: FileId) -> Arc<str>;

    /// Get the source root ID for a file.
    fn file_source_root(&self, file_id: FileId) -> SourceRootId;

    /// Get the source root data for a source root ID.
    fn source_root(&self, id: SourceRootId) -> Arc<SourceRoot>;

    // Setters for input queries

    /// Set the text content of a file.
    fn set_file_text(&mut self, file_id: FileId, text: &str);

    /// Set the source root for a file.
    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId);

    /// Set the source root data.
    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: Arc<SourceRoot>);
}

/// Higher-level database trait with derived queries.
///
/// This trait extends SourceDatabase with queries for parsing and analysis.
pub trait RootQueryDb: SourceDatabase {
    /// Parse a file into a syntax tree.
    ///
    /// This query is cached for incremental computation.
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode>;
}

// ========== Files Helper ==========

/// Helper structure for managing file state with concurrent access.
///
/// Uses DashMap for lock-free concurrent access to file data and parse caching.
#[derive(Debug, Default, Clone)]
pub struct Files {
    file_texts: Arc<DashMap<FileId, Arc<str>, BuildHasherDefault<FxHasher>>>,
    source_roots: Arc<DashMap<SourceRootId, Arc<SourceRoot>, BuildHasherDefault<FxHasher>>>,
    file_source_roots: Arc<DashMap<FileId, SourceRootId, BuildHasherDefault<FxHasher>>>,
    parse_cache:
        Arc<DashMap<FileId, Arc<syntax::Parse<syntax::SyntaxNode>>, BuildHasherDefault<FxHasher>>>,
}

impl Files {
    /// Create a new empty Files collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the text for a file.
    ///
    /// # Panics
    ///
    /// Panics if the file has not been set.
    pub fn file_text(&self, file_id: FileId) -> Arc<str> {
        self.file_texts.get(&file_id).map(|entry| entry.value().clone()).expect("file text not set")
    }

    /// Set the text for a file.
    pub fn set_file_text(&self, file_id: FileId, text: &str) {
        use dashmap::mapref::entry::Entry;

        let text_arc: Arc<str> = Arc::from(text);

        match self.file_texts.entry(file_id) {
            Entry::Occupied(mut entry) => {
                entry.insert(text_arc);
                // Invalidate parse cache
                self.parse_cache.remove(&file_id);
            }
            Entry::Vacant(entry) => {
                entry.insert(text_arc);
            }
        }
    }

    /// Get the source root for a source root ID.
    pub fn source_root(&self, source_root_id: SourceRootId) -> Arc<SourceRoot> {
        self.source_roots
            .get(&source_root_id)
            .map(|entry| entry.value().clone())
            .expect("source root not set")
    }

    /// Set the source root.
    pub fn set_source_root(&self, source_root_id: SourceRootId, source_root: Arc<SourceRoot>) {
        self.source_roots.insert(source_root_id, source_root);
    }

    /// Get the file source root mapping for a file.
    pub fn file_source_root(&self, file_id: FileId) -> SourceRootId {
        self.file_source_roots
            .get(&file_id)
            .map(|entry| *entry.value())
            .expect("file source root not set")
    }

    /// Set the file source root mapping.
    pub fn set_file_source_root(&self, file_id: FileId, source_root_id: SourceRootId) {
        self.file_source_roots.insert(file_id, source_root_id);
    }

    /// Parse a file with caching.
    pub fn parse(
        &self,
        db: &dyn SourceDatabase,
        file_id: FileId,
    ) -> syntax::Parse<syntax::SyntaxNode> {
        // Check cache first
        if let Some(cached) = self.parse_cache.get(&file_id) {
            return (**cached.value()).clone();
        }

        let _span = tracing::info_span!("parse", ?file_id).entered();

        // Get file text from database
        let text = db.file_text(file_id);

        // Parse the file
        let parse_result = parser::parse(&text);

        // Cache the result wrapped in Arc
        self.parse_cache.insert(file_id, Arc::new(parse_result.clone()));

        parse_result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;

    // Test database implementation
    // Note: This is a simplified implementation for testing.
    // Real Salsa integration will be completed in a later iteration.
    #[derive(Default, Clone)]
    struct TestDatabase {
        files: Files,
    }

    impl SourceDatabase for TestDatabase {
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
        }

        fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
            self.files.set_file_source_root(file_id, source_root_id);
        }

        fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: Arc<SourceRoot>) {
            self.files.set_source_root(source_root_id, source_root);
        }
    }

    impl RootQueryDb for TestDatabase {
        fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
            self.files.parse(self, file_id)
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
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
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
        db.set_source_root(SourceRootId(0), Arc::new(source_root));
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
}
