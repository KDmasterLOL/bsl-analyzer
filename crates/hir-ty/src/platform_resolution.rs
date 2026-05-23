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
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
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
    /// Re-fetch the underlying [`PlatformMethod`] from the platform-data
    /// singleton by `method_id` walk.
    ///
    /// Used by hover / signature help to obtain the method's `id` for
    /// `get_method_docs` and the parameter list for rendering. The walk
    /// is O(n) over a prefix-filtered or type-filtered sub-list (≤ ~30
    /// methods per prefix in HBK), and `PlatformDataInner` is loaded
    /// once per process (`OnceCell` singleton), so this is effectively
    /// constant-time per call site.
    ///
    /// `db` is currently unused — kept in the signature so a future
    /// `platform_method_by_handle` Salsa query can swap in without an
    /// API change. Marked `_ = db` rather than dropped so the symmetry
    /// with `platform_method_query`/`prefixed_method_query` stays
    /// visible at the call site.
    pub fn lookup(&self, db: &dyn salsa::Database) -> Option<PlatformMethod> {
        let _ = db;
        match &self.origin {
            PlatformMethodOrigin::Scalar { type_name } => {
                // Try the type-keyed sub-list first — fast path for
                // surfaced types (`Array`, `Запрос`, `ValueTable`, …).
                let method = bsl_platform::PlatformDataInner::instance()
                    .get_type_methods(type_name.as_str())
                    .into_iter()
                    .find(|m| m.id == self.method_id)
                    .cloned();
                if method.is_some() {
                    return method;
                }
                // Synthetic types like `"Tabular section"` and `"Filter"`
                // are not surfaced through `get_type_methods` (no entry
                // in `types_by_name`), so fall back to the full method
                // table. Hit only on the synthetic-receiver paths;
                // bounded by total platform method count.
                bsl_platform::PlatformDataInner::instance()
                    .all_methods()
                    .iter()
                    .find(|m| m.id == self.method_id)
                    .cloned()
            }
            PlatformMethodOrigin::Prefixed { prefix, .. } => {
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

    /// Kernel-native projection of [`Self::return_ty`].
    ///
    /// §4.C accessor — bridges via §4.A `ty_to_typeid`.
    #[allow(dead_code, reason = "Phase 3 §4.C — consumers migrate in 4.D-4.E")]
    pub fn return_typeid(&self, db: &dyn TypeKernelDb) -> TypeId {
        crate::ty_bridge::ty_to_typeid(db, &self.return_ty)
    }

    /// Kernel-native projection of [`Self::params`].
    #[allow(dead_code, reason = "Phase 3 §4.C — consumers migrate in 4.D-4.E")]
    pub fn params_typeid(&self, db: &dyn TypeKernelDb) -> Vec<TypeId> {
        self.params.iter().map(|t| crate::ty_bridge::ty_to_typeid(db, t)).collect()
    }

    /// Kernel-native projection of [`Self::overloads`].
    #[allow(dead_code, reason = "Phase 3 §4.C — consumers migrate in 4.D-4.E")]
    pub fn overloads_typeid(&self, db: &dyn TypeKernelDb) -> Vec<Vec<TypeId>> {
        self.overloads
            .iter()
            .map(|row| row.iter().map(|t| crate::ty_bridge::ty_to_typeid(db, t)).collect())
            .collect()
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
                overloads: resolution.overloads,
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
                overloads: resolution.overloads,
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
    use crate::ty_bridge::typeid_to_ty;
    use bsl_types::testing::InMemoryDb;

    /// §4.C drift-detector: `ResolvedPlatformMethod` kernel-native accessors
    /// mirror the Ty fields.
    #[test]
    fn resolved_platform_method_typeid_round_trips_via_ty() {
        let db = InMemoryDb::new();
        let res = ResolvedPlatformMethod {
            handle: PlatformMethodHandle {
                method_id: 0,
                origin: PlatformMethodOrigin::Scalar { type_name: SmolStr::from("X") },
            },
            return_ty: Ty::Number,
            params: vec![Ty::String, Ty::Boolean],
            overloads: vec![vec![Ty::Date]],
        };
        assert_eq!(typeid_to_ty(&db, res.return_typeid(&db)), res.return_ty);
        let pids_via_ty: Vec<Ty> =
            res.params_typeid(&db).iter().map(|id| typeid_to_ty(&db, *id)).collect();
        assert_eq!(pids_via_ty, res.params);
        let oids = res.overloads_typeid(&db);
        let oids_via_ty: Vec<Vec<Ty>> =
            oids.iter().map(|row| row.iter().map(|id| typeid_to_ty(&db, *id)).collect()).collect();
        assert_eq!(oids_via_ty, res.overloads);
    }

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
    fn composite_multi_overload_method_populates_overloads() {
        // Composite-prefix methods can declare multiple `Вариант
        // синтаксиса:` sections in HBK (e.g.
        // `InformationRegisterManager.Get`,
        // `AccountingRegisterRecordSet.Move`,
        // `BusinessProcessManager.FindByNumber`). Pre-fix, the
        // composite path produced `overloads: Vec::new()` because
        // `build_resolution` doesn't compute per-variant params.
        // After the `lower_overloads` lift this gap is closed; pin
        // it so a regression of either the helper or `build_resolution`
        // surfaces here.
        let db = db();
        let ty =
            Ty::ObjectManager { kind: MdoType::InformationRegister, name: Name::new("Курсы") };
        let res = resolve_method(&db, &ty, &Name::new("Получить"));
        let Some(res) = res else {
            println!("Skipping: no platform data available");
            return;
        };
        assert!(
            !res.overloads.is_empty(),
            "InformationRegisterManager.Получить must surface multi-overload variants; \
             got params={:?}, overloads={:?}",
            res.params,
            res.overloads,
        );
    }

    #[test]
    fn object_manager_resolves_with_prefixed_handle() {
        // `Справочники.<Имя>.СоздатьЭлемент()` — the receiver is
        // `Ty::ObjectManager { kind: Catalog, name: "Номенклатура" }`,
        // resolved through `manager_type_prefix() = Some("CatalogManager")`.
        // Ensures the ObjectManager arm of `resolve_method` produces the
        // same Prefixed origin shape as the MetadataRef composite-prefix
        // arm.
        let db = db();
        let ty = Ty::ObjectManager {
            kind: MdoType::Catalog, name: Name::new("Номенклатура")
        };
        let res = resolve_method(&db, &ty, &Name::new("СоздатьЭлемент"));
        let Some(res) = res else {
            println!("Skipping: no platform data available");
            return;
        };
        match &res.handle.origin {
            PlatformMethodOrigin::Prefixed { prefix, mdo_type, mdo_name } => {
                assert_eq!(prefix.as_str(), "CatalogManager");
                assert_eq!(*mdo_type, MdoType::Catalog);
                assert_eq!(mdo_name.as_str(), "Номенклатура");
            }
            PlatformMethodOrigin::Scalar { .. } => {
                panic!("ObjectManager must use Prefixed origin");
            }
        }
        // `СоздатьЭлемент()` returns a `СправочникОбъект.Номенклатура` —
        // generic-return rebinding (build_resolution) must produce a
        // concrete `Ty::MetadataRef { CatalogObject, "Номенклатура" }`.
        assert_eq!(
            res.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogObject, name: Name::new("Номенклатура")
            }
        );
    }

    #[test]
    fn union_first_branch_owns_handle_returns_unioned() {
        // Cohesion rule: for `Ty::Union([A, B])`, the FIRST successful
        // branch's handle / params / overloads are bound wholesale;
        // later branches contribute only their return types. Pin this
        // because regressing the rule would silently change the params
        // signature for fluent-style chains like
        // `Запрос.Выполнить().Выгрузить()` whose receiver is
        // `Ty::Union([QueryResult, Undefined])`.
        let db = db();
        let happy = Ty::PlatformObject(Name::new("РезультатЗапроса"));
        let union = Ty::union(vec![happy.clone(), Ty::Undefined]);
        let direct = resolve_method(&db, &happy, &Name::new("Выгрузить"));
        let through_union = resolve_method(&db, &union, &Name::new("Выгрузить"));
        match (direct, through_union) {
            (Some(direct_res), Some(union_res)) => {
                // First-branch handle: same id and origin.
                assert_eq!(direct_res.handle.method_id, union_res.handle.method_id);
                // Union return: must be unioned (Undefined didn't resolve,
                // so the union has just one branch — equality with direct).
                assert_eq!(union_res.return_ty, direct_res.return_ty);
            }
            (None, None) => {
                println!("Skipping: no platform data available");
            }
            _ => panic!("Direct and union routes must agree on success/failure"),
        }
    }

    #[test]
    fn union_strips_undefined_and_null_sentinels() {
        // `Ty::Union([Undefined, Null])` has no live branches —
        // resolution must be `None` cleanly without panic, regardless
        // of the method name. Mirrors `lookup_method`'s sentinel
        // stripping (no instance methods on Undefined / Null).
        let db = db();
        let dead = Ty::union(vec![Ty::Undefined, Ty::Null]);
        let res = resolve_method(&db, &dead, &Name::new("ЛюбоеИмя"));
        assert!(res.is_none());
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
