use crate::{DefDatabase, Name};
use base_db::{FileIdInput, SourceRootInput};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use stdx::case::CaseExt;
use vfs::FileId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileNameUsage {
    names: FxHashSet<Name>,
}

impl FileNameUsage {
    pub fn iter(&self) -> impl Iterator<Item = &Name> {
        self.names.iter()
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn contains(&self, lowercase_name: &Name) -> bool {
        self.names.contains(lowercase_name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceRootNameUsage {
    by_name: FxHashMap<Name, Vec<FileId>>,
}

impl SourceRootNameUsage {
    pub fn files_with(&self, lowercase_name: &Name) -> &[FileId] {
        self.by_name.get(lowercase_name).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

pub fn normalize_name(name: &Name) -> Name {
    Name::new(&name.as_str().fold_lower())
}

/// Approximate live heap bytes for Salsa's `memory_usage` report: the `names`
/// hashbrown set plus each name's non-inlined `SmolStr` payload.
fn file_name_usage_heap(v: &Arc<FileNameUsage>) -> usize {
    use crate::heap_estimate::{map_table_bytes, name_bytes};

    let mut bytes = map_table_bytes::<Name, ()>(v.names.len());
    for name in &v.names {
        bytes += name_bytes(name);
    }
    bytes
}

#[salsa::tracked(lru = 4096, heap_size = file_name_usage_heap)]
pub fn file_name_usage_query<'db>(
    db: &'db dyn DefDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<FileNameUsage> {
    let file_id = file_id_input.file_id(db);
    let _span = tracing::debug_span!("file_name_usage_query", ?file_id).entered();

    let parse = db.parse_ref(file_id);
    let root = parse.syntax_node();

    let mut names: FxHashSet<Name> = FxHashSet::default();
    for token in root.descendants_with_tokens().filter_map(|e| e.into_token()) {
        if !token.kind().is_name_token() {
            continue;
        }
        names.insert(Name::new(&token.text().fold_lower()));
    }

    Arc::new(FileNameUsage { names })
}

#[salsa::tracked(lru = 4)]
pub fn source_root_name_usage_query(
    db: &dyn DefDatabase,
    source_root_input: SourceRootInput,
) -> Arc<SourceRootNameUsage> {
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();

    let files: Vec<FileId> = source_root
        .iter()
        .filter(|&file_id| crate::workspace::is_bsl_source(file_set, file_id))
        .collect();

    let _span =
        tracing::info_span!("source_root_name_usage_query", file_count = files.len()).entered();

    let mut by_name: FxHashMap<Name, Vec<FileId>> = FxHashMap::default();
    for file_id in files {
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
