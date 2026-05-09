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
    /// `(lower_name, kind) -> entry index`. Two indices share one map
    /// because `kind` already separates global-method names from
    /// constructor-type names (BSL allows the same lexeme to play both
    /// roles for some types, e.g. `Файл`).
    by_name_kind: FxHashMap<(String, EntryKind), usize>,
}

impl SecurityRegistry {
    fn build() -> Self {
        let mut by_name_kind: FxHashMap<(String, EntryKind), usize> = FxHashMap::default();

        for (idx, entry) in ENTRIES.iter().enumerate() {
            let ru_key = entry.ru.to_lowercase();
            insert_key(&mut by_name_kind, ru_key.clone(), entry.kind, idx);
            if !entry.en.is_empty() {
                let en_key = entry.en.to_lowercase();
                // Some BSL types (e.g. `xBase`) share the same lexeme on
                // both sides; skip the second insert rather than treat it
                // as a collision.
                if en_key != ru_key {
                    insert_key(&mut by_name_kind, en_key, entry.kind, idx);
                }
            }
        }

        Self { entries: ENTRIES, by_name_kind }
    }

    /// All entries in declaration order.
    pub fn entries(&self) -> &'static [SecurityEntry] {
        self.entries
    }

    /// Look up a global method by name (RU or EN, case-insensitive).
    pub fn lookup_global(&self, name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_with_kind(name, EntryKind::GlobalMethod)
    }

    /// Look up a `Новый <Type>(...)` / `New <Type>(...)` entry.
    pub fn lookup_constructor(&self, type_name: &str) -> Option<&'static SecurityEntry> {
        self.lookup_with_kind(type_name, EntryKind::Constructor)
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

    fn lookup_with_kind(&self, name: &str, kind: EntryKind) -> Option<&'static SecurityEntry> {
        if name.is_empty() {
            return None;
        }
        let key = name.to_lowercase();
        self.by_name_kind.get(&(key, kind)).map(|&idx| &self.entries[idx])
    }
}

fn insert_key(
    map: &mut FxHashMap<(String, EntryKind), usize>,
    key: String,
    kind: EntryKind,
    idx: usize,
) {
    debug_assert!(
        !key.is_empty(),
        "registry entry has empty key — `en` should be checked before insertion"
    );
    if let Some(prev) = map.insert((key, kind), idx) {
        // The audit-test in `tests/security_registry.rs` enforces this on
        // CI, but a runtime collision in `build()` would silently drop a
        // previous entry — surface it on the first call.
        debug_assert!(false, "duplicate security-registry key: prev={prev}, new={idx}",);
    }
}
