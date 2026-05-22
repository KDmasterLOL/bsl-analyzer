//! Field lookup on a typed receiver.
//!
//! `lookup_field` answers the question "given `x: receiver_ty`, what does
//! `x.field_name` evaluate to?".
//!
//! It is now a thin name filter over [`crate::field_enum::enumerate_fields`]:
//! the enumeration logic (MDO attribute walk, tabular sections, register
//! parts, `НомерСтроки` fall-through) lives in `field_enum` so that both
//! `lookup_field` (point lookup) and `hir::Type::fields()` (full list for
//! IDE completion) stay in sync without duplicating any traversal.
//!
//! # Coverage
//!
//! See [`crate::field_enum`] for the complete coverage map.
//!
//! # Platform fall-through
//!
//! Receivers that are not `Ty::MetadataRef` (after `ThisObject` coercion) are
//! routed through [`crate::platform_property_lookup::lookup_platform_property`].
//! These are `Ty::PlatformObject`, collection variants, etc. — they never go
//! through the enumerator.

pub use crate::field_enum::FieldInfo;

use hir_def::configs::VisibleConfig;
use hir_def::ty::{FormDataKind, MetadataKind, Ty};
use hir_def::Name;

/// Project a [`Ty::FormData`] receiver to the underlying MDO for **field**
/// resolution.
///
/// Fields on a managed-form attribute that wraps an `*Object` MDO behave
/// like the MDO's attributes — `Объект.Дата` inside a document form must
/// reach the document's `Дата` declaration, not the form-data wrapper's
/// platform members. Methods are routed differently (see
/// [`crate::method_lookup::platform_type_key`]); that's why the projection
/// lives here, in `field_lookup`, not inside a global coercion.
///
/// Returns:
/// - `Some(MetadataRef { *Object, name })` for `Structure` /
///   `StructureWithCollection` whose `underlying` MDO has an `*Object`
///   companion in [`MetadataKind::object_kind_for`].
/// - `None` for `Collection` (no name-keyed fields), for `Structure`s with
///   no `underlying` (e.g. `ValueTable` attribute without an MDO), and for
///   underlying MDO kinds without an `*Object` surface.
pub(crate) fn project_form_data_for_fields(ty: &Ty) -> Option<Ty> {
    let Ty::FormData { kind, underlying: Some((mdo, name)) } = ty else {
        return None;
    };
    if !matches!(kind, FormDataKind::Structure | FormDataKind::StructureWithCollection) {
        return None;
    }
    let object_kind = MetadataKind::object_kind_for(*mdo)?;
    Some(Ty::MetadataRef { kind: object_kind, name: name.clone() })
}

fn lookup_form_data_tabular_section_field(
    configs: &[VisibleConfig],
    receiver_ty: &Ty,
    field_name: &Name,
) -> Option<FieldInfo> {
    let Ty::FormData { kind, underlying: Some((mdo_type, mdo_name)) } = receiver_ty else {
        return None;
    };
    if !matches!(kind, FormDataKind::Structure | FormDataKind::StructureWithCollection) {
        return None;
    }

    let needle = field_name.as_str().to_lowercase();
    for cfg in configs.iter().rev() {
        let Some(mdo) = cfg.configuration.find_metadata_object(*mdo_type, mdo_name.as_str()) else {
            continue;
        };
        let Some(ts) = mdo.tabular_sections.iter().find(|ts| {
            ts.name().to_lowercase() == needle
                || ts.name_en().is_some_and(|en| en.to_lowercase() == needle)
        }) else {
            continue;
        };

        let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
        return Some(FieldInfo {
            name: Name::new(ts.name()),
            name_en: ts.name_en().filter(|s| !s.is_empty()).map(Name::new),
            ty: Ty::FormData {
                kind: FormDataKind::Collection,
                underlying: Some((*mdo_type, qualified)),
            },
            value_ty: None,
            is_readonly: false,
            origin: crate::field_enum::FieldOrigin::TabularSection,
        });
    }

    None
}

/// Resolve a field access on a typed receiver.
///
/// Returns `None` when:
/// - the receiver type has no backing MDO (`Ty::Number`, `Ty::Unknown`,
///   managers, collectives, platform objects);
/// - the MDO exists but does not declare the requested attribute or
///   tabular section;
/// - the `Ty::MetadataRef` points at an MDO kind whose field lookup is
///   deferred (registers that haven't been populated yet).
///
/// `configs` should be the visible configurations for the receiver's file
/// (`db.configurations(file_id)`).
pub fn lookup_field(
    configs: &[VisibleConfig],
    receiver_ty: &Ty,
    field_name: &Name,
) -> Option<FieldInfo> {
    // SDBL projection branch — when `Ty::QueryResultSelection` carries
    // a resolved projection (Phase 1.3b+ synthesises it; Slice 1 only
    // ever passes `None` here), field lookup consults the projection's
    // `(Name, Ty)` table BEFORE falling through to the platform
    // `ВыборкаИзРезультатаЗапроса` surface. A miss in the projection
    // does NOT short-circuit — it falls through so platform properties
    // like `.НомерСтроки` / `.СледующаяСтрока` still resolve.
    if let Some(info) = lookup_field_in_query_projection(receiver_ty, field_name) {
        return Some(info);
    }

    if let Some(info) = lookup_form_data_tabular_section_field(configs, receiver_ty, field_name) {
        return Some(info);
    }

    // Managed-form attribute projection happens BEFORE `ThisObject`
    // coercion: a `Ty::FormData { Structure | StructureWithCollection,
    // underlying: Some((mdo, n)) }` walks the MDO's attribute table for
    // field lookup (`Объект.Дата` must reach the document's `Дата`).
    // Method lookup deliberately doesn't do this — see
    // [`Ty::FormData`] docs.
    let projected_form_data = project_form_data_for_fields(receiver_ty);
    let projected_ty = projected_form_data.as_ref().unwrap_or(receiver_ty);

    // `Ty::ThisObject` / `Ty::ThisManager` coercion and dispatch split:
    //   - `MetadataRef { *Object, .. }` (from `ThisObject`) → enumerator below;
    //   - `ObjectManager { kind, name }` (from `ThisManager`) → no
    //     attribute table here, falls through to the platform-property
    //     adapter (which itself returns `None` for `ObjectManager` —
    //     `Ty::platform_type_name` is `None` for managers). This matches
    //     the pre-Step-J behaviour of `Документы.ПКО.SomeField`: managers
    //     don't expose attributes through the field-lookup channel. The
    //     manager-flavoured "field" surface (predefined items reached as
    //     `Справочники.Mgr.PredefinedName`) lives on `ManagerCollection`
    //     indexing, not here. Adding predefined-item resolution under
    //     `ObjectManager` is a separate enhancement (out of Step J's
    //     scope — Step J only wires `ЭтотОбъект` provenance through to
    //     the same dispatch shape qualified manager access already had).
    //   - Union → intersection over live arms (handled below);
    //   - everything else → platform-property adapter.
    let coerced = crate::this_object::coerce_to_metadata_ref(projected_ty);
    let effective_ty = coerced.as_ref().unwrap_or(projected_ty);

    if let Ty::Union(arms) = effective_ty {
        return lookup_field_in_union_intersection(configs, arms, field_name);
    }
    if matches!(effective_ty, Ty::MetadataRef { .. }) {
        return lookup_field_on_metadata_ref(configs, effective_ty, field_name);
    }
    // Phase 5 row-aware refinement on `Ty::FormControl{Table, Some(b)}`:
    // `.ВыделенныеСтроки` / `.ТекущаяСтрока` / `.ТекущиеДанные` and
    // their English aliases override the platform's bare `Массив`
    // (or row-shaped) property types with `TypedArray(row)` / `row`.
    // Non-refined properties (`.Видимость`, `.Заголовок`,
    // `.УсловноеОформление`, …) fall through to the platform-property
    // adapter below, which resolves them through `ТаблицаФормы` via
    // [`Ty::platform_type_name`]. Both run on the ORIGINAL receiver
    // (not `effective_ty`) — FormControl never gets FormData projection
    // or ThisObject coercion applied.
    if let Some(refined) = crate::form_items::refine_form_control_property(receiver_ty, field_name)
    {
        return Some(refined);
    }
    lookup_field_via_platform_property(receiver_ty, field_name)
}

