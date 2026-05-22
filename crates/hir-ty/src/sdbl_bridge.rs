//! Bridge from SDBL HIR (`sdbl_hir`) types to BSL types (`hir_def::Ty`).
//!
//! Pure functions — no `db` access, no Salsa queries. The bridge is the
//! single source of truth for "how does an SDBL field type project onto
//! the BSL type system?". Callers (Phase 1.3 inference hooks) consume
//! `query_to_projection` / `package_to_projections` to attach
//! [`SdblProjection`] payloads to the new
//! [`Ty::Query`] / [`Ty::QueryResult`] / [`Ty::QueryResultSelection`] /
//! [`Ty::QueryBatchResult`] variants seeded in Phase 0.
//!
//! ## Mapping table
//!
//! | `SdblType` variant            | → `Ty`                                    |
//! |-------------------------------|-------------------------------------------|
//! | `Boolean`                     | `Ty::Boolean`                             |
//! | `String { length }`           | `Ty::String` (length preserved in shadow) |
//! | `Number { precision, scale }` | `Ty::Number` (dims preserved in shadow)   |
//! | `Date` / `DateTime`           | `Ty::Date`                                |
//! | `Ref(MdoRef)`                 | `Ty::MetadataRef { *Ref, name }`          |
//! | `AnyRef`                      | `Ty::Unknown`                             |
//! | `AnyObjectRef { mdo_type }`   | `Ty::AnyMetadataRef { mdo_type }`         |
//! | `Uuid`                        | `Ty::PlatformObject("УникальныйИдентификатор")` |
//! | `ValueStorage`                | `Ty::PlatformObject("ХранилищеЗначения")` |
//! | `DefinedType { underlying: Some(t) }` | `sdbl_type_to_ty(t)`              |
//! | `DefinedType { underlying: None }` | `Ty::Unknown`                        |
//! | `ValueTable`                  | `Ty::ValueTable`                          |
//! | `Null`                        | `Ty::Null`                                |
//! | `Aggregate(inner)`            | `sdbl_type_to_ty(inner)`                  |
//! | `Composite { types }`         | `Ty::union(types.iter().map(...))`        |
//! | `TabularSectionRef { … }`     | `Ty::MetadataRef { TabularSection, name }`|
//! | `Unknown` / `Error`           | `Ty::Unknown`                             |
//!
//! ## What the bridge does NOT do
//!
//! - **Asterisk expansion**. `FieldHir { is_asterisk: true }` is skipped
//!   in projection extraction. Phase 1.2 expands `*` / `Т.*` against the
//!   originating `ResolvedTable::fields` and re-runs the bridge over the
//!   expanded set.
//!
//!   **Policy until then:** a `SELECT *` (or `SELECT Т.*`) query whose
//!   field list contains *only* asterisks yields `None` from
//!   [`query_to_projection`]. Inference hook callers (Phase 1.3) attach
//!   `None` to the synthesised [`Ty::QueryResult`] / [`Ty::QueryResultSelection`]
//!   in that case, which makes the receiver fall back to the platform
//!   `РезультатЗапроса` / `ВыборкаИзРезультатаЗапроса` surface — same
//!   behaviour the legacy `Ty::PlatformObject("…")` shape produces
//!   today. No `UnresolvedField` regression on `Выборка.<Имя>` because
//!   field lookup degrades to the platform table.
//!
//!   `SELECT *, NamedField` — mixed shapes — yields `Some(projection)`
//!   carrying only `NamedField`, dropping the asterisk silently. That
//!   matches the "best-effort projection" contract: the projection is
//!   never wrong, only sometimes incomplete. Phase 1.2 fills the gap.
//! - **Variable refinement / data-flow**. The bridge takes a lowered
//!   `SdblPackage` as input; it does not trace `.Текст = "..."`
//!   assignments. That lives in `hir-ty::query_text_dataflow` (Phase 2).
//! - **CASE-WHEN heterogeneity**. SDBL HIR may model `ВЫБОР КОГДА X ТОГДА
//!   A ИНАЧЕ Б` as `SdblType::Composite`; the bridge's `Composite` arm
//!   then unions the bridged arm types. If the SDBL lowerer emits a
//!   single `SdblType` instead, the bridge inherits that decision.

