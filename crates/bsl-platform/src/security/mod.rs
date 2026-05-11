//! Curated security-API registry: types, catalogue, runtime indices.
//!
//! See `types.rs` for the data model and `registry.rs` for the entry list.
//! This module exposes:
//!
//! - [`SecurityRegistry`] — the runtime singleton that wraps
//!   [`registry::ENTRIES`] with bilingual case-insensitive lookup tables.
//! - [`registry()`] — `OnceLock`-backed accessor returning a process-wide
//!   instance, built lazily on first call.
//!
//! # Layer rules
//!
//! - Lookups take `&str` (not `Name`). `bsl-platform` is a leaf crate; if
//!   it took `&Name` it would have to depend on `hir-def`, which already
//!   depends on `bsl-platform` — a cycle.
//! - The catalogue is hand-maintained, not derived from HBK. The
//!   `bsl-platform/data/` directory is reserved for HBK-derived JSON
//!   (`platform_data.json`); keeping curated and derived sources apart
//!   preserves provenance.

pub mod registry;
pub mod types;

pub use registry::ENTRIES;
pub use types::{Category, EntryKind, Lifetime, ParamRole, Role, SecurityEntry, Severity};

use rustc_hash::FxHashMap;
use std::sync::OnceLock;

static REGISTRY: OnceLock<SecurityRegistry> = OnceLock::new();

/// Process-wide accessor. The first call builds the indices; subsequent
/// calls return the same `&'static SecurityRegistry`.
pub fn registry() -> &'static SecurityRegistry {
    REGISTRY.get_or_init(SecurityRegistry::build)
}

/// Bilingual case-insensitive index over [`ENTRIES`].
///
/// Keys are derived by `to_lowercase()` of the `ru` and `en` fields. Empty
/// `en` strings are skipped to keep the EN side disambiguated.
pub struct SecurityRegistry {
    entries: &'static [SecurityEntry],
    /// `lower_name -> entry index` for global-method names.
    globals: FxHashMap<String, usize>,
    /// `lower_name -> entry index` for constructor-type names.
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
                // Some BSL types (e.g. `xBase`) share the same lexeme on
                // both sides; skip the second insert rather than treat it
                // as a collision.
                if en_key != ru_key {
                    insert_key(map, en_key, idx);
                }
            }
        }

        Self { entries: ENTRIES, globals, constructors }
    }

    /// All entries in declaration order.
    pub fn entries(&self) -> &'static [SecurityEntry] {
        self.entries
    }

    /// Look up a global method by name (RU or EN, case-insensitive).
    ///
    /// Convenience slow path for one-off lookups; hot callers that already
    /// have a lowercased key should use [`Self::lookup_global_lc`].
    pub fn lookup_global(&self, name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_global_lc(&name.to_lowercase())
    }

    /// Look up a global method by an already-lowercase name.
    ///
    /// Hot-path variant: callers must pass a lowercased RU or EN key so the
    /// registry can probe the `String` map with `&str` and avoid allocating.
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

    /// Look up a `Новый <Type>(...)` / `New <Type>(...)` entry.
    ///
    /// Convenience slow path for one-off lookups; hot callers that already
    /// have a lowercased key should use [`Self::lookup_constructor_lc`].
    pub fn lookup_constructor(&self, type_name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_constructor_lc(&type_name.to_lowercase())
    }

    /// Look up a `Новый <Type>(...)` / `New <Type>(...)` entry by an
    /// already-lowercase type name.
    ///
    /// Hot-path variant: callers must pass a lowercased RU or EN key so the
    /// registry can probe the `String` map with `&str` and avoid allocating.
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

    /// Filter the catalogue by category. Returns a freshly allocated `Vec`
    /// because the const layout is heterogeneous; if this becomes hot we
    /// can pre-bucket at `build()` time.
    pub fn entries_by_category(&self, category: Category) -> Vec<&'static SecurityEntry> {
        self.entries.iter().filter(|e| e.category == category).collect()
    }

    /// Resolve the begin/end pair for a lifetime-bearing API. Returns
    /// `None` until paired APIs are added — see `Lifetime` doc-comment in
    /// `types.rs`.
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
        // The audit-test in `tests/security_registry.rs` enforces this on
        // CI, but a runtime collision in `build()` would silently drop a
        // previous entry — surface it on the first call.
        debug_assert!(false, "duplicate security-registry key: prev={prev}, new={idx}",);
    }
}
