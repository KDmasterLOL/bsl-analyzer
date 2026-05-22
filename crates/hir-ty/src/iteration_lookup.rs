//! Type inference for `Для каждого … Из …` loop variables.
//!
//! Bridges a collection's `Ty` to the element type yielded by `Для
//! каждого`. Source of truth is the `Элементы коллекции:` chapter of
//! the BSL platform syntax help (HBK), captured at JSON-regen time as
//! [`bsl_platform::PlatformType::iter_element_types`]. This module
//! does **not** carry a Rust-side mapping from collection name to
//! element name; everything flows through platform data.
//!
//! ## Algorithm
//!
//! For each call site, the resolver:
//!
//! 1. Names the receiver type — `Ty::PlatformObject` carries its name
//!    directly; `Ty::MetadataRef` is keyed by its
//!    [`hir_def::ty::MetadataKind::platform_prefix`] (e.g.
//!    `"InformationRegisterRecordSet"`); the legacy unparametric
//!    `Ty::Array` / `Ty::Map` / `Ty::ValueTable` map to their Russian
//!    HBK names; `Ty::Union` recurses on live members (after stripping
//!    `Undefined` / `Null`) and unions the results.
//! 2. Looks up `iter_element_types: Vec<SmolStr>` for that type — empty
//!    `Vec` means the platform did not declare iteration, so the result
//!    is `None` (the loop variable stays `Ty::Unknown`).
//! 3. Resolves each template string to a `Ty`. Composite templates
//!    (`"РегистрСведенийЗапись.<Имя регистра сведений>"`) flow through
//!    [`crate::platform_manager_lookup::map_generic_metadata_return_type`]
//!    using the receiver's MDO name; scalar templates flow through
//!    [`crate::method_lookup::lower_platform_type_name`].
//! 4. Single template → `Some(ty)`; multi-template page (e.g.
//!    `ПоляКолонкиСхемыЗапроса` lists three admissible element types)
//!    → `Some(Ty::union([…]))`.
//!
//! ## Why pass through platform data, not a hand-rolled table
//!
//! The HBK lists 304 iterable types. Encoding the pairing in Rust would
//! mean re-curating that list and drifting from the platform spec on
//! every regen. Letting the parser stamp `iter_element_types` onto each
//! `PlatformType` keeps the inference data-driven: a future HBK that
//! adds (or removes) iteration support flows through automatically.

use bsl_metadata::MdoType;
use bsl_platform::PlatformData;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::Name;
use smol_str::SmolStr;

use crate::lower::type_string::lower_platform_type_name;
use crate::platform_manager_lookup::{
    map_generic_metadata_return_type, metadata_kind_to_prefix_and_mdo,
};

