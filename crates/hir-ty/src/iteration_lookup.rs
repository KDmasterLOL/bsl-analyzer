//! Type inference for `Для каждого … Из …` loop variables.
//!
//! Maps a collection's [`TypeId`] to the element type yielded by `Для
//! каждого`. Source of truth is the `Элементы коллекции:` chapter of
//! the BSL platform syntax help (HBK), captured at JSON-regen time as
//! [`bsl_platform::PlatformType::iter_element_types`]. This module
//! does **not** carry a Rust-side mapping from collection name to
//! element name; everything flows through platform data.
//!
//! ## Algorithm
//!
//! For each call site, the resolver reads the receiver shape from
//! [`TypeKernelDb::lookup_type`], then:
//!
//! 1. Names the receiver type — `TypeKind::PlatformObject` carries its
//!    name directly; `TypeKind::MetadataRef` is keyed by its
//!    [`bsl_types::kind::MetadataKind::platform_prefix`] (e.g.
//!    `"InformationRegisterRecordSet"`); the unparametric
//!    `TypeKind::Array`/`Map`/`ValueTable` map to their Russian HBK
//!    names; `TypeKind::Union` recurses on live members (after stripping
//!    `Undefined`/`Null`) and unions the results. A parameterised
//!    `Array { element: Some(_) }` surfaces its element directly, and a
//!    projected `ValueTable` short-circuits to its projected row.
//! 2. Looks up `iter_element_types: Vec<SmolStr>` for that type — empty
//!    `Vec` means the platform did not declare iteration, so the result
//!    is `None` (the loop variable stays `Unknown`).
//! 3. Resolves each template string to a [`TypeId`]. Composite templates
//!    (`"РегистрСведенийЗапись.<Имя регистра сведений>"`) flow through
//!    [`crate::platform_manager_lookup::map_generic_metadata_return_type_typeid`]
//!    using the receiver's MDO name; scalar templates flow through
//!    [`crate::lower::type_string::lower_platform_type_name_typeid`].
//! 4. Single template → `Some(id)`; multi-template page (e.g.
//!    `ПоляКолонкиСхемыЗапроса` lists three admissible element types)
//!    → `Some(db.union([…]))`.
//!
//! ## Why pass through platform data, not a hand-rolled table
//!
//! The HBK lists 304 iterable types. Encoding the pairing in Rust would
//! mean re-curating that list and drifting from the platform spec on
//! every regen. Letting the parser stamp `iter_element_types` onto each
//! `PlatformType` keeps the inference data-driven: a future HBK that
//! adds (or removes) iteration support flows through automatically.

use std::sync::Arc;

use bsl_metadata::MdoType;
use bsl_platform::PlatformData;
use bsl_types::builders::Builders;
use bsl_types::facet::TableSource;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{MetadataKind, Projection, TypeId, TypeKind};
use hir_def::Name;
use smol_str::SmolStr;

use crate::lower::type_string::lower_platform_type_name_typeid;
use crate::platform_manager_lookup::{
    map_generic_metadata_return_type_typeid, metadata_kind_to_prefix_and_mdo,
};

/// Resolve the element type for `Для каждого … Из <collection> Цикл`.
///
/// Phase 3 §4.E.4b (boundary-flip): the public surface and the internal
/// resolution are both kernel-native (`collection: TypeId`, returns
/// `Option<TypeId>`). The inner pipeline matches on `db.lookup_type` and
/// resolves element templates through the `_typeid` lowerers
/// (`lower_platform_type_name_typeid`,
/// `map_generic_metadata_return_type_typeid`) — no `Ty` round-trip.
///
/// Receiver-side `Unknown` absorption: because `collection` is an
/// interned [`TypeId`], a union receiver that mixed `Unknown` with a
/// concrete arm (`Unknown | Массив`) was already canonicalised to the
/// concrete arm at intern time (kernel `canonicalise_union` absorbs
/// `Unknown`, single-source-of-truth rule, same as §4.E.1). So such a
/// receiver iterates as its concrete arm rather than reaching the
/// mixed-arm `None` guard in [`resolve_union`] — consistent with
/// gradual typing, where the `Unknown` arm adds no constraint.
pub(crate) fn resolve_iter_element_ty(db: &dyn TypeKernelDb, collection: TypeId) -> Option<TypeId> {
    resolve_iter_element_ty_inner(db, collection)
}