use std::sync::Arc;

use bsl_metadata::MdoType;
use hir_def::ty::{MetadataKind, SdblProjection, SdblTypeShadow, Ty};
use hir_def::Name;

/// Map a single SDBL field type to its BSL counterpart.
///
/// Pure function — see the module-level mapping table for the contract.
pub fn sdbl_type_to_ty(t: &sdbl_hir::SdblType) -> Ty {
    use sdbl_hir::SdblType as S;
    match t {
        S::Boolean => Ty::Boolean,
        // Length / precision / scale are display-only enrichments —
        // they drop out of `Ty` (which is structural) and live on
        // [`SdblTypeShadow.display`] for hover.
        S::String { .. } => Ty::String,
        S::Number { .. } => Ty::Number,
        S::Date | S::DateTime => Ty::Date,
        S::Ref(mdo) => mdo_ref_to_metadata_ref(mdo),
        // `AnyRef` is the SDBL "any MDO reference" cell — no useful
        // refinement available without context, so the bridge collapses
        // to `Unknown`. Field lookup falls through to the platform
        // fallback for any caller that needs `.Ссылка` / `.UUID` etc.
        S::AnyRef => Ty::Unknown,
        S::AnyObjectRef { mdo_type } => Ty::AnyMetadataRef { mdo_type: *mdo_type },
        // Platform value wrappers — the bilingual platform-data index
        // resolves their Russian names case-insensitively.
        S::Uuid => Ty::PlatformObject(Name::new("УникальныйИдентификатор")),
        S::ValueStorage => Ty::PlatformObject(Name::new("ХранилищеЗначения")),
        S::DefinedType { underlying_type, .. } => underlying_type
            .as_deref()
            .map(sdbl_type_to_ty)
            // Unresolved `ОпределяемыйТип` — hir-def-side resolver
            // expansion is a Phase-1+ extension; for now the bridge
            // surfaces `Unknown` so callers don't false-positive a
            // type-error on an opaque user-defined type.
            .unwrap_or(Ty::Unknown),
        S::ValueTable => Ty::ValueTable,
        S::Null => Ty::Null,
        // SDBL `SUM(Number) → Aggregate(Number)` — strip the wrapper
        // and bridge the inner type. The aggregate marker is irrelevant
        // to BSL inference.
        S::Aggregate(inner) => sdbl_type_to_ty(inner),
        S::Composite { types } => Ty::union(types.iter().map(sdbl_type_to_ty).collect()),
        S::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => Ty::MetadataRef {
            kind: MetadataKind::TabularSection { parent: *parent_mdo_type },
            // Name convention is "<Parent>.<Section>", mirroring the
            // `hir-def::MetadataKind::TabularSection` doc.
            name: Name::new(&format!("{}.{}", parent_mdo_name, ts_name)),
        },
        S::Unknown | S::Error => Ty::Unknown,
    }
}

/// Map a single SDBL `MdoRef` (reference cell) onto a `Ty::MetadataRef`
/// whose kind is the matching `*Ref` variant for the MDO family.
///
/// When the MDO family has no `*Ref` companion in [`MetadataKind`]
/// (`ChartOfCharacteristicTypes`, `ChartOfCalculationTypes`, etc.),
/// falls back to [`Ty::AnyMetadataRef { mdo_type }`]. That preserves
/// the MDO kind (so receivers like `<ref>.Метаданные()` and family-wide
/// completion still work) at the cost of losing the specific name —
/// a strict improvement over `Ty::Unknown` which would silently swallow
/// the SDBL provenance.
///
/// Extending [`MetadataKind`] with the missing `*Ref` variants
/// (`ChartOfCharacteristicTypesRef`, `ChartOfCalculationTypesRef`, …)
/// is the proper fix and a deferred follow-up.
fn mdo_ref_to_metadata_ref(mdo: &sdbl_hir::MdoRef) -> Ty {
    match ref_kind_for(mdo.mdo_type) {
        Some(kind) => Ty::MetadataRef { kind, name: Name::new(&mdo.name) },
        None => Ty::AnyMetadataRef { mdo_type: mdo.mdo_type },
    }
}

