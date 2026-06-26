pub mod registry;
pub mod types;

pub use registry::ENTRIES;
pub use types::{
    CompatibilityBucket, DeprecationEntry, DisplayKind, ElementKind, LifecycleGroup, Lookup,
    OwnerType, Replacement,
};

use rustc_hash::FxHashMap;
use std::sync::OnceLock;
use stdx::case::CaseExt;

static REGISTRY: OnceLock<DeprecationRegistry> = OnceLock::new();

pub fn registry() -> &'static DeprecationRegistry {
    REGISTRY.get_or_init(|| DeprecationRegistry::from_entries(ENTRIES))
}

pub struct DeprecationRegistry {
    entries: &'static [DeprecationEntry],
    groups: Vec<LifecycleGroup>,
    lookup: FxHashMap<LookupKey, usize>,
}

impl DeprecationRegistry {
    pub fn from_entries(entries: &'static [DeprecationEntry]) -> Self {
        let mut groups = Vec::new();
        let mut lookup = FxHashMap::default();

        for (idx, entry) in entries.iter().enumerate() {
            if !groups.contains(&entry.group) {
                groups.push(entry.group);
            }
            insert_entry(&mut lookup, EntryIndex { entry, idx });
        }

        Self { entries, groups, lookup }
    }

    pub fn entries(&self) -> &'static [DeprecationEntry] {
        self.entries
    }

    pub fn groups(&self) -> &[LifecycleGroup] {
        &self.groups
    }

    pub fn entries_by_group(&self, group: LifecycleGroup) -> Vec<&'static DeprecationEntry> {
        self.entries.iter().filter(|entry| entry.group == group).collect()
    }

    pub fn lookup(&self, query: Lookup<'_>) -> Option<&'static DeprecationEntry> {
        let owner = query.owner.map(CaseExt::fold_lower);
        let name = query.name.fold_lower();
        self.lookup_lc(Lookup::new(query.element_kind, owner.as_deref(), &name))
    }

    pub fn lookup_lc(&self, query: Lookup<'_>) -> Option<&'static DeprecationEntry> {
        debug_assert!(
            query.name == query.name.fold_lower(),
            "lookup_lc requires pre-lowercased input, got: {}",
            query.name,
        );
        if let Some(owner) = query.owner {
            debug_assert!(
                owner == owner.fold_lower(),
                "lookup_lc requires pre-lowercased owner input, got: {owner}",
            );
        }
        if query.name.is_empty() || query.owner.is_some_and(str::is_empty) {
            return None;
        }
        let key = LookupKey::from_lookup(query);
        self.lookup.get(&key).map(|&idx| &self.entries[idx])
    }
}

#[derive(Clone, Copy)]
struct EntryIndex<'a> {
    entry: &'a DeprecationEntry,
    idx: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LookupKey {
    element_kind: ElementKind,
    owner: Option<String>,
    name: String,
}

impl LookupKey {
    fn new(element_kind: ElementKind, owner: Option<String>, name: String) -> Self {
        Self { element_kind, owner, name }
    }

    fn from_lookup(query: Lookup<'_>) -> Self {
        Self::new(query.element_kind, query.owner.map(str::to_owned), query.name.to_owned())
    }
}

fn insert_entry(lookup: &mut FxHashMap<LookupKey, usize>, indexed: EntryIndex<'_>) {
    debug_assert!(
        !indexed.entry.ru.is_empty(),
        "deprecation registry entry has empty `ru`: {:?}",
        indexed.entry,
    );
    let ru_key = indexed.entry.ru.fold_lower();
    insert_name_alias(lookup, indexed, &ru_key);
    if !indexed.entry.en.is_empty() {
        let en_key = indexed.entry.en.fold_lower();
        if en_key != ru_key {
            insert_name_alias(lookup, indexed, &en_key);
        }
    }
}

fn insert_name_alias(
    lookup: &mut FxHashMap<LookupKey, usize>,
    indexed: EntryIndex<'_>,
    name_key: &str,
) {
    match indexed.entry.owner {
        Some(owner) => {
            debug_assert!(
                !owner.ru.is_empty(),
                "deprecation registry entry has empty owner `ru`: {:?}",
                indexed.entry,
            );
            let ru_owner = owner.ru.fold_lower();
            insert_key(
                lookup,
                LookupKey::new(
                    indexed.entry.element_kind,
                    Some(ru_owner.clone()),
                    name_key.to_owned(),
                ),
                indexed.idx,
            );
            if !owner.en.is_empty() {
                let en_owner = owner.en.fold_lower();
                if en_owner != ru_owner {
                    insert_key(
                        lookup,
                        LookupKey::new(
                            indexed.entry.element_kind,
                            Some(en_owner),
                            name_key.to_owned(),
                        ),
                        indexed.idx,
                    );
                }
            }
        }
        None => insert_key(
            lookup,
            LookupKey::new(indexed.entry.element_kind, None, name_key.to_owned()),
            indexed.idx,
        ),
    }
}

fn insert_key(lookup: &mut FxHashMap<LookupKey, usize>, key: LookupKey, idx: usize) {
    debug_assert!(
        !key.name.is_empty(),
        "deprecation registry entry has empty key — `en` should be checked before insertion"
    );
    if let Some(prev) = lookup.insert(key, idx) {
        debug_assert!(false, "duplicate deprecation-registry key: prev={prev}, new={idx}");
    }
}