/// Resolve the element type for `Для каждого … Из <collection> Цикл`.
///
/// Returns `None` when the platform syntax help did not declare the
/// receiver iterable, when a `Ty::Union` mixes iterable and
/// non-iterable arms (overinference would be wrong), or when the
/// substituted template cannot be resolved (e.g. `<Имя>`-substitution
/// without an MDO context). On success returns the element `Ty`,
/// possibly a `Ty::Union` for HBK pages that list multiple admissible
/// elements.
pub(crate) fn resolve_iter_element_ty(collection: &Ty) -> Option<Ty> {
    // Phase H Slice 3 — projected `Ty::ValueTable` short-circuits the
    // platform-template path: the row carries the same projection so
    // `Для Каждого Стр Из <ТЗ> → Стр.<column>` resolves through the
    // SDBL `SdblProjection::fields` slice. Projection-less ValueTable
    // falls through to the regular `СтрокаТаблицыЗначений` lookup.
    if let Ty::ValueTable { projection: Some(p) } = collection {
        return Some(Ty::ValueTableRow { projection: Some(p.clone()) });
    }
    let templates = match collection {
        Ty::PlatformObject(name) => lookup_by_type_name(name.as_str())?,
        Ty::MetadataRef { kind, .. } => lookup_by_metadata_kind(*kind)?,
        // Parameterised array bypasses the platform `iter_element_types`
        // table whose only declared element for `Массив` is
        // `"Произвольный"` (→ `Ty::Unknown`). The element type the
        // caller threaded through `Ty::TypedArray` is precisely the
        // information that table is missing — surface it directly.
        Ty::TypedArray(elem) => return Some((**elem).clone()),
        Ty::Array => lookup_by_type_name("Массив")?,
        Ty::Map => lookup_by_type_name("Соответствие")?,
        Ty::ValueTable { .. } => lookup_by_type_name("ТаблицаЗначений")?,
        Ty::ValueTableRow { .. } => lookup_by_type_name("СтрокаТаблицыЗначений")?,
        Ty::ValueList => lookup_by_type_name("СписокЗначений")?,
        Ty::Structure => lookup_by_type_name("Структура")?,
        Ty::Union(arms) => return resolve_union(arms.as_ref(), collection),
        _ => return None,
    };

    let context = match collection {
        Ty::MetadataRef { kind, name } => {
            metadata_kind_to_prefix_and_mdo(*kind).map(|(_, mdo)| (mdo, name.clone()))
        }
        _ => None,
    };

    resolve_templates(&templates, context.as_ref().map(|(m, n)| (*m, n)))
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
fn resolve_union(arms: &[Ty], _outer: &Ty) -> Option<Ty> {
    let alive: Vec<&Ty> = arms.iter().filter(|t| !matches!(t, Ty::Undefined | Ty::Null)).collect();
    if alive.is_empty() {
        return None;
    }
    let resolved: Option<Vec<Ty>> = alive.iter().map(|t| resolve_iter_element_ty(t)).collect();
    let resolved = resolved?;
    if resolved.len() == 1 {
        Some(resolved.into_iter().next().unwrap())
    } else {
        Some(Ty::union(resolved))
    }
}

/// Resolve a list of element templates, threading the receiver's
/// `(MdoType, Name)` context for `<Имя>` substitution where present.
fn resolve_templates(templates: &[SmolStr], context: Option<(MdoType, &Name)>) -> Option<Ty> {
    let resolved: Vec<Ty> =
        templates.iter().filter_map(|tpl| resolve_one_template(tpl, context)).collect();
    match resolved.len() {
        0 => None,
        1 => Some(resolved.into_iter().next().unwrap()),
        _ => Some(Ty::union(resolved)),
    }
}

/// Resolve one element template (`"Произвольный"`, `"СтрокаТаблицыЗначений"`,
/// `"РегистрСведенийЗапись.<Имя регистра сведений>"`, …).
fn resolve_one_template(template: &str, context: Option<(MdoType, &Name)>) -> Option<Ty> {
    // Composite shape: head before `.` is the generic kind name, the
    // tail (literal `<Имя …>`) is replaced by the receiver's mdo_name.
    if let Some(dot_pos) = template.find('.') {
        let head = &template[..dot_pos];
        let (mdo, mdo_name) = context?;
        return map_generic_metadata_return_type(head, mdo, mdo_name);
    }

    // Scalar shape: pass straight through the platform-name resolver
    // (handles `Произвольный` → `Ty::Unknown` and the
    // `Ty::PlatformObject` fallback for everything else).
    Some(lower_platform_type_name(template))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir_def::ty::MetadataKind;

    #[test]
    fn array_yields_unknown_per_platform_arbitrary() {
        // Per HBK, `Массив` iterates `Произвольный`, which lowers to
        // `Ty::Unknown` via `lower_platform_type_name`.
        let elem = resolve_iter_element_ty(&Ty::Array);
        assert_eq!(elem, Some(Ty::Unknown));
    }

    #[test]
    fn map_yields_kluch_i_znachenie() {
        // `Соответствие` iterates `КлючИЗначение`.
        let elem = resolve_iter_element_ty(&Ty::Map);
        assert_eq!(elem, Some(Ty::PlatformObject(Name::new("КлючИЗначение"))));
    }

    #[test]
    fn value_table_yields_row() {
        // `ТаблицаЗначений` iterates `СтрокаТаблицыЗначений`.
        let elem = resolve_iter_element_ty(&Ty::ValueTable { projection: None });
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
        let elem = resolve_iter_element_ty(&receiver);
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
        let elem = resolve_iter_element_ty(&receiver);
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
        let elem = resolve_iter_element_ty(&Ty::TypedArray(Box::new(Ty::String)));
        assert_eq!(elem, Some(Ty::String));
    }

    #[test]
    fn union_of_array_and_typed_array_iterates_to_union_with_unknown() {
        // Pre-Phase-5 guard: a `Union(Ty::Array, Ty::TypedArray(String))`
        // collection iterates to `Union(Ty::Unknown, Ty::String)`.
        // `Ty::Array` resolves through the platform `iter_element_types`
        // table (`["Произвольный"]` → `Ty::Unknown`); `Ty::TypedArray`
        // surfaces its element directly. The smart constructor unions
        // both and keeps `Unknown` because losing a member would erase
        // the warning that one arm has no element witness. Phase 5
        // (row-typed arrays) must keep this conservative.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::TypedArray(Box::new(Ty::String))];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve_iter_element_ty(&union);
        assert_eq!(elem, Some(Ty::union(vec![Ty::Unknown, Ty::String])));
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
        let elem = resolve_iter_element_ty(&collection);
        assert_eq!(elem, Some(row));
    }

    #[test]
    fn non_iterable_string_returns_none() {
        // Plain strings have no `Элементы коллекции:` chapter.
        let elem = resolve_iter_element_ty(&Ty::String);
        assert_eq!(elem, None);
    }

    #[test]
    fn union_with_undefined_filters_dead_arm() {
        // `Массив | Undefined` is iterable on its live arm; the
        // `Undefined` is dead for iteration. Element stays `Ty::Unknown`
        // (Произвольный) — the live arm's element.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::Undefined];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve_iter_element_ty(&union);
        assert_eq!(elem, Some(Ty::Unknown));
    }

    #[test]
    fn union_with_non_iterable_arm_returns_none() {
        // `Массив | Строка` mixes iterable and non-iterable arms —
        // overinference would be wrong, so the answer is `None`.
        let arms: Vec<Ty> = vec![Ty::Array, Ty::String];
        let union = Ty::Union(std::sync::Arc::from(arms.into_boxed_slice()));
        let elem = resolve_iter_element_ty(&union);
        assert_eq!(elem, None);
    }
}