/// Resolve a field on a [`Ty::Union`] receiver via **intersection** over
/// live arms.
///
/// Contract — distinct from [`crate::method_lookup::union_lookup`] which
/// uses first-hit semantics:
///
/// - `Undefined` / `Null` arms are stripped (consistent with
///   [`crate::field_enum::enumerate_fields`] and method lookup).
/// - If **any** live arm is missing the field, the entire lookup returns
///   `None`. Safe semantic for `x: A | B`: the field is reachable only
///   if it exists on every possible runtime shape.
/// - Single live arm short-circuits via a recursive `lookup_field` call
///   so the arm's full dispatch pipeline (FormData projection,
///   ThisObject coercion, FormControl refinement, …) runs again — the
///   helper deliberately does not bypass the orchestrator for the
///   degenerate one-arm case.
/// - Multi-arm result merges per-arm `Ty` via [`Ty::union`]; the first
///   arm's `name`/`name_en`/`origin` win (cohesion: the surface text
///   stays stable). `is_readonly` is the **disjunction** —
///   writeable iff EVERY arm is writeable, i.e. a single read-only arm
///   makes the union read-only.
fn lookup_field_in_union_intersection(
    configs: &[VisibleConfig],
    arms: &[Ty],
    field_name: &Name,
) -> Option<FieldInfo> {
    let live: Vec<&Ty> = arms.iter().filter(|t| !matches!(t, Ty::Undefined | Ty::Null)).collect();
    if live.is_empty() {
        return None;
    }
    if live.len() == 1 {
        return lookup_field(configs, live[0], field_name);
    }
    let mut per_arm: Vec<FieldInfo> = Vec::with_capacity(live.len());
    for arm in &live {
        let info = lookup_field(configs, arm, field_name)?;
        per_arm.push(info);
    }
    let first = &per_arm[0];
    let merged_ty = Ty::union(per_arm.iter().map(|f| f.ty.clone()).collect());
    let merged_readonly = per_arm.iter().any(|f| f.is_readonly);
    Some(FieldInfo {
        name: first.name.clone(),
        name_en: first.name_en.clone(),
        ty: merged_ty,
        value_ty: None,
        is_readonly: merged_readonly,
        origin: first.origin,
    })
}

/// Resolve a field on a [`Ty::MetadataRef`] receiver via bilingual
/// case-insensitive match over [`crate::field_enum::enumerate_fields`].
///
/// The enumerator is the source of truth for "what fields does an MDO
/// receiver expose?" (standard + user attributes, tabular sections,
/// register parts, `НомерСтроки` fall-through). This helper is the
/// thin filter that turns the enumeration into a point lookup.
///
/// Caller is expected to pre-coerce `ThisObject` / project `FormData`
/// before passing the receiver — see [`lookup_field`] orchestrator.
fn lookup_field_on_metadata_ref(
    configs: &[VisibleConfig],
    effective_ty: &Ty,
    field_name: &Name,
) -> Option<FieldInfo> {
    let needle = field_name.as_str().to_lowercase();
    crate::field_enum::enumerate_fields(configs, effective_ty).into_iter().find(|f| {
        f.name.as_str().to_lowercase() == needle
            || f.name_en.as_ref().is_some_and(|en| en.as_str().to_lowercase() == needle)
    })
}

/// Resolve `<field>` on a `Ty::QueryResultSelection { projection: Some(_) }`
/// receiver against the SDBL projection table.
///
/// Returns `None` when:
/// - the receiver is not a projection-typed `QueryResultSelection`,
/// - the projection is `None` (no SDBL trace available — falls through
///   to the platform `ВыборкаИзРезультатаЗапроса` table for legacy
///   behaviour),
/// - the projection doesn't carry `field_name` (caller falls through to
///   the platform table so `НомерСтроки` / `СледующаяСтрока` /
///   `Уровень` still resolve).
///
/// Case-insensitive bilingual match: BSL field-access is case-folded
/// the same way platform method lookup is. Same posture as
/// [`lookup_field_on_metadata_ref`].
fn lookup_field_in_query_projection(receiver_ty: &Ty, field_name: &Name) -> Option<FieldInfo> {
    let Ty::QueryResultSelection { projection: Some(projection) } = receiver_ty else {
        return None;
    };
    let needle = field_name.as_str().to_lowercase();
    projection.fields.iter().find(|(n, _)| n.as_str().to_lowercase() == needle).map(|(n, ty)| {
        FieldInfo {
            name: n.clone(),
            name_en: None,
            ty: ty.clone(),
            value_ty: None,
            // SDBL projection fields are not writeable through the
            // selection cursor — `Выборка.X = ...` is a runtime error.
            is_readonly: true,
            // Projection fields surface as user-defined names (the
            // SELECT alias or column ref the user wrote). `UserAttribute`
            // origin gives them the right icon and sort band in
            // completion. A dedicated `SdblProjectionField` variant
            // can replace this once the IDE differentiates them.
            origin: crate::field_enum::FieldOrigin::UserAttribute,
        }
    })
}