/// Pick the matching `*Ref` `MetadataKind` for an MDO flavour.
///
/// Companion to [`MetadataKind::object_kind_for`] which picks `*Object`;
/// this one picks `*Ref`. Kept private to the bridge module — public
/// callers can route through [`sdbl_type_to_ty`] / [`mdo_ref_to_metadata_ref`]
/// without needing to know the mapping themselves.
fn ref_kind_for(mdo: MdoType) -> Option<MetadataKind> {
    Some(match mdo {
        MdoType::Catalog => MetadataKind::CatalogRef,
        MdoType::Document => MetadataKind::DocumentRef,
        MdoType::Enum => MetadataKind::EnumRef,
        MdoType::Task => MetadataKind::TaskRef,
        MdoType::BusinessProcess => MetadataKind::BusinessProcessRef,
        MdoType::ExchangePlan => MetadataKind::ExchangePlanRef,
        MdoType::ChartOfAccounts => MetadataKind::ChartOfAccountsRef,
        MdoType::InformationRegister => MetadataKind::InformationRegisterRef,
        MdoType::AccumulationRegister => MetadataKind::AccumulationRegisterRef,
        MdoType::AccountingRegister => MetadataKind::AccountingRegisterRef,
        MdoType::CalculationRegister => MetadataKind::CalculationRegisterRef,
        // MDOs without a `*Ref` companion in `MetadataKind` (common
        // modules, dimensions, constants, …) — SDBL does not put these
        // in a reference cell, so the bridge returns `None` and the
        // caller falls back to `Ty::Unknown`.
        _ => return None,
    })
}

/// Extract a single SELECT query's projection.
///
/// Walks the query's `SELECT` field list, skipping asterisk fields and
/// parse-error fields. Returns `Some(projection)` when at least one
/// bridge-able named field remains, `None` otherwise — callers attach
/// a `None` projection to the receiver's [`Ty::QueryResult`] /
/// [`Ty::QueryResultSelection`] in that case.
///
/// Field-name priority follows
/// [`sdbl_hir::FieldHir::alias_or_name`]: alias > column name > raw
/// name. Fields with no recoverable name are dropped.
// TODO(Phase 1.2): expand `is_asterisk` fields against the originating
// `ResolvedTable::fields` so `SELECT * FROM Catalog.X` produces a
// projection over X's attributes instead of silently dropping the
// asterisk.
pub fn query_to_projection(q: &sdbl_hir::SdblQuery) -> Option<Arc<SdblProjection>> {
    let mut named_fields: Vec<(Name, Ty)> = Vec::with_capacity(q.hir.select.fields.len());
    let mut shadows: Vec<SdblTypeShadow> = Vec::with_capacity(q.hir.select.fields.len());

    for field in &q.hir.select.fields {
        if field.is_asterisk || field.has_parse_error {
            continue;
        }
        let Some(name) = field.alias_or_name() else {
            continue;
        };
        let bridged = sdbl_type_to_ty(&field.ty);
        named_fields.push((Name::new(name.as_str()), bridged));
        // Eager rendering — the shadow lives behind `Option<Arc<…>>` so
        // it amortises across all readers of the projection.
        shadows.push(SdblTypeShadow { display: field.ty.to_string() });
    }

    if named_fields.is_empty() {
        return None;
    }

    // Invariant: shadow vector indexes mirror the field vector. If this
    // ever drifts (e.g. someone introduces an early-continue between
    // the two pushes), every consumer that reads `raw_sdbl_types[i]`
    // alongside `fields[i]` would silently misalign.
    debug_assert_eq!(
        named_fields.len(),
        shadows.len(),
        "SdblProjection invariant: raw_sdbl_types.len() must equal fields.len()",
    );

    Some(Arc::new(SdblProjection {
        fields: named_fields.into(),
        raw_sdbl_types: Some(shadows.into()),
    }))
}

