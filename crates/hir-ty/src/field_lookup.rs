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
fn project_form_data_for_fields(ty: &Ty) -> Option<Ty> {
    let Ty::FormData { kind, underlying: Some((mdo, name)) } = ty else {
        return None;
    };
    if !matches!(kind, FormDataKind::Structure | FormDataKind::StructureWithCollection) {
        return None;
    }
    let object_kind = MetadataKind::object_kind_for(*mdo)?;
    Some(Ty::MetadataRef { kind: object_kind, name: name.clone() })
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
    // Managed-form attribute projection happens BEFORE `ThisObject`
    // coercion: a `Ty::FormData { Structure | StructureWithCollection,
    // underlying: Some((mdo, n)) }` walks the MDO's attribute table for
    // field lookup (`Объект.Дата` must reach the document's `Дата`).
    // Method lookup deliberately doesn't do this — see
    // [`Ty::FormData`] docs.
    let projected_form_data = project_form_data_for_fields(receiver_ty);
    let projected_ty = projected_form_data.as_ref().unwrap_or(receiver_ty);

    // `Ty::ThisObject` coercion and dispatch split: MetadataRef → enumerator,
    // Union → intersection over live arms, everything else → platform-property adapter.
    let coerced = crate::this_object::coerce_to_metadata_ref(projected_ty);
    let effective_ty = coerced.as_ref().unwrap_or(projected_ty);

    // Union receivers (e.g. `НайтиСтроки(...) → Union(TabularSectionRow,
    // Undefined)`). Skip nullish arms (consistent with `enumerate_fields`
    // and `method_lookup`), then require the field to be present in every
    // remaining arm — that's the safe semantic for `x: A | B`. The
    // resulting [`Ty`] is the union of per-arm field types via
    // [`Ty::union`]; `is_readonly` is the disjunction (writeable only if
    // every arm is writeable).
    if let Ty::Union(arms) = effective_ty {
        let live: Vec<&Ty> =
            arms.iter().filter(|t| !matches!(t, Ty::Undefined | Ty::Null)).collect();
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
        return Some(FieldInfo {
            name: first.name.clone(),
            name_en: first.name_en.clone(),
            ty: merged_ty,
            is_readonly: merged_readonly,
            origin: first.origin,
        });
    }

    if matches!(effective_ty, Ty::MetadataRef { .. }) {
        let needle = field_name.as_str().to_lowercase();
        return crate::field_enum::enumerate_fields(configs, effective_ty).into_iter().find(|f| {
            f.name.as_str().to_lowercase() == needle
                || f.name_en.as_ref().is_some_and(|en| en.as_str().to_lowercase() == needle)
        });
    }

    // Every other receiver type delegates to the platform-property adapter.
    // `lookup_platform_property` decides whether the shape is supported
    // (primitives return `None`), so we can safely call it for any
    // non-MetadataRef receiver.
    crate::platform_property_lookup::lookup_platform_property(receiver_ty, field_name).map(|res| {
        use crate::field_enum::FieldOrigin;
        FieldInfo {
            name: field_name.clone(),
            name_en: None,
            ty: res.return_ty,
            is_readonly: res.is_readonly,
            origin: FieldOrigin::PlatformProperty,
        }
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
}
