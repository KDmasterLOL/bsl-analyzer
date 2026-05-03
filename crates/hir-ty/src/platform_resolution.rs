//! Unified platform-method resolution for IDE consumers.
//!
//! Single use-case-layer entry point that turns
//! `(receiver_ty, method_name)` into a resolved [`ResolvedPlatformMethod`]
//! carrying both the semantic shape (return type / params / overloads)
//! and a stable [`PlatformMethodHandle`] that downstream IDE features
//! (hover, signature help, completion) can convert back to a
//! [`bsl_platform::PlatformMethod`] via Salsa-cached queries.
//!
//! ## Why this lives next to `method_lookup` rather than inside it
//!
//! [`crate::method_lookup::lookup_method`] serves type inference: it
//! takes no database, returns just `MethodInfo`, and is called from
//! `infer.rs` where the receiver type is already in hand. IDE features
//! (hover, goto, refs, signature help) need more — they need the
//! underlying `PlatformMethod` (specifically `id`) so they can fetch
//! method docs through `PlatformDataInner::get_method_docs(id)`.
//!
//! This module is the bridge between those two needs. It mirrors the
//! routing in `lookup_method` but additionally retains the
//! `PlatformMethod` it found, packages it into a [`PlatformMethodHandle`],
//! and feeds the rest of the IDE pipeline.
//!
//! ## Routing summary
//!
//! - [`Ty::ThisObject`] → coerced via [`crate::this_object::coerce_to_metadata_ref`]
//!   into the matching `*Object` `Ty::MetadataRef` before any other lookup.
//! - [`Ty::Union`] — recurse on each non-`Undefined`/`Null` member,
//!   bind handle / params / overloads to the **first** successful
//!   branch (cohesion rule mirrors `lookup_method`), union return types.
//! - [`Ty::ObjectManager { kind, name }`] — composite-prefix lookup
//!   under `kind.manager_type_prefix()` via [`bsl_platform::prefixed_method_query`].
//! - [`Ty::MetadataRef { kind, name }`]:
//!   * `MetadataKind::TabularSection { parent }` — scalar lookup under
//!     the flat `"Tabular section"` `type_name` with row-receiver
//!     rebinding via
//!     [`crate::method_lookup::build_tabular_section_method_info`].
//!   * `kind.platform_prefix() = Some(prefix)` — composite-prefix
//!     lookup under `prefix` via [`bsl_platform::prefixed_method_query`]
//!     plus generic-return rebinding via
//!     [`crate::platform_manager_lookup::build_resolution`].
//!   * `kind.scalar_platform_key() = Some(key)` — scalar lookup under
//!     `key` (synthetic `Filter` receiver on register record-set
//!     `<Набор>.Отбор.Сбросить()`).
//! - All other receivers — scalar lookup keyed by
//!   [`crate::method_lookup::platform_type_key`].

use std::hash::{Hash, Hasher};

use bsl_metadata::MdoType;
use bsl_platform::{
    platform_method_query, prefixed_method_query, MethodLookupInput, PlatformMethod,
    PrefixedMethodLookupInput,
};
use hir_def::ty::{MetadataKind, Ty};
use hir_def::Name;
use smol_str::SmolStr;

use crate::method_lookup::{
    build_tabular_section_method_info, platform_type_key, to_method_info, MethodInfo,
};
use crate::platform_manager_lookup::{build_resolution, metadata_kind_to_prefix_and_mdo};

/// Stable identity of a single platform method.
///
/// Equality and hashing key on `method_id` alone — `origin` is debug /
/// dispatch metadata and intentionally ignored in `Eq`/`Hash`. The same
/// method must never compare unequal just because two callers reached
/// it through different routes (e.g. once through a scalar key, once
/// through a composite prefix). Salsa downstream caches keyed on the
/// handle therefore stay stable across resolve sites.
#[derive(Debug, Clone)]
pub struct PlatformMethodHandle {
    /// The `PlatformMethod::id` from `bsl-platform`. Stable for the
    /// process lifetime — `PlatformData` is loaded once at startup.
    pub method_id: u32,
    /// How the method was reached. Drives the dispatch in
    /// [`PlatformMethodHandle::lookup`].
    pub origin: PlatformMethodOrigin,
}

impl PartialEq for PlatformMethodHandle {
    fn eq(&self, other: &Self) -> bool {
        self.method_id == other.method_id
    }
}

impl Eq for PlatformMethodHandle {}

impl Hash for PlatformMethodHandle {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.method_id.hash(state);
    }
}

