//! Platform API capability facts.
//!
//! This registry owns context-free platform membership facts: names of platform
//! APIs and optional replacements. Diagnostic crates decide whether and how to
//! report those facts.
//!
//! Deferred boundary:
//! - Security-sensitive API categories stay in `bsl_platform::security` because
//!   they carry vulnerability severity and parameter roles. Owning layer:
//!   `bsl-platform::security`, consumed by lowering and IDE diagnostics.
//! - Call-graph, dataflow, metadata, and source-doc diagnostics stay in their
//!   existing analysis layers because they require project-specific state, not
//!   pure platform membership. Owning layers: `hir-def::call_graph`, `dataflow`,
//!   `bsl-metadata`, `hir-def::docs`, and `ide-diagnostics` projection handlers.

pub mod registry;
pub mod types;

pub use registry::ENTRIES;
pub use types::{CapabilityEntry, Category, EntryKind, Replacement};

use rustc_hash::FxHashMap;
use std::sync::OnceLock;
use stdx::case::CaseExt;

static REGISTRY: OnceLock<CapabilityRegistry> = OnceLock::new();

pub fn registry() -> &'static CapabilityRegistry {
    REGISTRY.get_or_init(|| CapabilityRegistry::build_from(ENTRIES))
}

pub struct CapabilityRegistry {
    entries: &'static [CapabilityEntry],
    categories: Vec<Category>,
    lookup: LookupMaps,
}

impl CapabilityRegistry {
    fn build_from(entries: &'static [CapabilityEntry]) -> Self {
        let mut categories = Vec::new();
        let mut lookup = LookupMaps::default();

        for (idx, entry) in entries.iter().enumerate() {
            if !categories.contains(&entry.category) {
                categories.push(entry.category);
            }
            insert_entry(&mut lookup, entry, idx);
        }

        Self { entries, categories, lookup }
    }

    pub fn entries(&self) -> &'static [CapabilityEntry] {
        self.entries
    }

    pub fn categories(&self) -> &[Category] {
        &self.categories
    }

    pub fn entries_by_category(&self, category: Category) -> Vec<&'static CapabilityEntry> {
        self.entries.iter().filter(|entry| entry.category == category).collect()
    }

    pub fn lookup(
        &self,
        category: Category,
        kind: EntryKind,
        name: &str,
    ) -> Option<&'static CapabilityEntry> {
        self.lookup_lc(category, kind, &name.fold_lower())
    }

    pub fn lookup_lc(
        &self,
        category: Category,
        kind: EntryKind,
        lc_name: &str,
    ) -> Option<&'static CapabilityEntry> {
        debug_assert!(
            lc_name == lc_name.fold_lower(),
            "lookup_lc requires pre-lowercased input, got: {lc_name}"
        );
        if lc_name.is_empty() {
            return None;
        }
        self.lookup
            .bucket(kind, category)
            .and_then(|bucket| bucket.get(lc_name))
            .map(|&idx| &self.entries[idx])
    }
}

#[derive(Default)]
struct LookupMaps {
    globals: FxHashMap<Category, FxHashMap<String, usize>>,
    methods: FxHashMap<Category, FxHashMap<String, usize>>,
    constructors: FxHashMap<Category, FxHashMap<String, usize>>,
    types: FxHashMap<Category, FxHashMap<String, usize>>,
    global_properties: FxHashMap<Category, FxHashMap<String, usize>>,
}

impl LookupMaps {
    fn bucket(&self, kind: EntryKind, category: Category) -> Option<&FxHashMap<String, usize>> {
        match kind {
            EntryKind::GlobalMethod => self.globals.get(&category),
            EntryKind::Method => self.methods.get(&category),
            EntryKind::Constructor => self.constructors.get(&category),
            EntryKind::Type => self.types.get(&category),
            EntryKind::GlobalProperty => self.global_properties.get(&category),
        }
    }

    fn bucket_mut(&mut self, kind: EntryKind, category: Category) -> &mut FxHashMap<String, usize> {
        match kind {
            EntryKind::GlobalMethod => self.globals.entry(category).or_default(),
            EntryKind::Method => self.methods.entry(category).or_default(),
            EntryKind::Constructor => self.constructors.entry(category).or_default(),
            EntryKind::Type => self.types.entry(category).or_default(),
            EntryKind::GlobalProperty => self.global_properties.entry(category).or_default(),
        }
    }
}

fn insert_entry(lookup: &mut LookupMaps, entry: &CapabilityEntry, idx: usize) {
    debug_assert!(!entry.ru.is_empty(), "capability registry entry has empty `ru`: {entry:?}");
    let bucket = lookup.bucket_mut(entry.kind, entry.category);
    let ru_key = entry.ru.fold_lower();
    insert_key(bucket, ru_key.clone(), idx);
    if !entry.en.is_empty() {
        let en_key = entry.en.fold_lower();
        if en_key != ru_key {
            insert_key(bucket, en_key, idx);
        }
    }
}

fn insert_key(bucket: &mut FxHashMap<String, usize>, key: String, idx: usize) {
    debug_assert!(
        !key.is_empty(),
        "capability registry entry has empty key — `en` should be checked before insertion"
    );
    if let Some(prev) = bucket.insert(key, idx) {
        debug_assert!(false, "duplicate capability-registry key: prev={prev}, new={idx}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUPLICATE_ENTRIES: &[CapabilityEntry] = &[
        CapabilityEntry {
            ru: "Вопрос",
            en: "DoQueryBox",
            kind: EntryKind::GlobalMethod,
            category: Category::ModalWindow,
            replacement: None,
        },
        CapabilityEntry {
            ru: "Дубликат",
            en: "doquerybox",
            kind: EntryKind::GlobalMethod,
            category: Category::ModalWindow,
            replacement: None,
        },
    ];

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "duplicate capability-registry key")]
    fn build_debug_asserts_on_duplicate_lookup_key() {
        let _ = CapabilityRegistry::build_from(DUPLICATE_ENTRIES);
    }
}