/// Extract per-query projections from an entire SDBL package.
///
/// Result length equals the package's query count (`pkg.queries().len()`).
/// Indices align with `pkg.queries()` — `result[i]` is the projection of
/// the `i`-th sub-query in the batch. Phase 3 attaches the result to
/// [`Ty::QueryBatchResult { per_query: ... }`].
pub fn package_to_projections(pkg: &sdbl_hir::SdblPackage) -> Vec<Option<Arc<SdblProjection>>> {
    pkg.queries().iter().map(query_to_projection).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdbl_hir::SdblType;

    fn boxed_number() -> Box<SdblType> {
        Box::new(SdblType::Number { precision: Some(15), scale: Some(2) })
    }

    #[test]
    fn primitives_bridge_to_structural_ty() {
        // Display-only attributes (length / precision / scale) drop out
        // of `Ty` — the shadow path preserves them; here we only check
        // the structural projection.
        assert_eq!(sdbl_type_to_ty(&SdblType::Boolean), Ty::Boolean);
        assert_eq!(sdbl_type_to_ty(&SdblType::string()), Ty::String);
        assert_eq!(sdbl_type_to_ty(&SdblType::string_with_length(50)), Ty::String);
        assert_eq!(
            sdbl_type_to_ty(&SdblType::Number { precision: Some(15), scale: Some(2) }),
            Ty::Number,
        );
        assert_eq!(sdbl_type_to_ty(&SdblType::Date), Ty::Date);
        assert_eq!(sdbl_type_to_ty(&SdblType::DateTime), Ty::Date);
        assert_eq!(sdbl_type_to_ty(&SdblType::Null), Ty::Null);
        assert_eq!(sdbl_type_to_ty(&SdblType::ValueTable), Ty::ValueTable);
        assert_eq!(sdbl_type_to_ty(&SdblType::AnyRef), Ty::Unknown);
        assert_eq!(sdbl_type_to_ty(&SdblType::Unknown), Ty::Unknown);
        assert_eq!(sdbl_type_to_ty(&SdblType::Error), Ty::Unknown);
    }

    #[test]
    fn uuid_and_value_storage_lower_to_platform_objects() {
        // The bilingual platform-data index resolves both Russian and
        // English names — Russian is the canonical SDBL-side spelling.
        assert_eq!(
            sdbl_type_to_ty(&SdblType::Uuid),
            Ty::PlatformObject(Name::new("УникальныйИдентификатор")),
        );
        assert_eq!(
            sdbl_type_to_ty(&SdblType::ValueStorage),
            Ty::PlatformObject(Name::new("ХранилищеЗначения")),
        );
    }

    #[test]
    fn ref_bridges_to_matching_metadata_ref_kind() {
        let r = SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Catalog, "Товары"));
        assert_eq!(
            sdbl_type_to_ty(&r),
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Товары") },
        );

        let r = SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Document, "ПКО"));
        assert_eq!(
            sdbl_type_to_ty(&r),
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") },
        );
    }

    #[test]
    fn any_object_ref_lowers_to_dedicated_variant() {
        // `AnyObjectRef { Catalog }` is the "some catalog reference, no
        // specific name" cell — distinct from `Ty::ManagerCollection`
        // which models the global manager container.
        let t = SdblType::AnyObjectRef { mdo_type: MdoType::Catalog };
        assert_eq!(sdbl_type_to_ty(&t), Ty::AnyMetadataRef { mdo_type: MdoType::Catalog });
    }

    #[test]
    fn defined_type_with_underlying_recurses() {
        let t = SdblType::DefinedType {
            name: "Деньги".to_string(),
            underlying_type: Some(boxed_number()),
        };
        assert_eq!(sdbl_type_to_ty(&t), Ty::Number);
    }

    #[test]
    fn defined_type_without_underlying_falls_to_unknown() {
        // No hir-def-side resolver expansion in Phase 1; bridge surfaces
        // `Unknown` so callers don't false-positive a type-error.
        let t = SdblType::DefinedType { name: "Деньги".to_string(), underlying_type: None };
        assert_eq!(sdbl_type_to_ty(&t), Ty::Unknown);
    }

    #[test]
    fn aggregate_strips_wrapper() {
        // `SUM(Number) → Aggregate(Number)` — the aggregate marker has
        // no BSL counterpart, so the bridge strips it.
        let t = SdblType::Aggregate(boxed_number());
        assert_eq!(sdbl_type_to_ty(&t), Ty::Number);
    }

    #[test]
    fn composite_lowers_via_ty_union() {
        // The smart constructor sorts + dedups via `Ord` on `Ty`;
        // bridge correctness here is "every arm is bridged once". Order
        // is irrelevant — `Ty::union` is commutative under `PartialEq`.
        let t = SdblType::Composite {
            types: vec![
                SdblType::Boolean,
                SdblType::string(),
                SdblType::Number { precision: None, scale: None },
            ],
        };
        let ty = sdbl_type_to_ty(&t);
        match ty {
            Ty::Union(arms) => {
                assert_eq!(arms.len(), 3, "expected 3 distinct arms after dedup, got {arms:?}");
                assert!(arms.contains(&Ty::Boolean));
                assert!(arms.contains(&Ty::String));
                assert!(arms.contains(&Ty::Number));
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn tabular_section_ref_carries_parent_pair() {
        let t = SdblType::TabularSectionRef {
            parent_mdo_type: MdoType::Document,
            parent_mdo_name: "ПКО".to_string(),
            ts_name: "Товары".to_string(),
        };
        assert_eq!(
            sdbl_type_to_ty(&t),
            Ty::MetadataRef {
                kind: MetadataKind::TabularSection { parent: MdoType::Document },
                name: Name::new("ПКО.Товары"),
            },
        );
    }

    #[test]
    fn ref_kind_for_returns_none_for_managerless_mdo() {
        // `CommonModule` has no reference cell in `MetadataKind` — SDBL
        // doesn't put it there either, but the safety hatch keeps the
        // bridge from panicking on a future SDBL extension.
        assert_eq!(ref_kind_for(MdoType::CommonModule), None);
    }

    #[test]
    fn ref_without_matching_metadata_kind_falls_to_any_metadata_ref() {
        // `ChartOfCharacteristicTypes` is a real MDO family but
        // `hir-def::MetadataKind` doesn't carry a `*Ref` variant for it
        // yet. The bridge preserves the MDO kind (so family-wide
        // completion still works) by routing through `Ty::AnyMetadataRef`
        // — strictly better than dropping to `Ty::Unknown` and losing
        // the SDBL provenance.
        let r = SdblType::Ref(sdbl_hir::MdoRef::new(
            MdoType::ChartOfCharacteristicTypes,
            "ВидыНоменклатуры",
        ));
        assert_eq!(
            sdbl_type_to_ty(&r),
            Ty::AnyMetadataRef { mdo_type: MdoType::ChartOfCharacteristicTypes },
        );
    }

    #[test]
    fn composite_with_aggregate_folds_to_single_arm() {
        // `SUM(Number)` lowers to `Aggregate(Number)` which the bridge
        // strips to `Number`. A composite of `[Number, Aggregate(Number)]`
        // must dedup down to a single `Number` after bridging — the
        // smart `Ty::union` constructor is responsible for the fold;
        // this test pins the contract end-to-end so a future refactor
        // that reorders aggregate handling can't silently break it.
        let t = SdblType::Composite {
            types: vec![
                SdblType::Number { precision: None, scale: None },
                SdblType::Aggregate(Box::new(SdblType::Number { precision: None, scale: None })),
            ],
        };
        assert_eq!(sdbl_type_to_ty(&t), Ty::Number);
    }
}
