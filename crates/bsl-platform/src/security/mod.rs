pub mod registry;
pub mod types;

pub use registry::ENTRIES;
pub use types::{Category, EntryKind, Lifetime, ParamRole, Role, SecurityEntry, Severity};

use rustc_hash::FxHashMap;
use std::sync::OnceLock;

static REGISTRY: OnceLock<SecurityRegistry> = OnceLock::new();

pub fn registry() -> &'static SecurityRegistry {
    REGISTRY.get_or_init(SecurityRegistry::build)
}

pub struct SecurityRegistry {
    entries: &'static [SecurityEntry],
    globals: FxHashMap<String, usize>,
    constructors: FxHashMap<String, usize>,
}

impl SecurityRegistry {
    fn build() -> Self {
        let mut globals: FxHashMap<String, usize> = FxHashMap::default();
        let mut constructors: FxHashMap<String, usize> = FxHashMap::default();

        for (idx, entry) in ENTRIES.iter().enumerate() {
            let map = match entry.kind {
                EntryKind::GlobalMethod => &mut globals,
                EntryKind::Constructor => &mut constructors,
            };
            let ru_key = entry.ru.to_lowercase();
            insert_key(map, ru_key.clone(), idx);
            if !entry.en.is_empty() {
                let en_key = entry.en.to_lowercase();
                if en_key != ru_key {
                    insert_key(map, en_key, idx);
                }
            }
        }

        Self { entries: ENTRIES, globals, constructors }
    }

    pub fn entries(&self) -> &'static [SecurityEntry] {
        self.entries
    }

    pub fn lookup_global(&self, name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_global_lc(&name.to_lowercase())
    }

    pub fn lookup_global_lc(&self, lc_name: &str) -> Option<&'static SecurityEntry> {
        debug_assert!(
            lc_name == lc_name.to_lowercase(),
            "lookup_global_lc requires pre-lowercased input, got: {lc_name}"
        );
        if lc_name.is_empty() {
            return None;
        }
        self.globals.get(lc_name).map(|&idx| &self.entries[idx])
    }

    pub fn lookup_constructor(&self, type_name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_constructor_lc(&type_name.to_lowercase())
    }

    pub fn lookup_constructor_lc(&self, lc_name: &str) -> Option<&'static SecurityEntry> {
        debug_assert!(
            lc_name == lc_name.to_lowercase(),
            "lookup_constructor_lc requires pre-lowercased input, got: {lc_name}"
        );
        if lc_name.is_empty() {
            return None;
        }
        self.constructors.get(lc_name).map(|&idx| &self.entries[idx])
    }

    pub fn entries_by_category(&self, category: Category) -> Vec<&'static SecurityEntry> {
        self.entries.iter().filter(|e| e.category == category).collect()
    }

    pub fn lifetime_pair(
        &self,
        _entry: &SecurityEntry,
    ) -> Option<(&'static SecurityEntry, &'static SecurityEntry)> {
        None
    }
}

fn insert_key(map: &mut FxHashMap<String, usize>, key: String, idx: usize) {
    debug_assert!(
        !key.is_empty(),
        "registry entry has empty key — `en` should be checked before insertion"
    );
    if let Some(prev) = map.insert(key, idx) {
        debug_assert!(false, "duplicate security-registry key: prev={prev}, new={idx}",);
    }
}