/// Receiver shape extracted from `db.lookup_type` — owns its data so the
/// `&TypeKind` borrow is released before we call back into `db`.
enum IterShape {
    /// Projected `ТаблицаЗначений` → projected row (carries provenance).
    Row { projection: Option<Arc<Projection>>, source: TableSource },
    /// Parameterised `Массив` element surfaced directly.
    ArrayElement(TypeId),
    /// Platform `iter_element_types` templates + optional MDO context
    /// for `<Имя>` substitution.
    Templates { templates: Vec<SmolStr>, context: Option<(MdoType, Name)> },
    /// `Union` arms — resolved per live arm, intersected to `None` on a
    /// non-iterable arm.
    Union(Vec<TypeId>),
    /// Receiver the platform did not declare iterable.
    Unsupported,
}

/// Kernel-native resolver.
///
/// Returns `None` when the platform syntax help did not declare the
/// receiver iterable, when a `Union` mixes iterable and non-iterable
/// arms (overinference would be wrong), or when the substituted template
/// cannot be resolved (e.g. `<Имя>`-substitution without an MDO
/// context). On success returns the element `TypeId`, possibly a union
/// for HBK pages that list multiple admissible elements.
fn resolve_iter_element_ty_inner(db: &dyn TypeKernelDb, collection: TypeId) -> Option<TypeId> {
    let from_name = |name: &str| match lookup_by_type_name(name) {
        Some(templates) => IterShape::Templates { templates, context: None },
        None => IterShape::Unsupported,
    };

    // Read the receiver shape into owned data; no `db` callbacks inside
    // this match (the helpers only touch stateless `PlatformData`).
    let shape = match db.lookup_type(collection) {
        // Phase H Slice 3 — projected `ТаблицаЗначений` short-circuits the
        // platform-template path: the row carries the same projection so
        // `Для Каждого Стр Из <ТЗ> → Стр.<column>` resolves through the
        // projection slice. Projection-less ValueTable falls through to
        // the regular `СтрокаТаблицыЗначений` lookup.
        TypeKind::ValueTable(f) if f.projection.is_some() => {
            IterShape::Row { projection: f.projection.clone(), source: f.source }
        }
        TypeKind::PlatformObject(f) => from_name(f.name.as_str()),
        TypeKind::MetadataRef(f) => match lookup_by_metadata_kind(f.kind) {
            Some(templates) => {
                let context = metadata_kind_to_prefix_and_mdo(f.kind)
                    .map(|(_, mdo)| (mdo, Name::new(f.name.as_str())));
                IterShape::Templates { templates, context }
            }
            None => IterShape::Unsupported,
        },
        // Parameterised array bypasses the platform `iter_element_types`
        // table whose only declared element for `Массив` is
        // `"Произвольный"` (→ `Unknown`). The element type the caller
        // threaded through the array facet is precisely the information
        // that table is missing — surface it directly.
        TypeKind::Array(f) => match f.element {
            Some(elem) => IterShape::ArrayElement(elem),
            None => from_name("Массив"),
        },
        TypeKind::Map(_) => from_name("Соответствие"),
        TypeKind::ValueTable(_) => from_name("ТаблицаЗначений"),
        TypeKind::ValueTableRow(_) => from_name("СтрокаТаблицыЗначений"),
        TypeKind::ValueList(_) => from_name("СписокЗначений"),
        TypeKind::Structure(_) => from_name("Структура"),
        TypeKind::Union(arms) => IterShape::Union(arms.to_vec()),
        _ => IterShape::Unsupported,
    };

    match shape {
        IterShape::Row { projection, source } => Some(db.value_table_row(projection, source)),
        IterShape::ArrayElement(elem) => Some(elem),
        IterShape::Templates { templates, context } => resolve_templates(db, &templates, context),
        IterShape::Union(arms) => resolve_union(db, &arms),
        IterShape::Unsupported => None,
    }
}

/// Resolve `iter_element_types` for the receiver named `name` (Russian
/// or English, case-insensitive — `PlatformData::get_type` handles the
/// bilingual lookup).
fn lookup_by_type_name(name: &str) -> Option<Vec<SmolStr>> {
    let t = PlatformData::instance().get_type(name)?;
    if t.iter_element_types.is_empty() {
        None
    } else {
        Some(t.iter_element_types.clone())
    }
}

/// Resolve `iter_element_types` for a `Ty::MetadataRef` receiver. The
/// HBK indexes parametric collection types under composite names like
/// `"InformationRegisterRecordSet.<Information register name>"`, so a
/// linear scan over `all_types()` matching the English prefix is the
/// honest probe.
fn lookup_by_metadata_kind(kind: MetadataKind) -> Option<Vec<SmolStr>> {
    let (prefix, _) = metadata_kind_to_prefix_and_mdo(kind)?;
    let needle = format!("{}.", prefix.to_lowercase());
    PlatformData::instance()
        .all_types()
        .iter()
        .find(|t| t.english_name.to_lowercase().starts_with(&needle))
        .filter(|t| !t.iter_element_types.is_empty())
        .map(|t| t.iter_element_types.clone())
}