/// Where a [`PlatformMethodHandle`] points in the platform-data tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlatformMethodOrigin {
    /// Direct `(type_name, method_name)` index hit. `type_name` is the
    /// English canonical name (`"Array"`, `"Filter"`, `"Tabular section"`,
    /// `"Запрос"`).
    Scalar { type_name: SmolStr },
    /// Composite-prefix walk hit. `prefix` is the prefix used in
    /// `PlatformMethod::type_name` (`"CatalogManager"`,
    /// `"InformationRegisterRecordSet"`); `mdo_type` and `mdo_name`
    /// identify the concrete MDO this resolution belongs to so the IDE
    /// can rebuild a Russian display label (`"Справочники.Номенклатура"`).
    Prefixed { prefix: SmolStr, mdo_type: MdoType, mdo_name: Name },
}

impl PlatformMethodHandle {
    /// Re-fetch the underlying [`PlatformMethod`] through Salsa-cached
    /// queries. Used by hover / signature help to obtain the method's
    /// `id` for `get_method_docs` and the parameter list for rendering.
    pub fn lookup(&self, db: &dyn salsa::Database) -> Option<PlatformMethod> {
        match &self.origin {
            PlatformMethodOrigin::Scalar { type_name } => {
                // Method-name lookup goes through the scalar bilingual index.
                // We don't carry the method-name string in the handle (id
                // is enough for hover), so we walk the cached method list
                // and pick by id. For the common case the caller already
                // has the method via the `resolve_method` return value.
                let method = bsl_platform::PlatformDataInner::instance()
                    .get_type_methods(type_name.as_str())
                    .into_iter()
                    .find(|m| m.id == self.method_id)
                    .cloned();
                if method.is_some() {
                    return method;
                }
                // Fallback for synthetic types not surfaced through `get_type_methods`
                // (`Tabular section`, `Filter`): walk all methods. Cheap
                // because both the `instance()` table and `id` comparison
                // are O(n) on a small sub-list — and we hit only when
                // resolved-by-handle outside the resolve-method flow.
                bsl_platform::PlatformDataInner::instance()
                    .all_methods()
                    .iter()
                    .find(|m| m.id == self.method_id)
                    .cloned()
            }
            PlatformMethodOrigin::Prefixed { prefix, .. } => {
                // The Salsa-cached prefixed query keys on `(prefix,
                // method_name)`; we have only the id here. Walk the
                // prefix-filtered method list and pick by id — list
                // size is bounded (≤ ~30 methods per prefix in HBK).
                let _ = db; // Salsa db isn't needed for the id walk;
                            // kept in the signature for symmetry with
                            // future Salsa-cached id→method query.
                bsl_platform::PlatformDataInner::instance()
                    .get_manager_methods(prefix.as_str())
                    .into_iter()
                    .find(|m| m.id == self.method_id)
                    .cloned()
            }
        }
    }
}

/// Result of a successful platform-method resolution.
///
/// Carries the stable handle (for IDE-side rebuilding of hover /
/// signature markdown) plus the semantic shape (`return_ty`, `params`,
/// `overloads`) inference would otherwise compute via
/// [`crate::method_lookup::lookup_method`].
///
/// Use [`Self::into_method_info`] to project away the handle when the
/// caller only needs inference data.
#[derive(Debug, Clone)]
pub struct ResolvedPlatformMethod {
    pub handle: PlatformMethodHandle,
    pub return_ty: Ty,
    pub params: Vec<Ty>,
    pub overloads: Vec<Vec<Ty>>,
}

impl ResolvedPlatformMethod {
    /// Drop the handle and keep just the inference-shaped projection.
    pub fn into_method_info(self) -> MethodInfo {
        MethodInfo { return_ty: self.return_ty, params: self.params, overloads: self.overloads }
    }
}

