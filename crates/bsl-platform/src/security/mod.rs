pub mod registry;
pub mod types;

pub use registry::ENTRIES;
pub use types::{Category, EntryKind, Lifetime, ParamRole, Role, SecurityEntry, Severity};

use rustc_hash::FxHashMap;
use std::sync::OnceLock;
use stdx::case::CaseExt;

static REGISTRY: OnceLock<SecurityRegistry> = OnceLock::new();

pub fn registry() -> &'static SecurityRegistry {
    REGISTRY.get_or_init(SecurityRegistry::build)
}

pub struct SecurityRegistry {
    entries: &'static [SecurityEntry],
    globals: FxHashMap<String, usize>,
    constructors: FxHashMap<String, usize>,
    module_methods: FxHashMap<String, usize>,
}

impl SecurityRegistry {
    fn build() -> Self {
        let mut globals: FxHashMap<String, usize> = FxHashMap::default();
        let mut constructors: FxHashMap<String, usize> = FxHashMap::default();
        let mut module_methods: FxHashMap<String, usize> = FxHashMap::default();

        for (idx, entry) in ENTRIES.iter().enumerate() {
            let map = match entry.kind {
                EntryKind::GlobalMethod => &mut globals,
                EntryKind::Constructor => &mut constructors,
                EntryKind::ModuleMethod { .. } => &mut module_methods,
            };
            let ru_key = entry.ru.fold_lower();
            insert_key(map, ru_key.clone(), idx);
            if !entry.en.is_empty() {
                let en_key = entry.en.fold_lower();
                if en_key != ru_key {
                    insert_key(map, en_key, idx);
                }
            }
            for (ru, legacy_en) in crate::LEGACY_GLOBAL_FUNCTION_EN_ALIASES {
                if ru.fold_lower() == ru_key {
                    insert_key(map, legacy_en.fold_lower(), idx);
                }
            }
        }

        Self { entries: ENTRIES, globals, constructors, module_methods }
    }

    pub fn entries(&self) -> &'static [SecurityEntry] {
        self.entries
    }

    pub fn lookup_global(&self, name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_global_lc(&name.fold_lower())
    }

    pub fn lookup_global_lc(&self, lc_name: &str) -> Option<&'static SecurityEntry> {
        debug_assert!(
            lc_name == lc_name.fold_lower(),
            "lookup_global_lc requires pre-lowercased input, got: {lc_name}"
        );
        if lc_name.is_empty() {
            return None;
        }
        self.globals.get(lc_name).map(|&idx| &self.entries[idx])
    }

    /// Resolve `<receiver>.<name>` against the module methods. Returns `None`
    /// unless the receiver is one of the entry's declared owners — any other
    /// receiver means a different method that merely shares the spelling.
    pub fn lookup_module_method(
        &self,
        receiver: &str,
        name: &str,
    ) -> Option<&'static SecurityEntry> {
        self.lookup_module_method_lc(&receiver.fold_lower(), &name.fold_lower())
    }

    pub fn lookup_module_method_lc(
        &self,
        lc_receiver: &str,
        lc_name: &str,
    ) -> Option<&'static SecurityEntry> {
        debug_assert!(
            lc_receiver == lc_receiver.fold_lower() && lc_name == lc_name.fold_lower(),
            "lookup_module_method_lc requires pre-lowercased input, got: {lc_receiver}.{lc_name}"
        );
        if lc_receiver.is_empty() || lc_name.is_empty() {
            return None;
        }
        let entry = self.module_methods.get(lc_name).map(|&idx| &self.entries[idx])?;
        let EntryKind::ModuleMethod { owners } = entry.kind else {
            return None;
        };
        owners.iter().any(|owner| owner.fold_lower() == lc_receiver).then_some(entry)
    }

    pub fn lookup_constructor(&self, type_name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_constructor_lc(&type_name.fold_lower())
    }

    pub fn lookup_constructor_lc(&self, lc_name: &str) -> Option<&'static SecurityEntry> {
        debug_assert!(
            lc_name == lc_name.fold_lower(),
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