/// `Ty::Union` semantics: every live arm must be iterable. Mixing
/// iterable and non-iterable members would let the loop variable
/// silently widen to a partial element type — surface `None` instead
/// so the inference stays honest.
fn resolve_union(db: &dyn TypeKernelDb, arms: &[TypeId]) -> Option<TypeId> {
    let alive: Vec<TypeId> = arms
        .iter()
        .copied()
        .filter(|t| !matches!(db.lookup_type(*t), TypeKind::Undefined | TypeKind::Null))
        .collect();
    if alive.is_empty() {
        return None;
    }
    let resolved: Option<Vec<TypeId>> =
        alive.iter().map(|t| resolve_iter_element_ty_inner(db, *t)).collect();
    let resolved = resolved?;
    if resolved.len() == 1 {
        Some(resolved[0])
    } else {
        Some(db.union(resolved))
    }
}

/// Resolve a list of element templates, threading the receiver's
/// `(MdoType, Name)` context for `<Имя>` substitution where present.
fn resolve_templates(
    db: &dyn TypeKernelDb,
    templates: &[SmolStr],
    context: Option<(MdoType, Name)>,
) -> Option<TypeId> {
    let ctx = context.as_ref().map(|(m, n)| (*m, n));
    let resolved: Vec<TypeId> =
        templates.iter().filter_map(|tpl| resolve_one_template(db, tpl, ctx)).collect();
    match resolved.len() {
        0 => None,
        1 => Some(resolved[0]),
        _ => Some(db.union(resolved)),
    }
}