/// Resolve a field on every other receiver via the platform-property
/// adapter.
///
/// `lookup_platform_property` itself decides whether the shape is
/// supported — primitives and managers return `None` — so the wrapper
/// is safe to call for any non-MetadataRef, non-Union receiver.
///
/// **Important:** takes the ORIGINAL `receiver_ty`, not the
/// projected/coerced `effective_ty`. FormData projection and
/// `ThisObject` coercion are MetadataRef-specific transforms and must
/// not leak into the platform-property channel (e.g. a `FormControl`
/// receiver must keep its `FormControl` identity through this fallback,
/// not get projected away).
fn lookup_field_via_platform_property(receiver_ty: &Ty, field_name: &Name) -> Option<FieldInfo> {
    let res = crate::platform_property_lookup::lookup_platform_property(receiver_ty, field_name)?;
    Some(FieldInfo {
        name: field_name.clone(),
        name_en: None,
        ty: res.return_ty,
        value_ty: None,
        is_readonly: res.is_readonly,
        origin: crate::field_enum::FieldOrigin::PlatformProperty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::tabular_section::{TabularSection, TabularSectionAttribute};
    use bsl_metadata::{Attribute, AttributeType, Configuration, MdoType, MetadataObject};
    use hir_def::ty::MetadataKind;
    use std::sync::Arc;
    use uuid::Uuid;

    fn wrap(config: Configuration) -> Vec<VisibleConfig> {
        vec![VisibleConfig { name: None, configuration: Arc::new(config) }]
    }

    fn attr(name: &str, name_en: Option<&str>, attr_type: AttributeType) -> Attribute {
        Attribute { name: name.to_string(), name_en: name_en.map(String::from), attr_type }
    }

    fn catalog(name: &str, attrs: Vec<Attribute>) -> MetadataObject {
        let mut mdo = MetadataObject::new(MdoType::Catalog, name);
        for a in attrs {
            mdo.add_attribute(a);
        }
        mdo
    }

    fn document(name: &str, attrs: Vec<Attribute>) -> MetadataObject {
        let mut mdo = MetadataObject::new(MdoType::Document, name);
        for a in attrs {
            mdo.add_attribute(a);
        }
        mdo
    }

    fn mdo_of(mdo_type: MdoType, name: &str, attrs: Vec<Attribute>) -> MetadataObject {
        let mut mdo = MetadataObject::new(mdo_type, name);
        for a in attrs {
            mdo.add_attribute(a);
        }
        mdo
    }

    #[test]
    fn field_lookup_mdo_attribute_exchange_plan_and_chart_of_accounts() {
        let mut config = Configuration::new("Test");
        config.add_metadata_object(mdo_of(
            MdoType::ExchangePlan,
            "Контрагенты",
            vec![attr("Признак", None, AttributeType::Boolean)],
        ));
        config.add_metadata_object(mdo_of(
            MdoType::ChartOfAccounts,
            "Хозрасчетный",
            vec![attr("Порядок", None, AttributeType::Number { precision: 15, scale: 0 })],
        ));
        let configs = wrap(config);

        let ep_info = lookup_field(
            &configs,
            &Ty::MetadataRef {
                kind: MetadataKind::ExchangePlanRef,
                name: Name::new("Контрагенты"),
            },
            &Name::new("Признак"),
        )
        .expect("ExchangePlanRef.Признак resolves");
        assert_eq!(ep_info.ty, Ty::Boolean);

        let coa_info = lookup_field(
            &configs,
            &Ty::MetadataRef {
                kind: MetadataKind::ChartOfAccountsRef,
                name: Name::new("Хозрасчетный"),
            },
            &Name::new("Порядок"),
        )
        .expect("ChartOfAccountsRef.Порядок resolves");
        assert_eq!(coa_info.ty, Ty::Number);
    }

    #[test]
    fn field_lookup_mdo_attribute_catalog() {
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog(
            "Номенклатура",
            vec![attr("Цена", None, AttributeType::Number { precision: 15, scale: 2 })],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Цена"))
            .expect("Цена resolves on Номенклатура");
        assert_eq!(info.ty, Ty::Number);
    }

    #[test]
    fn field_lookup_standard_attribute_code() {
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog(
            "Номенклатура",
            vec![attr("Код", Some("Code"), AttributeType::String { length: Some(9) })],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Код"))
            .expect("standard Code attribute resolves");
        assert_eq!(info.ty, Ty::String);

        let info_en = lookup_field(&configs, &receiver, &Name::new("Code"))
            .expect("Code (en) resolves through bilingual match");
        assert_eq!(info_en.ty, Ty::String);
    }

    #[test]
    fn field_lookup_tabular_section() {
        let mut ts = TabularSection::new(Uuid::new_v4(), "Товары");
        ts.set_attributes(vec![TabularSectionAttribute::new(
            Uuid::new_v4(),
            "Количество",
            AttributeType::Number { precision: 15, scale: 3 },
        )]);
        let mut doc = document("ПКО", vec![]);
        doc.add_tabular_section(ts);

        let mut config = Configuration::new("Test");
        config.add_metadata_object(doc);
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") };
        let info = lookup_field(&configs, &receiver, &Name::new("Товары"))
            .expect("tabular section name resolves to TabularSection Ty");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSection { parent: MdoType::Document },
                name: Name::new("ПКО.Товары"),
            }
        );
    }

    #[test]
    fn field_lookup_tabular_row_attribute() {
        let mut ts = TabularSection::new(Uuid::new_v4(), "Товары");
        ts.set_attributes(vec![TabularSectionAttribute::new(
            Uuid::new_v4(),
            "Количество",
            AttributeType::Number { precision: 15, scale: 3 },
        )]);
        let mut doc = document("ПКО", vec![]);
        doc.add_tabular_section(ts);

        let mut config = Configuration::new("Test");
        config.add_metadata_object(doc);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
            name: Name::new("ПКО.Товары"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Количество"))
            .expect("row attribute Количество resolves to Number");
        assert_eq!(info.ty, Ty::Number);
    }

    #[test]
    fn field_lookup_same_name_catalog_and_document_disambiguated_by_parent() {
        let make_ts = |attr_type: AttributeType| {
            let mut ts = TabularSection::new(Uuid::new_v4(), "Товары");
            ts.set_attributes(vec![TabularSectionAttribute::new(
                Uuid::new_v4(),
                "Количество",
                attr_type,
            )]);
            ts
        };

        let mut cat = catalog("X", vec![]);
        cat.add_tabular_section(make_ts(AttributeType::String { length: Some(10) }));
        let mut doc = document("X", vec![]);
        doc.add_tabular_section(make_ts(AttributeType::Number { precision: 15, scale: 3 }));

        let mut config = Configuration::new("Test");
        config.add_metadata_object(cat);
        config.add_metadata_object(doc);
        let configs = wrap(config);

        let cat_row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
            name: Name::new("X.Товары"),
        };
        let doc_row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
            name: Name::new("X.Товары"),
        };
        assert_eq!(
            lookup_field(&configs, &cat_row, &Name::new("Количество")).unwrap().ty,
            Ty::String,
            "Catalog row must resolve via its own tabular section",
        );
        assert_eq!(
            lookup_field(&configs, &doc_row, &Name::new("Количество")).unwrap().ty,
            Ty::Number,
            "Document row must resolve via its own tabular section — not Catalog's",
        );
    }

    #[test]
    fn field_lookup_tabular_row_line_number_resolves_via_platform() {
        let ts = TabularSection::new(Uuid::new_v4(), "Услуги");
        let mut cat = catalog("Номенклатура", vec![]);
        cat.add_tabular_section(ts);
        let mut config = Configuration::new("Test");
        config.add_metadata_object(cat);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
            name: Name::new("Номенклатура.Услуги"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("НомерСтроки"))
            .expect("НомерСтроки resolves through platform property fall-through");
        assert_eq!(info.ty, Ty::Number);
    }

    #[test]
    fn field_lookup_tabular_row_custom_attribute_wins_over_platform() {
        let mut ts = TabularSection::new(Uuid::new_v4(), "Услуги");
        ts.set_attributes(vec![TabularSectionAttribute::new(
            Uuid::new_v4(),
            "НомерСтроки",
            AttributeType::String { length: Some(36) },
        )]);
        let mut cat = catalog("Номенклатура", vec![]);
        cat.add_tabular_section(ts);
        let mut config = Configuration::new("Test");
        config.add_metadata_object(cat);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
            name: Name::new("Номенклатура.Услуги"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("НомерСтроки"))
            .expect("custom attribute named НомерСтроки must still resolve");
        assert_eq!(
            info.ty,
            Ty::String,
            "custom XML attribute must win over the platform standard row property",
        );
    }

    #[test]
    fn field_lookup_unknown_receiver_returns_none() {
        let configs = wrap(Configuration::new("Test"));
        for ty in [
            Ty::Unknown,
            Ty::Number,
            Ty::String,
            Ty::Array,
            Ty::Undefined,
            Ty::Union(vec![Ty::Number, Ty::String].into()),
        ] {
            assert!(
                lookup_field(&configs, &ty, &Name::new("Любой")).is_none(),
                "no field lookup on {ty:?}"
            );
        }
    }

    #[test]
    fn field_lookup_missing_attribute_returns_none() {
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("Номенклатура", vec![]));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        assert!(lookup_field(&configs, &receiver, &Name::new("НесуществующееПоле")).is_none());
    }

    #[test]
    fn field_lookup_register_missing_in_config_returns_none() {
        let configs = wrap(Configuration::new("Test"));
        let r = Ty::MetadataRef {
            kind: MetadataKind::AccumulationRegisterRef,
            name: Name::new("ТоварыНаСкладах"),
        };
        assert!(lookup_field(&configs, &r, &Name::new("Количество")).is_none());
    }

    fn register_with(
        name: &str,
        mdo_type: MdoType,
        dimensions: Vec<bsl_metadata::dimension::Dimension>,
        resources: Vec<bsl_metadata::register::RegisterResource>,
        attributes: Vec<bsl_metadata::register::RegisterAttribute>,
    ) -> bsl_metadata::Register {
        let mut builder = bsl_metadata::Register::builder().name(name).mdo_type(mdo_type);
        for d in dimensions {
            builder = builder.add_dimension(d);
        }
        for r in resources {
            builder = builder.add_resource(r);
        }
        for a in attributes {
            builder = builder.add_attribute(a);
        }
        builder.build()
    }

    fn dimension_typed(name: &str, attr_type: AttributeType) -> bsl_metadata::dimension::Dimension {
        let mut d = bsl_metadata::dimension::Dimension::builder().name(name).build();
        d.set_attr_type(attr_type);
        d
    }

    fn resource_typed(
        name: &str,
        attr_type: AttributeType,
    ) -> bsl_metadata::register::RegisterResource {
        let mut r = bsl_metadata::register::RegisterResource::new(Uuid::new_v4(), name);
        r.set_attr_type(attr_type);
        r
    }

    fn attribute_typed(
        name: &str,
        attr_type: AttributeType,
    ) -> bsl_metadata::register::RegisterAttribute {
        let mut a = bsl_metadata::register::RegisterAttribute::new(Uuid::new_v4(), name);
        a.set_attr_type(attr_type);
        a
    }

    #[test]
    fn field_lookup_register_dimension_typed_returns_lowered_ty() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![dimension_typed(
                "Справочник1",
                AttributeType::Ref {
                    mdo_type: MdoType::Catalog, name: "Справочник1".into()
                },
            )],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRef,
            name: Name::new("РегистрСведений1"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Справочник1"))
            .expect("dimension resolves against Configuration.registers");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
            },
            "typed dimension must lower through TyLoweringContext to a concrete MetadataRef",
        );
    }

    #[test]
    fn field_lookup_register_resource_typed_on_accumulation() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "ТоварыНаСкладах",
            MdoType::AccumulationRegister,
            vec![],
            vec![resource_typed("Количество", AttributeType::Number { precision: 15, scale: 3 })],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::AccumulationRegisterRecordSet,
            name: Name::new("ТоварыНаСкладах"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Количество"))
            .expect("resource resolves against Configuration.registers");
        assert_eq!(info.ty, Ty::Number);
    }

    #[test]
    fn field_lookup_register_attribute_typed_on_information() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![],
            vec![attribute_typed("Комментарий", AttributeType::String { length: Some(100) })],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordManager,
            name: Name::new("РегистрСведений1"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Комментарий"))
            .expect("attribute resolves against Configuration.registers");
        assert_eq!(info.ty, Ty::String);
    }

    #[test]
    fn field_lookup_register_untyped_part_returns_symbolic_fallback() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![bsl_metadata::dimension::Dimension::builder().name("Справочник1").build()],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRef,
            name: Name::new("РегистрСведений1"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Справочник1"))
            .expect("untyped dimension still resolves with symbolic fallback");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::RegisterDimension { parent: MdoType::InformationRegister },
                name: Name::new("РегистрСведений1.Справочник1"),
            },
            "fallback must carry parent flavour + `Register.Part` name for provenance",
        );
    }

    #[test]
    fn field_lookup_register_all_four_flavours_resolve() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегСвед",
            MdoType::InformationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 0 })],
            vec![],
        ));
        config.add_register(register_with(
            "РегНак",
            MdoType::AccumulationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 0 })],
            vec![],
        ));
        config.add_register(register_with(
            "РегБух",
            MdoType::AccountingRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 0 })],
            vec![],
        ));
        config.add_register(register_with(
            "РегРасч",
            MdoType::CalculationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 0 })],
            vec![],
        ));
        let configs = wrap(config);

        let cases = [
            (MetadataKind::InformationRegisterRef, "РегСвед"),
            (MetadataKind::AccumulationRegisterRef, "РегНак"),
            (MetadataKind::AccountingRegisterRef, "РегБух"),
            (MetadataKind::CalculationRegisterRef, "РегРасч"),
        ];
        for (kind, name) in cases {
            let receiver = Ty::MetadataRef { kind, name: Name::new(name) };
            let info = lookup_field(&configs, &receiver, &Name::new("R"))
                .unwrap_or_else(|| panic!("resource R must resolve on {kind:?}/{name}"));
            assert_eq!(info.ty, Ty::Number, "{kind:?}/{name}.R must lower to Ty::Number");
        }
    }

    #[test]
    fn field_lookup_register_leaf_parts_have_no_field_surface() {
        let configs = wrap(Configuration::new("Test"));
        for kind in [
            MetadataKind::RegisterDimension { parent: MdoType::InformationRegister },
            MetadataKind::RegisterResource { parent: MdoType::AccumulationRegister },
            MetadataKind::RegisterAttribute { parent: MdoType::CalculationRegister },
        ] {
            let receiver = Ty::MetadataRef {
                kind,
                name: Name::new("РегистрСведений1.Справочник1"),
            };
            assert!(
                lookup_field(&configs, &receiver, &Name::new("ЛюбоеПоле")).is_none(),
                "leaf part kind {kind:?} must not expose a field surface",
            );
        }
    }

    #[test]
    fn field_lookup_register_wrong_flavour_returns_none() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "X",
            MdoType::InformationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 0 })],
            vec![],
        ));
        let configs = wrap(config);

        let wrong_flavour_receiver =
            Ty::MetadataRef { kind: MetadataKind::AccumulationRegisterRef, name: Name::new("X") };
        assert!(
            lookup_field(&configs, &wrong_flavour_receiver, &Name::new("R")).is_none(),
            "AccumulationRegisterRef must not resolve against an InformationRegister even with the same name",
        );
    }

    #[test]
    fn field_lookup_information_register_record_set_synthesizes_filter() {
        // `<recordSet>.Отбор` must resolve to the synthetic
        // `RegisterFilter` receiver pinned to the parent register
        // flavour. The HBK has no `Отбор` property on RecordSet rows;
        // synthesis lives in `enumerate_register_fields`.
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "Курсы",
            MdoType::InformationRegister,
            vec![dimension_typed(
                "Валюта",
                AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".into() },
            )],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("Курсы"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Отбор"))
            .expect("synthetic .Отбор must resolve on InformationRegisterRecordSet");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
                name: Name::new("Курсы"),
            },
        );

        // English alias must also work (бilingual contract).
        let info_en = lookup_field(&configs, &receiver, &Name::new("Filter"))
            .expect("English alias `.Filter` must resolve too");
        assert_eq!(info_en.ty, info.ty);
    }

    #[test]
    fn field_lookup_register_filter_dimension_resolves_as_filter_item() {
        // Inside the synthetic Filter, each register dimension is a
        // member typed as `Ty::PlatformObject("ЭлементОтбора")` so
        // platform `FilterItem` methods (`Установить`, …) can apply.
        // Resources / attributes are intentionally excluded.
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "Курсы",
            MdoType::InformationRegister,
            vec![dimension_typed(
                "Валюта",
                AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".into() },
            )],
            vec![resource_typed("Курс", AttributeType::Number { precision: 15, scale: 4 })],
            vec![],
        ));
        let configs = wrap(config);

        let filter_receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("Курсы"),
        };
        let dim_info = lookup_field(&configs, &filter_receiver, &Name::new("Валюта"))
            .expect("dimension Валюта resolves through the synthetic Filter receiver");
        assert_eq!(
            dim_info.ty,
            Ty::PlatformObject(Name::new("ЭлементОтбора")),
            "Filter members must lower to platform `ЭлементОтбора` so FilterItem methods apply",
        );

        // Resources are NOT exposed as Filter members (only dimensions).
        assert!(
            lookup_field(&configs, &filter_receiver, &Name::new("Курс")).is_none(),
            "resources must not appear as Filter members",
        );
    }

    #[test]
    fn field_lookup_register_filter_dim_named_otbor_loses_to_synthetic() {
        // Collision contract: a register dimension named `Отбор` must
        // NOT shadow the synthetic `.Отбор` Filter property — 1С
        // semantics give the platform property priority. The dimension
        // remains reachable as `<recordSet>.Отбор.Отбор` (through the
        // Filter member surface).
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегСвед",
            MdoType::InformationRegister,
            vec![dimension_typed("Отбор", AttributeType::String { length: Some(50) })],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("РегСвед"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Отбор"))
            .expect("synthetic .Отбор must win over a same-named dimension");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
                name: Name::new("РегСвед"),
            },
            "synthetic Filter wins over a register dimension named `Отбор`",
        );

        // Dimension stays reachable through the Filter receiver.
        let filter_receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("РегСвед"),
        };
        let dim_via_filter = lookup_field(&configs, &filter_receiver, &Name::new("Отбор"))
            .expect("dimension stays reachable as <recordSet>.Отбор.Отбор");
        assert_eq!(dim_via_filter.ty, Ty::PlatformObject(Name::new("ЭлементОтбора")));
    }

    #[test]
    fn field_lookup_register_filter_unknown_dimension_returns_none() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегСвед",
            MdoType::InformationRegister,
            vec![dimension_typed("Валюта", AttributeType::String { length: Some(3) })],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let filter_receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("РегСвед"),
        };
        assert!(
            lookup_field(&configs, &filter_receiver, &Name::new("НетТакогоИзмерения")).is_none(),
            "unknown dimension on Filter receiver must return None",
        );
    }

    #[test]
    fn field_lookup_information_register_record_set_pulls_platform_properties() {
        // After the html-parser fix lets composite-type properties land
        // in `platform_data.json`, the field enumerator must surface them
        // on every register record-set receiver. Pin three of the seven
        // properties HBK declares for `InformationRegisterRecordSet.<Имя>`:
        // - `Записывать` / `Write` (Boolean)
        // - `ДополнительныеСвойства` / `AdditionalProperties` (Структура)
        // - `ЗаписьИсторииДанных` / `WriteDataHistory` (Boolean)
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "Курсы",
            MdoType::InformationRegister,
            vec![dimension_typed("Валюта", AttributeType::String { length: Some(3) })],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecordSet,
            name: Name::new("Курсы"),
        };
        for prop in ["Записывать", "ДополнительныеСвойства", "ЗаписьИсторииДанных"]
        {
            assert!(
                lookup_field(&configs, &receiver, &Name::new(prop)).is_some(),
                "platform property `{prop}` must surface on InformationRegisterRecordSet",
            );
        }
        // Bilingual contract: English alias resolves too.
        assert!(
            lookup_field(&configs, &receiver, &Name::new("Write")).is_some(),
            "English alias `Write` must resolve via bilingual rsplit on english_name",
        );

        // Accounting-flavour-only property (`БлокироватьДляИзменения` /
        // `LockForUpdate`) must NOT appear on InformationRegister, but
        // MUST appear on AccountingRegisterRecordSet — proves per-flavour
        // platform-prefix routing.
        assert!(
            lookup_field(&configs, &receiver, &Name::new("БлокироватьДляИзменения")).is_none(),
            "Accounting-only property must not leak into InformationRegister surface",
        );
    }

    #[test]
    fn field_lookup_accounting_register_record_set_has_lock_for_update() {
        // Cross-flavour pin: AccountingRegisterRecordSet's HBK page declares
        // `БлокироватьДляИзменения` / `LockForUpdate`, which is absent on
        // every other record-set flavour. The platform-prefix routing in
        // `enumerate_register_fields` must thread the right prefix per
        // receiver kind so these flavour-specific properties surface.
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "Хозрасчетный",
            MdoType::AccountingRegister,
            vec![],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::AccountingRegisterRecordSet,
            name: Name::new("Хозрасчетный"),
        };
        assert!(
            lookup_field(&configs, &receiver, &Name::new("БлокироватьДляИзменения")).is_some(),
            "AccountingRegisterRecordSet must expose БлокироватьДляИзменения from HBK",
        );
        assert!(
            lookup_field(&configs, &receiver, &Name::new("LockForUpdate")).is_some(),
            "English alias LockForUpdate must resolve via bilingual rsplit",
        );
    }

    #[test]
    fn field_lookup_register_filter_synthesized_for_all_record_set_flavours() {
        // The synthetic `.Отбор` push must fire for every register
        // record-set kind we declare today (Information / Accumulation
        // / Accounting / Calculation), with the parent flavour
        // threaded into RegisterFilter so dimension lookup hits the
        // right register.
        let mut config = Configuration::new("Test");
        for (name, mdo_type) in [
            ("РегСвед", MdoType::InformationRegister),
            ("РегНак", MdoType::AccumulationRegister),
            ("РегБух", MdoType::AccountingRegister),
            ("РегРасч", MdoType::CalculationRegister),
        ] {
            config.add_register(register_with(
                name,
                mdo_type,
                vec![dimension_typed("Дим", AttributeType::String { length: Some(10) })],
                vec![],
                vec![],
            ));
        }
        let configs = wrap(config);

        let cases = [
            (MetadataKind::InformationRegisterRecordSet, "РегСвед", MdoType::InformationRegister),
            (MetadataKind::AccumulationRegisterRecordSet, "РегНак", MdoType::AccumulationRegister),
            (MetadataKind::AccountingRegisterRecordSet, "РегБух", MdoType::AccountingRegister),
            (MetadataKind::CalculationRegisterRecordSet, "РегРасч", MdoType::CalculationRegister),
        ];
        for (kind, name, parent) in cases {
            let receiver = Ty::MetadataRef { kind, name: Name::new(name) };
            let info = lookup_field(&configs, &receiver, &Name::new("Отбор"))
                .unwrap_or_else(|| panic!("{kind:?}/{name}: synthetic .Отбор must resolve"));
            assert_eq!(
                info.ty,
                Ty::MetadataRef {
                    kind: MetadataKind::RegisterFilter { parent },
                    name: Name::new(name),
                },
                "{kind:?}/{name}: RegisterFilter parent must match register flavour",
            );
        }
    }

    #[test]
    fn field_lookup_extension_wins_on_collision() {
        let mut main = Configuration::new("Main");
        main.add_metadata_object(catalog(
            "Номенклатура",
            vec![attr("Цена", None, AttributeType::Number { precision: 15, scale: 2 })],
        ));
        let mut ext = Configuration::new("Ext");
        ext.add_metadata_object(catalog(
            "Номенклатура",
            vec![attr("Цена", None, AttributeType::String { length: Some(64) })],
        ));
        let configs = vec![
            VisibleConfig { name: None, configuration: Arc::new(main) },
            VisibleConfig { name: Some("Ext".into()), configuration: Arc::new(ext) },
        ];

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Цена"))
            .expect("Цена resolves via extension override");
        assert_eq!(info.ty, Ty::String, "extension type wins over main config");
    }

    #[test]
    fn field_lookup_register_extension_wins_on_collision() {
        let mut main = Configuration::new("Main");
        main.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::Number { precision: 15, scale: 2 })],
            vec![],
        ));
        let mut ext = Configuration::new("Ext");
        ext.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![resource_typed("R", AttributeType::String { length: Some(64) })],
            vec![],
        ));
        let configs = vec![
            VisibleConfig { name: None, configuration: Arc::new(main) },
            VisibleConfig { name: Some("Ext".into()), configuration: Arc::new(ext) },
        ];

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRef,
            name: Name::new("РегистрСведений1"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("R"))
            .expect("R resolves via extension override");
        assert_eq!(info.ty, Ty::String, "extension register type wins over main config");
    }

    #[test]
    fn split_parent_section_rejects_malformed() {
        use crate::field_enum::split_parent_section;
        assert_eq!(split_parent_section("ПКО.Товары"), Some(("ПКО", "Товары")));
        assert_eq!(split_parent_section("ПКО"), None);
        assert_eq!(split_parent_section(""), None);
        assert_eq!(split_parent_section("."), None);
        assert_eq!(split_parent_section("ПКО."), None);
        assert_eq!(split_parent_section(".Товары"), None);
    }

    #[test]
    fn lookup_field_on_union_with_undefined_resolves_to_arm() {
        // `НайтиСтроки → Union(TabularSectionRow, Undefined)` — `Undefined`
        // is filtered out, the single live arm is consulted directly so the
        // semantic type stays sharp.
        let mut ts = TabularSection::new(Uuid::new_v4(), "Товары");
        ts.set_attributes(vec![TabularSectionAttribute::new(
            Uuid::new_v4(),
            "Номенклатура",
            AttributeType::Ref {
                mdo_type: MdoType::Catalog, name: "Номенклатура".into()
            },
        )]);
        let mut doc = MetadataObject::new(MdoType::Document, "ПКО");
        doc.add_tabular_section(ts);
        let mut config = Configuration::new("Test");
        config.add_metadata_object(doc);
        let configs = wrap(config);

        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
            name: Name::new("ПКО.Товары"),
        };
        let receiver = Ty::Union(vec![row, Ty::Undefined].into());
        let info = lookup_field(&configs, &receiver, &Name::new("Номенклатура"))
            .expect("union arm column must resolve");
        assert!(matches!(info.ty, Ty::MetadataRef { .. }), "Номенклатура is a Ref");
    }

    #[test]
    fn lookup_field_on_union_intersection_requires_field_in_every_arm() {
        // Two distinct catalogs sharing the standard attribute `Код` and
        // each carrying a unique custom attribute. Receiver is
        // `Union(CatalogRef.A, CatalogRef.B)`:
        //   - `Код` exists in both → resolves.
        //   - `OnlyInA` exists only in A → must NOT resolve (intersection).
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog(
            "A",
            vec![
                attr(
                    "Ссылка",
                    Some("Ref"),
                    AttributeType::Ref { mdo_type: MdoType::Catalog, name: "A".into() },
                ),
                attr("Код", Some("Code"), AttributeType::String { length: Some(9) }),
                attr("OnlyInA", None, AttributeType::Boolean),
            ],
        ));
        config.add_metadata_object(catalog(
            "B",
            vec![
                attr(
                    "Ссылка",
                    Some("Ref"),
                    AttributeType::Ref { mdo_type: MdoType::Catalog, name: "B".into() },
                ),
                attr("Код", Some("Code"), AttributeType::String { length: Some(11) }),
            ],
        ));
        let configs = wrap(config);

        let a = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("A") };
        let b = Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("B") };
        let receiver = Ty::Union(vec![a, b].into());

        let common = lookup_field(&configs, &receiver, &Name::new("Код"))
            .expect("Код is in both arms — intersection succeeds");
        assert_eq!(common.ty, Ty::String, "merged type collapses identical String arms");

        let only_a = lookup_field(&configs, &receiver, &Name::new("OnlyInA"));
        assert!(only_a.is_none(), "field absent in B must not resolve under union");
    }

    #[test]
    fn lookup_field_on_union_intersection_readonly_merges_via_or() {
        // OR-merge semantic of `is_readonly` in union intersection
        // (`lookup_field_in_union_intersection` cohesion rule).
        //
        // Construction:
        //   - arm A = `CatalogObject.A` without a user
        //     `ДополнительныеСвойства` → HBK platform-property cascade
        //     surfaces the field as `is_readonly = true`.
        //   - arm B = `CatalogObject.B` WITH a user attribute named
        //     `ДополнительныеСвойства` (not a recognised standard, so
        //     `classify_attr` returns `None` → `UserAttribute, false`).
        //     `push_unique` blocks the HBK fall-through behind the
        //     user version, leaving `is_readonly = false`.
        //
        // Union merge MUST be `true` (writeable iff EVERY arm writeable —
        // a single read-only arm makes the union read-only). If the
        // merge ever flips to AND, this assertion fires.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("A", vec![]));
        config.add_metadata_object(catalog(
            "B",
            vec![attr("ДополнительныеСвойства", None, AttributeType::Boolean)],
        ));
        let configs = wrap(config);

        let a = Ty::MetadataRef { kind: MetadataKind::CatalogObject, name: Name::new("A") };
        let b = Ty::MetadataRef { kind: MetadataKind::CatalogObject, name: Name::new("B") };
        let receiver = Ty::Union(vec![a, b].into());

        let info = lookup_field(&configs, &receiver, &Name::new("ДополнительныеСвойства"))
            .expect("ДополнительныеСвойства is in both arms — intersection succeeds");
        assert!(
            info.is_readonly,
            "OR-merge: arm A is read-only (HBK), so union read-only regardless of arm B"
        );
    }

    // -------------------------------------------------------------
    // Phase A: HBK platform-property cascade on *Object / *Ref MDO
    // receivers. Symmetric with
    // `field_lookup_information_register_record_set_pulls_platform_properties`
    // and pinned per `crates/hir-def/src/ty.rs::MetadataKind::platform_prefix`.
    // -------------------------------------------------------------

    #[test]
    fn field_lookup_document_object_pulls_platform_properties() {
        // HBK lists 12 properties on `DocumentObject.<Имя>`. Pin the seven
        // that were invisible before the cascade — they cannot come from
        // `standard_mdo_attributes::DOCUMENT_OBJECT` (which only specs
        // Ref/DeletionMark/Number/Date/Posted) and therefore exercise the
        // new `push_platform_prefix_properties` path.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(document("ПКО", vec![]));
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") };

        let dop = lookup_field(&configs, &receiver, &Name::new("ДополнительныеСвойства"))
            .expect("ДополнительныеСвойства must surface on DocumentObject via HBK");
        assert_eq!(dop.ty, Ty::Structure, "HBK declares Структура");
        assert!(dop.is_readonly, "DocumentObject.ДополнительныеСвойства is read-only per HBK");

        for prop in [
            "Движения",
            "ОбменДанными",
            "ВерсияДанных",
            "ЗаписьИсторииДанных",
            "ПринадлежностьПоследовательностям",
            "ЭтотОбъект",
        ] {
            assert!(
                lookup_field(&configs, &receiver, &Name::new(prop)).is_some(),
                "platform property `{prop}` must surface on DocumentObject",
            );
        }

        // Bilingual: English alias resolves via `rsplit('.').next()` on
        // the composite `english_name` shape.
        assert!(
            lookup_field(&configs, &receiver, &Name::new("AdditionalProperties")).is_some(),
            "English alias AdditionalProperties must resolve through bilingual rsplit",
        );
    }

    #[test]
    fn field_lookup_this_object_is_typed_metadata_ref_not_generic_platform_object() {
        // HBK declares `DocumentObject.<Имя>.ЭтотОбъект` with property type
        // `ДокументОбъект` (the base name, NOT the composite). Without
        // specialization the cascade would return
        // `Ty::PlatformObject("ДокументОбъект")` — chain typing through
        // `<Doc>.ЭтотОбъект.Записать()` would then break because the
        // receiver type lost its MDO anchor.
        //
        // `specialize_self_ref_ty` promotes the self-base name to a
        // concrete `Ty::MetadataRef` pinned to this receiver's `mdo_name`.
        // Pinned both for Document (Object self) and Catalog (Object self),
        // and for `Ссылка` cascade-side typing (Ref of self).
        let mut config = Configuration::new("Test");
        config.add_metadata_object(document("ПКО", vec![]));
        config.add_metadata_object(catalog("Номенклатура", vec![]));
        let configs = wrap(config);

        let doc = Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") };
        let this_obj = lookup_field(&configs, &doc, &Name::new("ЭтотОбъект"))
            .expect("ЭтотОбъект resolves on DocumentObject");
        assert_eq!(
            this_obj.ty,
            Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") },
            "ЭтотОбъект must be a typed MetadataRef (self), not a generic PlatformObject",
        );

        let cat = Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };
        let cat_this_obj = lookup_field(&configs, &cat, &Name::new("ЭтотОбъект"))
            .expect("ЭтотОбъект resolves on CatalogObject");
        assert_eq!(
            cat_this_obj.ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogObject, name: Name::new("Номенклатура")
            },
            "CatalogObject.ЭтотОбъект must specialize to typed self",
        );

        // `Ссылка` on Object: HBK type `ДокументСсылка` → specialize to
        // MetadataRef{DocumentRef, ПКО}. (Note: when xml_parser
        // materialises `Ссылка` into mdo.attributes, that wins over
        // cascade. Here `vec![]` means cascade is the source.)
        let r#ref = lookup_field(&configs, &doc, &Name::new("Ссылка"))
            .expect("Ссылка resolves on DocumentObject via cascade");
        assert_eq!(
            r#ref.ty,
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") },
            "Ссылка must specialize to typed DocumentRef self",
        );
    }

    #[test]
    fn field_lookup_this_object_specializes_across_yo_spelling_difference() {
        // HBK ships `ReportObject.<Имя>.ЭтотОбъект` with declared type
        // `ОтчетОбъект` (no `ё`), while `MetadataKind::display_label`
        // returns `ОтчётОбъект` (with `ё`). Exact-string comparison
        // would silently fail and leave `ЭтотОбъект` as a generic
        // `Ty::PlatformObject`. `eq_yo_insensitive` in
        // `specialize_self_ref_ty` folds the spellings so the
        // specialization fires.
        //
        // Also pin DataProcessor (no ё mismatch — `ОбработкаОбъект`
        // matches both sides directly) to ensure the family coverage
        // is symmetric.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(mdo_of(MdoType::Report, "ОстаткиТоваров", vec![]));
        config.add_metadata_object(mdo_of(MdoType::DataProcessor, "Обработка1", vec![]));
        let configs = wrap(config);

        let report = Ty::MetadataRef {
            kind: MetadataKind::ReportObject,
            name: Name::new("ОстаткиТоваров"),
        };
        let report_this = lookup_field(&configs, &report, &Name::new("ЭтотОбъект"))
            .expect("ЭтотОбъект resolves on ReportObject");
        assert_eq!(
            report_this.ty,
            Ty::MetadataRef {
                kind: MetadataKind::ReportObject, name: Name::new("ОстаткиТоваров")
            },
            "ReportObject.ЭтотОбъект must specialize through ё↔е folding",
        );

        let dp = Ty::MetadataRef {
            kind: MetadataKind::DataProcessorObject,
            name: Name::new("Обработка1"),
        };
        let dp_this = lookup_field(&configs, &dp, &Name::new("ЭтотОбъект"))
            .expect("ЭтотОбъект resolves on DataProcessorObject");
        assert_eq!(
            dp_this.ty,
            Ty::MetadataRef {
                kind: MetadataKind::DataProcessorObject,
                name: Name::new("Обработка1"),
            },
            "DataProcessorObject.ЭтотОбъект must specialize to typed self",
        );
    }

    #[test]
    fn field_lookup_catalog_object_pulls_platform_properties() {
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("Номенклатура", vec![]));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };

        let dop = lookup_field(&configs, &receiver, &Name::new("ДополнительныеСвойства"))
            .expect("ДополнительныеСвойства must surface on CatalogObject via HBK");
        assert_eq!(dop.ty, Ty::Structure);
        assert!(dop.is_readonly);

        assert!(lookup_field(&configs, &receiver, &Name::new("ВерсияДанных")).is_some());
        assert!(lookup_field(&configs, &receiver, &Name::new("ЗаписьИсторииДанных")).is_some());
        assert!(lookup_field(&configs, &receiver, &Name::new("ЭтотОбъект")).is_some());
    }

    /// Seed a `Document "ПКО"` with the standard attributes that
    /// `bsl-metadata::xml_parser::standard_attributes` would materialise
    /// for an Object-view spec at parse time. Direct `MetadataObject::new`
    /// does not run that adapter, so tests that pin the
    /// `mdo.attributes` → HBK cascade priority must seed manually.
    fn document_with_standard_attrs(name: &str) -> MetadataObject {
        document(
            name,
            vec![
                attr(
                    "Ссылка",
                    Some("Ref"),
                    AttributeType::Ref { mdo_type: MdoType::Document, name: name.to_string() },
                ),
                attr("ПометкаУдаления", Some("DeletionMark"), AttributeType::Boolean),
                attr("Дата", Some("Date"), AttributeType::DateTime),
                attr("Проведен", Some("Posted"), AttributeType::Boolean),
            ],
        )
    }

    #[test]
    fn field_lookup_document_object_priority_pin_keeps_typed_standard_attrs() {
        // Priority of `push_unique`: `mdo.attributes` (typed standard attrs
        // from the spec) push first, HBK cascade pushes last. Where the spec
        // materializes the attribute (`PresenceCondition::Always`), its type
        // must win over the HBK fall-through.
        //
        // Regression guard: if the push order ever flips, `Дата` could
        // surface as a `Ty::PlatformObject(...)` rather than `Ty::Date`.
        //
        // `Номер` is intentionally NOT pinned: it is `HasNumber`-gated, so
        // a stripped-down test config leaves it unmaterialized and the
        // HBK fall-through (declared as `Число|Строка`) wins — that's
        // documented Phase A behavior, deferred to B1.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(document_with_standard_attrs("ПКО"));
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") };

        let date = lookup_field(&configs, &receiver, &Name::new("Дата"))
            .expect("standard Дата resolves on DocumentObject");
        assert_eq!(date.ty, Ty::Date, "spec-typed Дата must win over HBK cascade");
        assert!(!date.is_readonly, "Дата on DocumentObject is writable per spec");

        let r#ref = lookup_field(&configs, &receiver, &Name::new("Ссылка"))
            .expect("standard Ссылка resolves on DocumentObject");
        assert_eq!(
            r#ref.ty,
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") },
            "Ссылка must remain a typed self-ref, not a stringly-typed HBK entry",
        );

        let posted = lookup_field(&configs, &receiver, &Name::new("Проведен"))
            .expect("standard Проведен resolves on DocumentObject");
        assert_eq!(posted.ty, Ty::Boolean, "spec-typed Проведен must keep its Boolean type");
    }

    #[test]
    fn field_lookup_document_ref_caveats_and_cascade() {
        // DocumentRef has a separate HBK prefix (`DocumentRef.<Имя>`) and a
        // smaller property surface — no `ДополнительныеСвойства`,
        // no `Движения`, no `ЭтотОбъект`.
        //
        // Pins two invariants:
        //   1. `platform_prefix()` keys differentiate Object vs Ref —
        //      Object-only properties must NOT leak onto Ref.
        //   2. B1 caveat: when the configuration's `mdo.attributes` seeds
        //      `Дата` (Object-view spec, writable), that entry wins
        //      `push_unique` even on a Ref receiver — so the HBK cascade
        //      cannot uplift `Дата.is_readonly = true` on DocumentRef
        //      until the spec gets a Ref-view variant (see B1).
        let mut config = Configuration::new("Test");
        config.add_metadata_object(document_with_standard_attrs("ПКО"));
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") };

        // 1) Object-only HBK properties must NOT appear on Ref.
        assert!(
            lookup_field(&configs, &receiver, &Name::new("ДополнительныеСвойства")).is_none(),
            "Object-only property must not leak into DocumentRef surface",
        );
        assert!(
            lookup_field(&configs, &receiver, &Name::new("Движения")).is_none(),
            "Движения is DocumentObject-only per HBK",
        );
        assert!(
            lookup_field(&configs, &receiver, &Name::new("ЭтотОбъект")).is_none(),
            "ЭтотОбъект is DocumentObject-only per HBK",
        );

        // 2) HBK property declared only on DocumentRef must surface
        // via the cascade (no mdo.attributes collision).
        assert!(
            lookup_field(&configs, &receiver, &Name::new("ВерсияДанных")).is_some(),
            "DocumentRef.ВерсияДанных must surface via HBK cascade",
        );

        // 3) B1 caveat: standard `Дата` collides with seeded Object-view
        // mdo.attributes entry → mdo wins, so is_readonly is the
        // Object-view value (writable). When B1 introduces Ref-view spec
        // entries, this assertion flips to `is_readonly`.
        let date = lookup_field(&configs, &receiver, &Name::new("Дата"))
            .expect("standard Дата resolves on DocumentRef");
        assert!(
            !date.is_readonly,
            "Phase A limitation: DocumentRef.Дата is_readonly is not uplifted from Object-view spec (see B1)",
        );
    }

    #[test]
    fn field_lookup_cascade_respects_presence_conditional_standard_attrs() {
        // Codex stop-time review pin: the HBK cascade must NOT surface a
        // standard attribute whose presence the spec gates on a
        // configuration property (`HasCode`, `HasNumber`, `Hierarchical`,
        // `HasOwners`, `IsPeriodic`).
        //
        // A `Document` with no `Номер` materialised in `mdo.attributes`
        // (e.g. the config has no `NumberLength`) must NOT see `Номер`
        // bleeding in from HBK's `DocumentObject.<Имя>.Номер` declaration
        // — the spec is the sole arbiter of presence.
        //
        // Same for `Код` on a catalog without `CodeLength`, `ЭтоГруппа`
        // / `Родитель` on a non-hierarchical catalog, `Владелец` on a
        // catalog without owners.
        let mut config = Configuration::new("Test");
        // Intentionally NO standard attrs seeded: simulates a doc/catalog
        // whose XML did not configure HasNumber/HasCode/Hierarchical/HasOwners.
        config.add_metadata_object(document("ПКО", vec![]));
        config.add_metadata_object(catalog("Номенклатура", vec![]));
        let configs = wrap(config);

        let doc = Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") };
        assert!(
            lookup_field(&configs, &doc, &Name::new("Номер")).is_none(),
            "Номер must NOT leak from HBK when the document has no NumberLength",
        );

        let cat = Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };
        assert!(
            lookup_field(&configs, &cat, &Name::new("Код")).is_none(),
            "Код must NOT leak from HBK when the catalog has no CodeLength",
        );
        assert!(
            lookup_field(&configs, &cat, &Name::new("ЭтоГруппа")).is_none(),
            "ЭтоГруппа must NOT leak from HBK when the catalog is not Hierarchical",
        );
        assert!(
            lookup_field(&configs, &cat, &Name::new("Родитель")).is_none(),
            "Родитель must NOT leak from HBK when the catalog is not Hierarchical",
        );
        assert!(
            lookup_field(&configs, &cat, &Name::new("Владелец")).is_none(),
            "Владелец must NOT leak from HBK when the catalog has no Owners",
        );

        // Counter-pin: non-spec HBK properties still come through.
        assert!(
            lookup_field(&configs, &cat, &Name::new("ДополнительныеСвойства")).is_some(),
            "non-spec HBK properties remain visible after the gate",
        );
    }

    #[test]
    fn field_lookup_non_mdo_receivers_unaffected_by_mdo_cascade() {
        // Negative pin: `MetadataRef{RegisterFilter, ..}` and
        // `MetadataRef{TabularSectionRow, ..}` dispatch to their own
        // enumerators (`enumerate_filter_fields` / `enumerate_tabular_row_fields`,
        // see field_enum.rs:127 and field_enum.rs:131) and never visit
        // `enumerate_mdo_fields`. They must NOT surface object-flavoured
        // HBK properties.
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегСвед",
            MdoType::InformationRegister,
            vec![dimension_typed("Валюта", AttributeType::String { length: Some(3) })],
            vec![],
            vec![],
        ));
        config.add_metadata_object({
            let mut mdo = catalog("Товары", vec![]);
            let mut ts = TabularSection::new(Uuid::new_v4(), "Цены");
            ts.set_attributes(vec![TabularSectionAttribute::new(
                Uuid::new_v4(),
                "Цена",
                AttributeType::Number { precision: 15, scale: 2 },
            )]);
            mdo.add_tabular_section(ts);
            mdo
        });
        let configs = wrap(config);

        let filter = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("РегСвед"),
        };
        assert!(
            lookup_field(&configs, &filter, &Name::new("ДополнительныеСвойства")).is_none(),
            "RegisterFilter must not pull DocumentObject/CatalogObject cascade",
        );

        let row = Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
            name: Name::new("Товары.Цены"),
        };
        assert!(
            lookup_field(&configs, &row, &Name::new("ДополнительныеСвойства")).is_none(),
            "TabularSectionRow must not pull MDO cascade",
        );
    }

    // ============================================================
    // SDBL projection-branch lookup (Phase 1.3 Slice 2)
    // ============================================================

    fn projection_with_two_fields() -> std::sync::Arc<hir_def::ty::SdblProjection> {
        // Manually-constructed projection mirroring what the bridge
        // produces for `SELECT Код AS КодТов, Наименование FROM …`.
        std::sync::Arc::new(hir_def::ty::SdblProjection {
            fields: std::sync::Arc::from([
                (Name::new("КодТов"), Ty::String),
                (Name::new("Наименование"), Ty::String),
            ]),
            raw_sdbl_types: None,
        })
    }

    #[test]
    fn sdbl_projection_field_resolves_via_projection_table() {
        let receiver = Ty::QueryResultSelection { projection: Some(projection_with_two_fields()) };
        let info = lookup_field(&[], &receiver, &Name::new("КодТов"))
            .expect("projection field must resolve");
        assert_eq!(info.ty, Ty::String);
        assert!(info.is_readonly, "SDBL projection fields are read-only");
    }

    #[test]
    fn sdbl_projection_field_lookup_is_case_insensitive() {
        // BSL field access is case-folded; the projection lookup
        // matches the same way platform field/method lookup does.
        let receiver = Ty::QueryResultSelection { projection: Some(projection_with_two_fields()) };
        assert!(lookup_field(&[], &receiver, &Name::new("кодтов")).is_some());
        assert!(lookup_field(&[], &receiver, &Name::new("НАИМЕНОВАНИЕ")).is_some());
    }

    #[test]
    fn sdbl_projection_falls_through_to_platform_on_miss() {
        // Field not in the projection — must fall through to the
        // platform `ВыборкаИзРезультатаЗапроса` table so platform
        // properties like `НомерСтроки` still resolve.
        // (The exact platform property must exist in `platform_data`;
        // we don't pin the lookup result here — only that the
        // projection-branch returned `None` so the orchestrator
        // continued to the platform fallback.)
        let receiver = Ty::QueryResultSelection { projection: Some(projection_with_two_fields()) };
        assert!(
            lookup_field_in_query_projection(&receiver, &Name::new("НесуществующееПоле")).is_none(),
            "projection lookup must miss on unknown field — orchestrator continues to platform fallback",
        );
    }

    #[test]
    fn sdbl_projection_lookup_no_op_for_none_projection() {
        // `QueryResultSelection{None}` carries no projection (dynamic
        // query text or pre-Phase-1.3b lift). The projection branch
        // must return `None` immediately so the orchestrator falls
        // through to platform property lookup — preserving the
        // legacy `Ty::PlatformObject` behaviour for tests that pin
        // the chain without any SDBL trace.
        let receiver = Ty::QueryResultSelection { projection: None };
        assert!(lookup_field_in_query_projection(&receiver, &Name::new("Имя")).is_none());
    }

    #[test]
    fn sdbl_projection_lookup_no_op_for_non_selection_receiver() {
        // A different receiver shape that happens to carry a
        // projection (e.g. `Ty::Query{Some(p)}` after Phase 1.3b)
        // must NOT be served by this branch — the projection
        // surface only exists on the *selection* cursor.
        let receiver = Ty::Query { projections: Arc::from([Some(projection_with_two_fields())]) };
        assert!(lookup_field_in_query_projection(&receiver, &Name::new("КодТов")).is_none());
    }
}