/// Resolve a method call on a typed receiver, returning both the
/// semantic shape and a stable [`PlatformMethodHandle`].
///
/// Returns `None` for receivers that carry no platform method table
/// (`Ty::Unknown`, primitives without instance methods, manager
/// collections, dimensions / resources / attributes / row receivers
/// without a platform surface), and for method names not present in
/// the resolved table.
pub fn resolve_method(
    db: &dyn salsa::Database,
    receiver_ty: &Ty,
    method_name: &Name,
) -> Option<ResolvedPlatformMethod> {
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    if let Ty::Union(members) = receiver_ty {
        let live: Vec<&Ty> =
            members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).collect();
        let mut returns: Vec<Ty> = Vec::with_capacity(live.len());
        let mut chosen: Option<ResolvedPlatformMethod> = None;
        // Cohesion rule mirrors `lookup_method`: bind the FIRST
        // successful branch's handle / params / overloads wholesale;
        // later branches contribute only their return types.
        for member in live {
            if let Some(res) = resolve_method(db, member, method_name) {
                returns.push(res.return_ty.clone());
                if chosen.is_none() {
                    chosen = Some(res);
                }
            }
        }
        return chosen.map(|mut r| {
            r.return_ty = Ty::union(returns);
            r
        });
    }

    match receiver_ty {
        Ty::ObjectManager { kind, name } => {
            let prefix = kind.manager_type_prefix()?;
            let method = lookup_prefixed(db, prefix, method_name.as_str())?;
            let resolution = build_resolution(&method, *kind, name);
            Some(ResolvedPlatformMethod {
                handle: PlatformMethodHandle {
                    method_id: method.id,
                    origin: PlatformMethodOrigin::Prefixed {
                        prefix: SmolStr::from(prefix),
                        mdo_type: *kind,
                        mdo_name: name.clone(),
                    },
                },
                return_ty: resolution.return_ty,
                params: resolution.signature.params.to_vec(),
                // Composite-prefix path doesn't surface multi-overload
                // signatures today; mirrors `lookup_method`.
                overloads: Vec::new(),
            })
        }
        Ty::MetadataRef { kind, name } => resolve_metadata_ref(db, *kind, name, method_name),
        _ => {
            let key = platform_type_key(receiver_ty)?;
            let method = lookup_scalar(db, key, method_name.as_str())?;
            let info = to_method_info(&method);
            Some(ResolvedPlatformMethod {
                handle: PlatformMethodHandle {
                    method_id: method.id,
                    origin: PlatformMethodOrigin::Scalar { type_name: SmolStr::from(key) },
                },
                return_ty: info.return_ty,
                params: info.params,
                overloads: info.overloads,
            })
        }
    }
}