/// Resolve one element template (`"Произвольный"`, `"СтрокаТаблицыЗначений"`,
/// `"РегистрСведенийЗапись.<Имя регистра сведений>"`, …).
fn resolve_one_template(
    db: &dyn TypeKernelDb,
    template: &str,
    context: Option<(MdoType, &Name)>,
) -> Option<TypeId> {
    // Composite shape: head before `.` is the generic kind name, the
    // tail (literal `<Имя …>`) is replaced by the receiver's mdo_name.
    if let Some(dot_pos) = template.find('.') {
        let head = &template[..dot_pos];
        let (mdo, mdo_name) = context?;
        return map_generic_metadata_return_type_typeid(db, head, mdo, mdo_name);
    }

    // Scalar shape: pass straight through the platform-name resolver
    // (handles `Произвольный` → `Unknown` and the `PlatformObject`
    // fallback for everything else).
    Some(lower_platform_type_name_typeid(db, template))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;
    use hir_def::ty::Ty;

    use crate::ty_bridge::{ty_to_typeid, typeid_to_ty};

    /// Phase 3 §4.E.4 test shim: the public `resolve_iter_element_ty`
    /// now takes `db` + an interned `TypeId` and returns `Option<TypeId>`.
    /// Tests build readable `Ty` fixtures, so this interns the collection
    /// and bridges the kernel result back to `Ty`. A fresh sandbox
    /// [`InMemoryDb`] per call is fine — platform lookup is stateless
    /// w.r.t. the intern table.
    fn resolve(collection: &Ty) -> Option<Ty> {
        let db = InMemoryDb::new();
        super::resolve_iter_element_ty(&db, ty_to_typeid(&db, collection))
            .map(|id| typeid_to_ty(&db, id))
    }

    #[test]
    fn array_yields_unknown_per_platform_arbitrary() {
        // Per HBK, `Массив` iterates `Произвольный`, which lowers to
        // `Ty::Unknown` via `lower_platform_type_name`.
        let elem = resolve(&Ty::Array);
        assert_eq!(elem, Some(Ty::Unknown));
    }

    #[test]
    fn map_yields_kluch_i_znachenie() {
        // `Соответствие` iterates `КлючИЗначение`.
        let elem = resolve(&Ty::Map);
        assert_eq!(elem, Some(Ty::PlatformObject(Name::new("КлючИЗначение"))));
    }

    #[test]
    fn value_table_yields_row() {
        // `ТаблицаЗначений` iterates `СтрокаТаблицыЗначений`.
        let elem = resolve(&Ty::ValueTable { projection: None });
        assert_eq!(elem, Some(Ty::PlatformObject(Name::new("СтрокаТаблицыЗначений"))));
    }

    #[test]
    fn information_register_record_set_yields_record_with_mdo_name() {
        // `Ty::MetadataRef { InformationRegisterRecordSet, "БУС_..." }`
        // iterates `Ty::MetadataRef { InformationRegisterRecord, "БУС_..." }`.
        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("БУС_ЗаполнениеКаталога"),
        };
        let elem = resolve(&receiver);
        assert_eq!(
            elem,
            Some(Ty::MetadataRef {
                kind: MetadataKind::InformationRegisterRecord,
                name: Name::new("БУС_ЗаполнениеКаталога"),
            })
        );
    }

    #[test]
    fn accumulation_register_record_set_yields_record_with_mdo_name() {
        let receiver = Ty::MetadataRef {
            kind: MetadataKind::AccumulationRegisterRecordSet,
            name: Name::new("ОстаткиТоваров"),
        };
        let elem = resolve(&receiver);
        assert_eq!(
            elem,
            Some(Ty::MetadataRef {
                kind: MetadataKind::AccumulationRegisterRecord,
                name: Name::new("ОстаткиТоваров"),
            })
        );
    }

    #[test]
    fn typed_array_yields_inner_element_directly() {
        // `Ty::TypedArray(t)` bypasses the platform `iter_element_types`
        // table (which only declares `"Произвольный"` for `Массив`) and
        // surfaces `t` directly. This is the whole point of carrying
        // the element type through the new variant.
        let elem = resolve(&Ty::TypedArray(Box::new(Ty::String)));
        assert_eq!(elem, Some(Ty::String));
    }

    #[test]
    fn union_of_array_and_typed_array_iterates_to_concrete_arm() {
        // A `Union(Ty::Array, Ty::TypedArray(String))` collection
        // iterates to the element union of `Ty::Unknown` (`Ty::Array`
        // resolves `["Произвольный"]` → `Ty::Unknown`) and `Ty::String`
        // (`Ty::TypedArray` surfaces its element directly). Phase 3
        // §4.E.4: the kernel surface returns an interned `TypeId`, and
        // kernel `canonicalise_union` ABSORBS `Unknown` (single-source-
        // of-truth rule, same divergence as §4.E.1) — so the element
        // type collapses to the concrete `Ty::String` arm rather than
        // the legacy `Union(Unknown, String)` the `Ty`-native smart
        // constructor produced.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::TypedArray(Box::new(Ty::String))];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve(&union);
        assert_eq!(elem, Some(Ty::String));
    }

    #[test]
    fn union_receiver_with_unknown_arm_absorbs_to_concrete_iterable() {
        // §4.E.4 receiver-side divergence: `Union(Unknown, Array)` is
        // interned as the collection receiver, and kernel
        // `canonicalise_union` ABSORBS the `Unknown` arm -> the receiver
        // is just `Array`. So iteration yields `Array`'s element
        // (`Произвольный` -> `Unknown`) instead of the legacy `None`
        // that the `Ty`-native `resolve_union` returned (it treated the
        // live `Unknown` arm as a blocking non-iterable). Consistent
        // with gradual typing: the `Unknown` arm adds no constraint.
        let arms: Vec<Ty> = vec![Ty::Unknown, Ty::Array];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        assert_eq!(resolve(&union), Some(Ty::Unknown));

        // Same rule with a typed concrete arm: `Unknown | TypedArray(String)`
        // collapses to `TypedArray(String)` and iterates to `String`.
        let arms: Vec<Ty> = vec![Ty::Unknown, Ty::TypedArray(Box::new(Ty::String))];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        assert_eq!(resolve(&union), Some(Ty::String));
    }

    #[test]
    fn typed_array_with_metadata_ref_element_passes_through_unchanged() {
        // The element can be any `Ty`. Iteration must not touch it —
        // the wrapper only owns the array semantics.
        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
            name: Name::new("ПКО.Товары"),
        };
        let collection = Ty::TypedArray(Box::new(row.clone()));
        let elem = resolve(&collection);
        assert_eq!(elem, Some(row));
    }

    #[test]
    fn non_iterable_string_returns_none() {
        // Plain strings have no `Элементы коллекции:` chapter.
        let elem = resolve(&Ty::String);
        assert_eq!(elem, None);
    }

    #[test]
    fn union_with_undefined_filters_dead_arm() {
        // `Массив | Undefined` is iterable on its live arm; the
        // `Undefined` is dead for iteration. Element stays `Ty::Unknown`
        // (Произвольный) — the live arm's element.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::Undefined];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve(&union);
        assert_eq!(elem, Some(Ty::Unknown));
    }

    #[test]
    fn union_with_non_iterable_arm_returns_none() {
        // `Массив | Строка` mixes iterable and non-iterable arms —
        // overinference would be wrong, so the answer is `None`.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::String];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve(&union);
        assert_eq!(elem, None);
    }
}
