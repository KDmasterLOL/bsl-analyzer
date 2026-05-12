//! Name-usage index — fast cross-file lookup of files mentioning a name.
//!
//! ## Problem
//!
//! `WorkspaceIndex` indexes only DEFINITIONS, so `find_references` for an
//! export symbol had to walk every BSL file in the source root (25k+ on the
//! ERP workspace), parse each one and hold its HIR in Salsa's cache.
//!
//! ## Solution
//!
//! Two-tier Salsa query:
//!
//! - [`file_name_usage_query`] — per-file lowercase-normalized set of every
//!   name-token in the file. Re-runs only when `file_text(file_id)` changes,
//!   so a single edit invalidates one entry.
//! - [`source_root_name_usage_query`] — per-source-root aggregator that
//!   merges file-level results into a reverse map `Name → Vec<FileId>`.
//!   Cached with `lru = 4`; re-runs after any per-file invalidation, but all
//!   untouched per-file results stay cached.
//!
//! ## Predicate parity
//!
//! Both `find_references_in_file` (in `ide::references`) and this index gate
//! candidate tokens with [`SyntaxKind::is_name_token`] and case-fold via
//! `to_lowercase()`. Diverging predicates here would cause candidates to
//! silently disappear, so they MUST stay in lock-step.

use crate::{DefDatabase, Name};
use base_db::{FileIdInput, SourceRootInput};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use vfs::FileId;

/// Lowercase-normalized name-tokens mentioned in a single file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileNameUsage {
    names: FxHashSet<Name>,
}

impl FileNameUsage {
    /// Iterate over the lowercase-normalized names mentioned in the file.
    pub fn iter(&self) -> impl Iterator<Item = &Name> {
        self.names.iter()
    }

    /// Number of distinct lowercase-normalized names.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// `true` if no name-tokens were found (empty file, only keywords, etc.).
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// `true` if `lowercase_name` was observed somewhere in the file.
    /// `lowercase_name` MUST be pre-normalized via [`normalize_name`].
    pub fn contains(&self, lowercase_name: &Name) -> bool {
        self.names.contains(lowercase_name)
    }
}

/// Reverse map `lowercase Name → sorted, deduplicated Vec<FileId>` covering
/// a single source root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRootNameUsage {
    by_name: FxHashMap<Name, Vec<FileId>>,
}

impl SourceRootNameUsage {
    /// Files mentioning `lowercase_name`. `lowercase_name` MUST be pre-normalized.
    pub fn files_with(&self, lowercase_name: &Name) -> &[FileId] {
        self.by_name.get(lowercase_name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of distinct names recorded.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// `true` if no name-tokens were observed across the source root.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// Case-fold `name` into the form used as a key in this index.
///
/// Mirrors the case-fold side of [`Name::eq_ignore_case`] so callers and the
/// index agree on bucket identity. Callers MUST normalize before lookup.
pub fn normalize_name(name: &Name) -> Name {
    Name::new(&name.as_str().to_lowercase())
}

/// Salsa-tracked per-file index of name-token occurrences.
///
/// Re-runs only when `file_text(file_id)` changes, so an edit in one module
/// does NOT invalidate the cache for any other module.
#[salsa::tracked]
pub fn file_name_usage_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<FileNameUsage> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::debug_span!("file_name_usage_query", ?file_id).entered();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let mut names: FxHashSet<Name> = FxHashSet::default();
    for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if !token.kind().is_name_token() {
            continue;
        }
        names.insert(Name::new(&token.text().to_lowercase()));
    }

    Arc::new(FileNameUsage { names })
}

/// Salsa-tracked aggregator: reverse `Name → Vec<FileId>` across an entire
/// source root.
///
/// LRU = 4 (one per source root, typically 1-2 in practice). Calling this
/// after a single-file edit re-runs the aggregator but reuses every other
/// file's cached [`file_name_usage_query`] result.
#[salsa::tracked(lru = 4)]
pub fn source_root_name_usage_query<'db>(
    db: &'db dyn DefDatabase,
    source_root_input: SourceRootInput,
) -> Arc<SourceRootNameUsage> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();

    // Only BSL source files are valid input for `parse`. SourceRoot also holds
    // XML/MD/TXT entries, mirroring the filter used by `workspace_index_query`.
    let files: Vec<FileId> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .collect();

    let _span =
        tracing::info_span!("source_root_name_usage_query", file_count = files.len()).entered();

    let mut by_name: FxHashMap<Name, Vec<FileId>> = FxHashMap::default();
    for file_id in files {
        // Cold-build over a 25k-file source root takes seconds; without this
        // probe a freshly arrived edit (or a superseding LSP request) would
        // have to wait for the entire walk to complete before Salsa noticed.
        db.unwind_if_revision_cancelled();
        let input = FileIdInput::new(db, file_id);
        let usage = file_name_usage_query(db, input);
        for name in usage.names.iter() {
            by_name.entry(name.clone()).or_default().push(file_id);
        }
    }

    for entry in by_name.values_mut() {
        entry.sort_unstable();
        entry.dedup();
    }

    tracing::info!(names = by_name.len(), "SourceRootNameUsage built");

    Arc::new(SourceRootNameUsage { by_name })
}

#[cfg(test)]
mod tests {
    //! Pure-data tests for the index's value types and `normalize_name`.
    //!
    //! Salsa-query tests live in `hir::tests` — exercising
    //! `file_name_usage_query` / `source_root_name_usage_query` requires the
    //! concrete `RootDatabaseImpl` from `ide-db`, which `hir-def` cannot import
    //! at trait-bound granularity without colliding with itself in the
    //! dev-dependency diamond.

    use super::*;

    #[test]
    fn normalize_name_lowercases_cyrillic() {
        assert_eq!(normalize_name(&Name::new("Тест")), normalize_name(&Name::new("ТЕСТ")));
        assert_eq!(normalize_name(&Name::new("Тест")), normalize_name(&Name::new("тест")));
    }

    #[test]
    fn normalize_name_lowercases_latin() {
        assert_eq!(normalize_name(&Name::new("Test")), normalize_name(&Name::new("TEST")));
        assert_eq!(normalize_name(&Name::new("Test")), normalize_name(&Name::new("test")));
    }

    #[test]
    fn file_name_usage_membership_uses_normalized_keys() {
        // Hand-built usage to verify the lookup contract without running the
        // Salsa query.
        let mut names = FxHashSet::default();
        names.insert(normalize_name(&Name::new("Тест")));
        let usage = FileNameUsage { names };

        assert!(usage.contains(&normalize_name(&Name::new("ТЕСТ"))));
        assert!(usage.contains(&normalize_name(&Name::new("тест"))));
        assert!(!usage.contains(&normalize_name(&Name::new("Другое"))));
    }

    #[test]
    fn source_root_name_usage_returns_empty_slice_on_miss() {
        let aggregator = SourceRootNameUsage::default();
        assert!(aggregator.files_with(&normalize_name(&Name::new("чтонибудь"))).is_empty());
    }
}
