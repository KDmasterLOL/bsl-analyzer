//! Bridge from SDBL HIR (`sdbl_hir`) types to BSL types (`hir_def::Ty`).
//!
//! The bridge is the single source of truth for "how does an SDBL field
//! type project onto the BSL type system?". Projection producers mint
//! kernel `TypeId`s through the caller's `TypeKernelDb`. Callers
//! (Phase 1.3 inference hooks) consume
//! `query_to_projection` / `package_to_projections` to attach
//! [`Projection`] payloads to the new
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
//! ## Asterisk expansion
//!
//! `FieldHir { is_asterisk: true }` is expanded against the originating
//! `ResolvedTable::fields()`:
//!
//! - Bare `*` — expanded against every table in `SdblHir::all_tables()`
//!   (FROM ∪ JOINs) in declaration order. Duplicate output names across
//!   tables are deduped first-wins, mirroring `lookup_field`'s linear
//!   scan.
//! - `Т.*` (or `Справочник.Товары.*`) — the lowerer preserves the
//!   qualifier in `FieldHir::asterisk_qualifier`; the bridge matches it
//!   case-insensitively against `TableRef::effective_name()` / `full_name`
//!   and expands the single matching table.
//!
//! Mixed `SELECT *, NamedField` produces a projection with the expanded
//! asterisk first, followed by named fields, deduped by lowercased name
//! (first occurrence wins). Dedup is bilingual when a `FieldDef`
//! carries both a Russian and an English spelling — the dedup `seen` set
//! receives both forms so a later named field that re-projects the
//! English spelling of an asterisk-expanded Russian field is dropped.
//!
//! Asterisks against tables with no resolved metadata (parser-error FROM
//! / unresolved temp tables) contribute no fields — they degrade the
//! projection silently rather than emitting placeholder `Unknown`
//! entries. Surfacing those unresolved sources as diagnostics is the
//! responsibility of the `ide-diagnostics` / SDBL diagnostic layers,
//! not the bridge: the bridge is a pure projection extractor and never
//! emits its own warnings.
//!
//! Register virtual tables (`.Обороты(...)`, `.СрезПоследних(...)`) are
//! pre-processed by the SDBL lowerer (`crates/sdbl-hir/src/lower/from_clause.rs`):
//! the synthesised columns (`<Resource>Оборот`, etc.) land in
//! `ResolvedTable::Register::fields` before the bridge runs, so the
//! bridge expands them transparently without virtual-table-specific code.
//!
//! ## What the bridge does NOT do
//!
//! - **Variable refinement / data-flow**. The bridge takes a lowered
//!   `SdblPackage` as input; it does not trace `.Текст = "..."`
//!   assignments. That lives in `hir-ty::query_text_dataflow` (Phase 2).
//! - **CASE-WHEN heterogeneity**. SDBL HIR may model `ВЫБОР КОГДА X ТОГДА
//!   A ИНАЧЕ Б` as `SdblType::Composite`; the bridge's `Composite` arm
//!   then unions the bridged arm types. If the SDBL lowerer emits a
//!   single `SdblType` instead, the bridge inherits that decision.

use std::sync::Arc;

use bsl_metadata::MdoType;
use bsl_types::builders::Builders;
use bsl_types::facet::{DateComponent, SdblTypeShadowFacet, TableSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId,
};
use bsl_types::testing::RootConfigCtx;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::Name;