fn resolve_metadata_ref(
    db: &dyn salsa::Database,
    kind: MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<ResolvedPlatformMethod> {
    // Tabular section — scalar dispatch under the flat
    // `"Tabular section"` type_name, with row-generic rebinding for
    // chained accessors (`ТЧ.Получить(0).<row attr>`).
    if let MetadataKind::TabularSection { parent } = kind {
        let method = lookup_scalar(db, "Tabular section", method_name.as_str())?;
        let info = build_tabular_section_method_info(&method, parent, mdo_name);
        return Some(ResolvedPlatformMethod {
            handle: PlatformMethodHandle {
                method_id: method.id,
                origin: PlatformMethodOrigin::Scalar {
                    type_name: SmolStr::from("Tabular section"),
                },
            },
            return_ty: info.return_ty,
            params: info.params,
            overloads: info.overloads,
        });
    }

    // Composite-prefix path — covers all 16 kinds with
    // `platform_prefix() = Some(prefix)`.
    if let Some((prefix, parent_mdo)) = metadata_kind_to_prefix_and_mdo(kind) {
        if let Some(method) = lookup_prefixed(db, prefix, method_name.as_str()) {
            let resolution = build_resolution(&method, parent_mdo, mdo_name);
            return Some(ResolvedPlatformMethod {
                handle: PlatformMethodHandle {
                    method_id: method.id,
                    origin: PlatformMethodOrigin::Prefixed {
                        prefix: SmolStr::from(prefix),
                        mdo_type: parent_mdo,
                        mdo_name: mdo_name.clone(),
                    },
                },
                return_ty: resolution.return_ty,
                params: resolution.signature.params.to_vec(),
                overloads: Vec::new(),
            });
        }
    }

    // Synthetic-scalar fallback (e.g. `RegisterFilter` → `"Filter"`).
    if let Some(scalar_key) = kind.scalar_platform_key() {
        if let Some(method) = lookup_scalar(db, scalar_key, method_name.as_str()) {
            let info = to_method_info(&method);
            return Some(ResolvedPlatformMethod {
                handle: PlatformMethodHandle {
                    method_id: method.id,
                    origin: PlatformMethodOrigin::Scalar { type_name: SmolStr::from(scalar_key) },
                },
                return_ty: info.return_ty,
                params: info.params,
                overloads: info.overloads,
            });
        }
    }

    None
}

fn lookup_scalar(
    db: &dyn salsa::Database,
    type_name: &str,
    method_name: &str,
) -> Option<PlatformMethod> {
    let input = MethodLookupInput::new(db, type_name.to_string(), method_name.to_string());
    platform_method_query(db, input)
}

fn lookup_prefixed(
    db: &dyn salsa::Database,
    prefix: &str,
    method_name: &str,
) -> Option<PlatformMethod> {
    let input = PrefixedMethodLookupInput::new(db, prefix.to_string(), method_name.to_string());
    prefixed_method_query(db, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal Salsa database for unit tests in this crate. Mirrors
    // `bsl_platform::db::tests::TestDatabase` and avoids pulling
    // `ide_db` into hir-ty's dev-deps just for these tests.
    #[salsa::db]
    #[derive(Clone, Default)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    impl salsa::Database for TestDatabase {}

    fn db() -> TestDatabase {
        TestDatabase::default()
    }

    #[test]
    fn metadata_ref_record_set_resolves_with_prefixed_handle() {
        let db = db();
        let ty = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("Курсы"),
        };
        let res = resolve_method(&db, &ty, &Name::new("Прочитать"));
        let Some(res) = res else {
            // Skip when running without platform data (CI without HBK).
            println!("Skipping: no platform data available");
            return;
        };
        assert_ne!(res.handle.method_id, 0);
        match &res.handle.origin {
            PlatformMethodOrigin::Prefixed { prefix, mdo_type, mdo_name } => {
                assert_eq!(prefix.as_str(), "InformationRegisterRecordSet");
                assert_eq!(*mdo_type, MdoType::InformationRegister);
                assert_eq!(mdo_name.as_str(), "Курсы");
            }
            PlatformMethodOrigin::Scalar { .. } => {
                panic!("composite prefix kind must use Prefixed origin");
            }
        }
        // `Прочитать()` is a procedure → return is `Ty::Undefined`.
        assert_eq!(res.return_ty, Ty::Undefined);
    }

    #[test]
    fn metadata_ref_register_filter_resolves_with_scalar_handle() {
        // RegisterFilter has scalar_platform_key = Some("Filter") and no
        // composite prefix; resolve_method must reach the scalar fallback.
        let db = db();
        let ty = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("Курсы"),
        };
        let res = resolve_method(&db, &ty, &Name::new("Сбросить"));
        let Some(res) = res else {
            println!("Skipping: no platform data available");
            return;
        };
        assert_ne!(res.handle.method_id, 0);
        match &res.handle.origin {
            PlatformMethodOrigin::Scalar { type_name } => {
                assert_eq!(type_name.as_str(), "Filter");
            }
            PlatformMethodOrigin::Prefixed { .. } => {
                panic!("RegisterFilter must reach Scalar fallback");
            }
        }
    }

    #[test]
    fn handle_eq_keys_on_method_id_only() {
        // Two handles with the same method_id but different origins
        // must compare equal — origin is purely informational.
        let h1 = PlatformMethodHandle {
            method_id: 42,
            origin: PlatformMethodOrigin::Scalar { type_name: SmolStr::from("Foo") },
        };
        let h2 = PlatformMethodHandle {
            method_id: 42,
            origin: PlatformMethodOrigin::Prefixed {
                prefix: SmolStr::from("BarPrefix"),
                mdo_type: MdoType::Catalog,
                mdo_name: Name::new("Bar"),
            },
        };
        assert_eq!(h1, h2);
        let mut h1_h = std::collections::hash_map::DefaultHasher::new();
        let mut h2_h = std::collections::hash_map::DefaultHasher::new();
        h1.hash(&mut h1_h);
        h2.hash(&mut h2_h);
        assert_eq!(h1_h.finish(), h2_h.finish());
    }

    #[test]
    fn unknown_method_returns_none_not_panic() {
        let db = db();
        let ty = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("Курсы"),
        };
        let res = resolve_method(&db, &ty, &Name::new("НесуществующийМетод"));
        assert!(res.is_none());
    }

    #[test]
    fn english_name_resolves_bilingually() {
        // Bilingual lookup: passing the English method name (`Read`)
        // must find the same method as the Russian (`Прочитать`).
        let db = db();
        let ty = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("Курсы"),
        };
        let ru = resolve_method(&db, &ty, &Name::new("Прочитать"));
        let en = resolve_method(&db, &ty, &Name::new("Read"));
        match (ru, en) {
            (Some(r), Some(e)) => assert_eq!(r.handle.method_id, e.handle.method_id),
            (None, None) => println!("Skipping: no platform data available"),
            _ => panic!("Russian and English lookups must both succeed or both fail"),
        }
    }

    #[test]
    fn into_method_info_drops_handle() {
        let res = ResolvedPlatformMethod {
            handle: PlatformMethodHandle {
                method_id: 1,
                origin: PlatformMethodOrigin::Scalar { type_name: SmolStr::from("X") },
            },
            return_ty: Ty::Number,
            params: vec![Ty::String],
            overloads: vec![vec![Ty::Boolean]],
        };
        let info = res.into_method_info();
        assert_eq!(info.return_ty, Ty::Number);
        assert_eq!(info.params, vec![Ty::String]);
        assert_eq!(info.overloads, vec![vec![Ty::Boolean]]);
    }
}