/// Kernel-native counterpart of [`sdbl_type_to_ty`].
///
/// Mints the `TypeId` directly through the kernel [`Builders`], mirroring
/// [`sdbl_type_to_ty`] arm-for-arm. The mapping is **lossy-structural** —
/// SDBL precision/scale/length facets drop (they live on the
/// [`SdblTypeShadowFacet`] display shadow), so each arm is byte-identical to
/// `ty_to_typeid(db, &sdbl_type_to_ty(t))` (asserted by the drift test).
#[allow(dead_code, reason = "Phase 3 §4.C producer — projection callers migrate in 4.C.2")]
pub fn sdbl_type_to_typeid(db: &dyn TypeKernelDb, t: &sdbl_hir::SdblType) -> TypeId {
    use sdbl_hir::SdblType as S;
    match t {
        S::Boolean => db.boolean(),
        S::String { .. } => db.string(None, false),
        S::Number { .. } => db.number(None, None),
        S::Date | S::DateTime => db.date(DateComponent::DateTime),
        S::Ref(mdo) => mdo_ref_to_typeid(db, mdo),
        S::AnyRef => db.unknown(),
        S::AnyObjectRef { mdo_type } => db.any_metadata_ref(*mdo_type),
        S::Uuid => db.platform_object("УникальныйИдентификатор".to_string()),
        S::ValueStorage => db.platform_object("ХранилищеЗначения".to_string()),
        S::DefinedType { underlying_type, .. } => underlying_type
            .as_deref()
            .map(|inner| sdbl_type_to_typeid(db, inner))
            .unwrap_or_else(|| db.unknown()),
        S::ValueTable => db.value_table(None, TableSource::Unknown),
        S::Null => db.null(),
        S::Aggregate(inner) => sdbl_type_to_typeid(db, inner),
        S::Composite { types } => {
            db.union(types.iter().map(|t| sdbl_type_to_typeid(db, t)).collect())
        }
        S::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => db.metadata_ref(
            MetadataKind::TabularSection { parent: *parent_mdo_type },
            format!("{parent_mdo_name}.{ts_name}"),
            &RootConfigCtx,
        ),
        S::Unknown | S::Error => db.unknown(),
    }
}

/// Map a single SDBL field type to its BSL counterpart.
///
/// Pure function — see the module-level mapping table for the contract.
pub fn sdbl_type_to_ty(t: &sdbl_hir::SdblType) -> Ty {
    use sdbl_hir::SdblType as S;
    match t {
        S::Boolean => Ty::Boolean,
        // Length / precision / scale are display-only enrichments —
        // they drop out of `Ty` (which is structural) and live on
        // [`SdblTypeShadowFacet.display`] for hover.
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
        S::ValueTable => Ty::ValueTable { projection: None },
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

/// Kernel-native counterpart of [`mdo_ref_to_metadata_ref`] — mints the
/// `MetadataRef` id via `db.metadata_ref(.., &RootConfigCtx)` (the same
/// builder + config fallback the bridge uses) or falls back to
/// `db.any_metadata_ref` for MDO families without a `*Ref` companion.
fn mdo_ref_to_typeid(db: &dyn TypeKernelDb, mdo: &sdbl_hir::MdoRef) -> TypeId {
    match ref_kind_for(mdo.mdo_type) {
        Some(kind) => db.metadata_ref(kind, mdo.name.clone(), &RootConfigCtx),
        None => db.any_metadata_ref(mdo.mdo_type),
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
/// Walks the query's `SELECT` field list, expanding asterisks against
/// the originating `ResolvedTable::fields()` and bridging each named
/// field's `SdblType` into a `TypeId`. Returns `Some(projection)` when at
/// least one bridge-able field remains, `None` otherwise — callers
/// attach a `None` projection to the receiver's [`Ty::QueryResult`] /
/// [`Ty::QueryResultSelection`] in that case.
///
/// Field-name priority for named fields follows
/// [`sdbl_hir::FieldHir::alias_or_name`]: alias > column name > raw
/// name. Fields with no recoverable name are dropped.
///
/// Duplicate names (across asterisk expansions, across joined tables,
/// or between an asterisk-expanded field and a named field that
/// re-projects the same column) are deduped first-wins to mirror
/// `lookup_field`'s linear scan order.
pub fn query_to_projection(
    db: &dyn TypeKernelDb,
    q: &sdbl_hir::SdblQuery,
) -> Option<Arc<Projection>> {
    // Capacity is a best-effort hint — asterisk expansion can grow the
    // result well beyond `select.fields.len()`. Allocating up to the
    // base length avoids over-reservation on the common no-asterisk path.
    let initial_cap = q.hir.select.fields.len();
    let mut named_fields: Vec<ProjectionField> = Vec::with_capacity(initial_cap);
    let mut shadows: Vec<SdblTypeShadowFacet> = Vec::with_capacity(initial_cap);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Seeds `seen` with every dedup key tied to this insertion so
    // bilingual collisions and earlier asterisk expansions are caught
    // first-wins. Returns whether the field was newly inserted.
    let push_unique = |name: Name,
                       alt_keys: &[&str],
                       ty: TypeId,
                       shadow: SdblTypeShadowFacet,
                       named_fields: &mut Vec<ProjectionField>,
                       shadows: &mut Vec<SdblTypeShadowFacet>,
                       seen: &mut std::collections::HashSet<String>|
     -> bool {
        let primary_key = name.as_str().to_lowercase();
        if !seen.contains(&primary_key)
            && !alt_keys.iter().any(|k| seen.contains(&k.to_lowercase()))
        {
            seen.insert(primary_key);
            for k in alt_keys {
                seen.insert(k.to_lowercase());
            }
            // Uniform `Column` source — `query_to_projection` does not
            // (yet) discriminate Cast/Aggregate fields. Provenance is
            // deterministic, so projection equality across reaching defs
            // (query_text_dataflow) stays stable.
            named_fields.push(ProjectionField::new(
                name.as_str().to_string(),
                ty,
                ProjectionFieldSource::Column,
            ));
            shadows.push(shadow);
            true
        } else {
            false
        }
    };

    for field in &q.hir.select.fields {
        if field.has_parse_error {
            continue;
        }
        if field.is_asterisk {
            for (name, alt_en, ty, shadow) in
                expand_asterisk(db, field.asterisk_qualifier.as_deref(), &q.hir)
            {
                let alt_keys: Vec<&str> = alt_en.as_deref().into_iter().collect();
                push_unique(
                    name,
                    &alt_keys,
                    ty,
                    shadow,
                    &mut named_fields,
                    &mut shadows,
                    &mut seen,
                );
            }
            continue;
        }
        let Some(name) = field.alias_or_name() else {
            continue;
        };
        let bridged = sdbl_type_to_typeid(db, &field.ty);
        let shadow = SdblTypeShadowFacet::new(field.ty.to_string());
        push_unique(
            Name::new(name.as_str()),
            &[],
            bridged,
            shadow,
            &mut named_fields,
            &mut shadows,
            &mut seen,
        );
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
        "Projection invariant: raw_sdbl_types.len() must equal fields.len()",
    );

    Some(Arc::new(Projection::new(
        named_fields.into(),
        ProjectionOrigin::SdblQuery,
        Some(shadows.into()),
    )))
}

/// Expand an asterisk field against the tables in scope.
///
/// - `qualifier == None` (bare `*`): yields all fields from every table
///   in `hir.all_tables()`, in declaration order (FROM then JOINs).
/// - `qualifier == Some(q)` (`Т.*`): yields the fields of every table
///   whose `effective_name()` or `full_name` matches `q`
///   case-insensitively. In valid SDBL only one table matches —
///   alias collisions are surfaced as diagnostics elsewhere — but the
///   bridge stays defensive and emits all matches in declaration order.
///
/// The English spelling of bilingual `FieldDef`s is also yielded
/// alongside the primary name so the upstream dedup `seen` set can
/// catch later named fields that re-project the English form of an
/// asterisk-expanded Russian field. The returned shape is
/// `(primary_name, alt_name_en, ty, shadow)`.
///
/// Tables with no resolved metadata (`TableRef::metadata == None`)
/// contribute nothing — they degrade the projection silently instead
/// of introducing `Unknown` placeholders.
fn expand_asterisk(
    db: &dyn TypeKernelDb,
    qualifier: Option<&str>,
    hir: &sdbl_hir::SdblHir,
) -> Vec<(Name, Option<String>, TypeId, SdblTypeShadowFacet)> {
    let qualifier_lower = qualifier.map(|q| q.to_lowercase());
    let mut out = Vec::new();
    for table in hir.all_tables() {
        if let Some(q_lower) = qualifier_lower.as_deref() {
            let effective = table.effective_name().to_lowercase();
            let full = table.full_name.to_lowercase();
            if effective != q_lower && full != q_lower {
                continue;
            }
        }
        let Some(resolved) = &table.metadata else {
            continue;
        };
        for field_def in resolved.fields() {
            out.push((
                Name::new(&field_def.name),
                field_def.name_en.clone(),
                sdbl_type_to_typeid(db, &field_def.ty),
                SdblTypeShadowFacet::new(field_def.ty.to_string()),
            ));
        }
    }
    out
}

/// Extract per-query projections from an entire SDBL package.
///
/// Result length equals the package's query count (`pkg.queries().len()`).
/// Indices align with `pkg.queries()` — `result[i]` is the projection of
/// the `i`-th sub-query in the batch. Phase 3 attaches the result to
/// [`Ty::QueryBatchResult { per_query: ... }`].
pub fn package_to_projections(
    db: &dyn TypeKernelDb,
    pkg: &sdbl_hir::SdblPackage,
) -> Vec<Option<Arc<Projection>>> {
    pkg.queries().iter().map(|q| query_to_projection(db, q)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;
    use sdbl_hir::SdblType;

    fn boxed_number() -> Box<SdblType> {
        Box::new(SdblType::Number { precision: Some(15), scale: Some(2) })
    }

    /// §4.C drift-detector: native minting must produce the *same interned
    /// id* as bridging the legacy `Ty` path. Guards §4.C.2 (the
    /// `Projection.fields` flip) — and ultimately the `Ty`-path deletion
    /// — against any divergence. Covers precision/scale + string length
    /// (must drop), unknown/error, composite-with-unknown, and the ref
    /// fallback (MDO family without a `*Ref` companion).
    #[test]
    fn sdbl_typeid_matches_bridge() {
        use crate::ty_bridge::ty_to_typeid;
        let db = InMemoryDb::new();
        let cases = vec![
            SdblType::Boolean,
            SdblType::string(),
            SdblType::string_with_length(50),
            SdblType::Number { precision: Some(15), scale: Some(2) },
            SdblType::Date,
            SdblType::DateTime,
            SdblType::Null,
            SdblType::ValueTable,
            SdblType::Uuid,
            SdblType::ValueStorage,
            SdblType::AnyRef,
            SdblType::Unknown,
            SdblType::Error,
            SdblType::AnyObjectRef { mdo_type: MdoType::Catalog },
            SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::Catalog, "Товары")),
            // Ref fallback: CommonModule has no `*Ref` companion → any_metadata_ref.
            SdblType::Ref(sdbl_hir::MdoRef::new(MdoType::CommonModule, "Х")),
            SdblType::DefinedType {
                name: "Деньги".to_string(),
                underlying_type: Some(boxed_number()),
            },
            SdblType::Aggregate(boxed_number()),
            SdblType::Composite {
                types: vec![SdblType::Number { precision: None, scale: None }, SdblType::Unknown],
            },
            SdblType::TabularSectionRef {
                parent_mdo_type: MdoType::Catalog,
                parent_mdo_name: "Номенклатура".to_string(),
                ts_name: "Товары".to_string(),
            },
        ];
        for t in &cases {
            assert_eq!(
                sdbl_type_to_typeid(&db, t),
                ty_to_typeid(&db, &sdbl_type_to_ty(t)),
                "drift for {t:?}"
            );
        }
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
        assert_eq!(sdbl_type_to_ty(&SdblType::ValueTable), Ty::ValueTable { projection: None });
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

    // ====================================================================
    // Asterisk expansion — `SELECT *` / `SELECT Т.*` against
    // `ResolvedTable::fields()`.
    // ====================================================================

    use sdbl_hir::{
        ExprHir, FieldDef, FieldHir, ResolvedTable, SdblHir, SdblQuery, SelectHir, TableRef,
    };
    use syntax::MODULE_RANGE;

    fn mk_asterisk(qualifier: Option<&str>) -> FieldHir {
        FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: None,
            has_as_keyword: false,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Unknown,
            is_asterisk: true,
            asterisk_qualifier: qualifier.map(str::to_string),
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        }
    }

    fn mk_named(name: &str, ty: SdblType) -> FieldHir {
        FieldHir {
            expr: ExprHir::ColumnRef {
                parts: vec![sdbl_hir::Name::from(name)],
                ty: ty.clone(),
                range: MODULE_RANGE,
            },
            alias: None,
            has_as_keyword: false,
            has_parse_error: false,
            raw_name: Some(sdbl_hir::Name::from(name)),
            ty,
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        }
    }

    fn mk_metadata_table(full_name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: full_name.split('.').map(sdbl_hir::Name::from).collect(),
            full_name: full_name.to_string(),
            alias: alias.map(sdbl_hir::Name::from),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: full_name.to_string(),
                fields,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_register_table(
        full_name: &str,
        fields: Vec<FieldDef>,
        dimensions: Vec<FieldDef>,
        resources: Vec<FieldDef>,
        attributes: Vec<FieldDef>,
    ) -> TableRef {
        TableRef {
            parts: full_name.split('.').map(sdbl_hir::Name::from).collect(),
            full_name: full_name.to_string(),
            alias: None,
            metadata: Some(ResolvedTable::Register {
                mdo_type: MdoType::AccumulationRegister,
                name: full_name.to_string(),
                fields,
                dimensions,
                resources,
                attributes,
            }),
            is_virtual_table: true,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_temp_table(name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: vec![sdbl_hir::Name::from(name)],
            full_name: name.to_string(),
            alias: alias.map(sdbl_hir::Name::from),
            metadata: Some(ResolvedTable::TempTable { name: name.to_string(), fields }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        }
    }

    fn mk_query(fields: Vec<FieldHir>, from: Vec<TableRef>) -> SdblQuery {
        let mut hir = SdblHir::empty();
        hir.select = SelectHir { fields, distinct: false, top: None };
        hir.from = from;
        SdblQuery { hir, range: MODULE_RANGE }
    }

    fn projection_field_names(p: &Projection) -> Vec<String> {
        p.fields.iter().map(|f| f.name.clone()).collect()
    }

    #[test]
    fn asterisk_expands_metadata_table_fields() {
        let db = InMemoryDb::new();
        // `SELECT * FROM Catalog.Товары` — bare asterisk over a single
        // resolved Metadata table walks every FieldDef and bridges types
        // structurally.
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![
                FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                FieldDef::new("Наименование", SdblType::string_with_length(150)),
                FieldDef::new("Цена", SdblType::Number { precision: Some(15), scale: Some(2) }),
            ],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("asterisk over resolved table must project");
        assert_eq!(projection_field_names(&p), vec!["Ссылка", "Наименование", "Цена"]);
        // Type bridging applies per-field. Asterisk fields lift directly
        // from the FieldDef's `SdblType` — no aliasing layer.
        assert_eq!(p.fields[1].ty, sdbl_type_to_typeid(&db, &SdblType::String { length: None }));
        assert_eq!(
            p.fields[2].ty,
            sdbl_type_to_typeid(&db, &SdblType::Number { precision: Some(15), scale: Some(2) },),
        );
    }

    #[test]
    fn qualified_asterisk_expands_only_matching_table() {
        let db = InMemoryDb::new();
        // `SELECT Т.* FROM Catalog.A AS Т, Catalog.B` — qualifier resolves
        // by alias against `effective_name()`, NOT by table full_name when
        // an alias is present.
        let table_a = mk_metadata_table(
            "Справочник.A",
            Some("Т"),
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let table_b = mk_metadata_table(
            "Справочник.B",
            None,
            vec![FieldDef::new("Другое", SdblType::Boolean)],
        );
        let q = mk_query(vec![mk_asterisk(Some("Т"))], vec![table_a, table_b]);
        let p =
            query_to_projection(&db, &q).expect("qualified asterisk must project matching table");
        assert_eq!(projection_field_names(&p), vec!["Имя"]);
    }

    #[test]
    fn qualified_asterisk_matches_full_name_when_no_alias() {
        let db = InMemoryDb::new();
        // `SELECT Справочник.Товары.*` when no alias is set — qualifier
        // resolves against `full_name`. Case-insensitive comparison.
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::new("Код", SdblType::string_with_length(11))],
        );
        let q = mk_query(vec![mk_asterisk(Some("справочник.товары"))], vec![table]);
        let p =
            query_to_projection(&db, &q).expect("case-insensitive full_name match must project");
        assert_eq!(projection_field_names(&p), vec!["Код"]);
    }

    #[test]
    fn bare_asterisk_walks_all_tables_in_declaration_order() {
        let db = InMemoryDb::new();
        // `SELECT * FROM A, B` — bare asterisk produces A's fields first,
        // then B's. Order is preserved so `lookup_field`'s linear scan
        // matches the first-occurrence-wins rule consumers expect.
        let table_a =
            mk_metadata_table("Справочник.A", None, vec![FieldDef::new("X", SdblType::Boolean)]);
        let table_b =
            mk_metadata_table("Справочник.B", None, vec![FieldDef::new("Y", SdblType::string())]);
        let q = mk_query(vec![mk_asterisk(None)], vec![table_a, table_b]);
        let p = query_to_projection(&db, &q).expect("bare asterisk with tables must project");
        assert_eq!(projection_field_names(&p), vec!["X", "Y"]);
    }

    #[test]
    fn bare_asterisk_dedupes_duplicate_names_first_wins() {
        let db = InMemoryDb::new();
        // Two tables both expose `Ссылка`. First-wins dedup keeps the
        // first table's type (here a CatalogRef) and drops the second's,
        // mirroring `lookup_field`'s linear scan order.
        let table_a = mk_metadata_table(
            "Справочник.A",
            None,
            vec![FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "A"))],
        );
        let table_b = mk_metadata_table(
            "Справочник.B",
            None,
            vec![FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "B"))],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table_a, table_b]);
        let p =
            query_to_projection(&db, &q).expect("bare asterisk must project at least one field");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].name.as_str(), "Ссылка");
        assert_eq!(
            p.fields[0].ty,
            sdbl_type_to_typeid(&db, &SdblType::reference(MdoType::Catalog, "A")),
        );
    }

    #[test]
    fn mixed_asterisk_and_named_appends_named_after_expansion() {
        let db = InMemoryDb::new();
        // `SELECT *, NamedField` — asterisk-expanded fields come first,
        // named field appended. Dedup is by lowercased name first-wins:
        // a named field that re-projects an asterisk field (same name)
        // is dropped.
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![
                FieldDef::new("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                FieldDef::new("Наименование", SdblType::string_with_length(150)),
            ],
        );
        let q = mk_query(
            vec![
                mk_asterisk(None),
                // Re-projection of an asterisk-expanded name — must be deduped.
                mk_named("Ссылка", SdblType::reference(MdoType::Catalog, "Товары")),
                // Distinct name — must be appended.
                mk_named("Новое", SdblType::Boolean),
            ],
            vec![table],
        );
        let p = query_to_projection(&db, &q).expect("mixed shape must project");
        assert_eq!(projection_field_names(&p), vec!["Ссылка", "Наименование", "Новое"]);
    }

    #[test]
    fn asterisk_against_register_walks_combined_fields() {
        let db = InMemoryDb::new();
        // Register virtual-table fields (`<Resource>Оборот` etc.) are
        // synthesised by the SDBL lowerer into `Register::fields` before
        // the bridge runs. Asterisk over a register expands every field
        // the lowerer prepared — dimensions + resources + attributes when
        // it's a plain register, or the synthesised virtual-table columns
        // when it's `.Обороты(...)`.
        let table = mk_register_table(
            "РегистрНакопления.ОстаткиТоваров.Обороты",
            vec![
                FieldDef::new("Период", SdblType::Date),
                FieldDef::new(
                    "КоличествоОборот",
                    SdblType::Number { precision: None, scale: None },
                ),
                FieldDef::new(
                    "КоличествоПриход",
                    SdblType::Number { precision: None, scale: None },
                ),
                FieldDef::new(
                    "КоличествоРасход",
                    SdblType::Number { precision: None, scale: None },
                ),
            ],
            vec![FieldDef::new("Период", SdblType::Date)],
            vec![FieldDef::new("Количество", SdblType::Number { precision: None, scale: None })],
            Vec::new(),
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("register virtual asterisk must project");
        assert_eq!(
            projection_field_names(&p),
            vec!["Период", "КоличествоОборот", "КоличествоПриход", "КоличествоРасход"],
        );
    }

    #[test]
    fn asterisk_against_temp_table_expands_subquery_fields() {
        let db = InMemoryDb::new();
        // `SELECT * FROM ВТ_Имена AS T` — temp tables carry the
        // originating subquery's SELECT names/types in
        // `ResolvedTable::TempTable::fields`. The bridge treats them
        // identically to Metadata tables.
        let table = mk_temp_table(
            "ВТ_Имена",
            Some("T"),
            vec![
                FieldDef::new("Имя", SdblType::string_with_length(50)),
                FieldDef::new("Активность", SdblType::Boolean),
            ],
        );
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        let p = query_to_projection(&db, &q).expect("temp-table asterisk must project");
        assert_eq!(projection_field_names(&p), vec!["Имя", "Активность"]);
    }

    #[test]
    fn qualified_asterisk_with_no_matching_table_yields_none() {
        let db = InMemoryDb::new();
        // `SELECT Z.* FROM Catalog.A AS Т` — qualifier `Z` matches
        // neither alias nor full_name. Asterisk contributes nothing;
        // the projection is None because no other fields are present.
        let table = mk_metadata_table(
            "Справочник.A",
            Some("Т"),
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let q = mk_query(vec![mk_asterisk(Some("Z"))], vec![table]);
        assert!(
            query_to_projection(&db, &q).is_none(),
            "asterisk with unresolved qualifier must drop silently",
        );
    }

    #[test]
    fn asterisk_against_unresolved_table_yields_none() {
        let db = InMemoryDb::new();
        // `SELECT * FROM <parse_error>` — TableRef::metadata is None.
        // The bridge contributes no fields, mirroring the pre-Phase-A
        // policy for cases where SDBL never resolves the source table.
        let table = TableRef {
            parts: Vec::new(),
            full_name: String::new(),
            alias: None,
            metadata: None,
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![mk_asterisk(None)], vec![table]);
        assert!(query_to_projection(&db, &q).is_none());
    }

    #[test]
    fn bilingual_dedup_drops_named_field_reprojecting_english_spelling() {
        let db = InMemoryDb::new();
        // `Ссылка` and `Ref` are the bilingual pair for `MetadataKind::CatalogRef`'s
        // standard reference attribute. An asterisk expansion that yields
        // `Ссылка` (with `name_en = Some("Ref")`) must seed BOTH spellings
        // into the dedup set so a later `SELECT *, T.Ref AS R` (or any
        // named field re-projecting the English spelling) is dropped.
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::standard(
                "Ссылка",
                "Ref",
                SdblType::reference(MdoType::Catalog, "Товары"),
            )],
        );
        let q = mk_query(
            vec![
                mk_asterisk(None),
                // English-spelling re-projection — must be deduped.
                mk_named("Ref", SdblType::reference(MdoType::Catalog, "Товары")),
            ],
            vec![table],
        );
        let p = query_to_projection(&db, &q).expect("bilingual mixed shape must project");
        assert_eq!(
            projection_field_names(&p),
            vec!["Ссылка"],
            "English spelling of an asterisk-expanded Russian field must dedup first-wins",
        );
    }

    #[test]
    fn asterisk_field_with_parse_error_is_skipped() {
        let db = InMemoryDb::new();
        // Defensive: a parse-error asterisk does not crash expansion;
        // it's skipped entirely, just like any other parse-error field.
        let table = mk_metadata_table(
            "Справочник.Товары",
            None,
            vec![FieldDef::new("Имя", SdblType::string_with_length(50))],
        );
        let mut bad = mk_asterisk(None);
        bad.has_parse_error = true;
        let q = mk_query(vec![bad], vec![table]);
        assert!(query_to_projection(&db, &q).is_none());
    }

    #[test]
    fn cast_projection_field_carries_precise_shadow_display() {
        let db = InMemoryDb::new();
        // Phase G end-to-end contract: a SELECT field whose expression is
        // a CAST/ВЫРАЗИТЬ-typed `SdblType::Number { Some(15), Some(2) }`
        // (the shape the lowerer now produces for
        // `ВЫРАЗИТЬ(0 КАК Число(15, 2))`) must flow precision/scale into
        // `SdblTypeShadowFacet.display` via `field.ty.to_string()`. The
        // structural `Ty` collapses to `Ty::Number` — display lives only
        // in the shadow lane.
        let cast_field = FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: Some(sdbl_hir::Name::from("Цена")),
            has_as_keyword: true,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Number { precision: Some(15), scale: Some(2) },
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![cast_field], Vec::new());
        let p = query_to_projection(&db, &q).expect("CAST field must project");
        assert_eq!(p.fields.len(), 1);
        assert_eq!(p.fields[0].name.as_str(), "Цена");
        assert_eq!(
            p.fields[0].ty,
            sdbl_type_to_typeid(&db, &SdblType::Number { precision: Some(15), scale: Some(2) },),
        );
        let shadows = p.raw_sdbl_types.as_ref().expect("Phase E shadows always populated");
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].display, "Число(15, 2)");
    }

    #[test]
    fn cast_projection_field_renders_precision_only_number() {
        let db = InMemoryDb::new();
        // Precision-only CAST (`ВЫРАЗИТЬ(0 КАК Число(15))`) is a Phase G
        // Slice 2 addition — Display now emits `Число(15)` instead of the
        // bare `Число` it used to collapse to.
        let cast_field = FieldHir {
            expr: ExprHir::Missing { range: MODULE_RANGE },
            alias: Some(sdbl_hir::Name::from("Сумма")),
            has_as_keyword: true,
            has_parse_error: false,
            raw_name: None,
            ty: SdblType::Number { precision: Some(15), scale: None },
            is_asterisk: false,
            asterisk_qualifier: None,
            diagnostic_range: MODULE_RANGE,
            range: MODULE_RANGE,
        };
        let q = mk_query(vec![cast_field], Vec::new());
        let p = query_to_projection(&db, &q).expect("CAST field must project");
        let shadows = p.raw_sdbl_types.as_ref().expect("shadows populated");
        assert_eq!(shadows[0].display, "Число(15)");
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
