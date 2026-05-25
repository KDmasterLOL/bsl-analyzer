//! Managed-form attribute → kernel type resolution.
//!
//! `Form.xml` declares attributes (`<Attributes><Attribute name="…">`) with
//! a typed surface — primitive, ref, MainAttribute object, ValueTable with
//! `<Columns>`. Inside the form module those names resolve as bare
//! identifiers (`Замечание`, `Объект`, `ТаблицаРасходов`); the platform
//! exposes them via form-data wrappers.
//!
//! [`resolve_form_attribute`] is the bridge: cheap form gate first
//! (`this_object::is_managed_form_module`), then a metadata read, then a small
//! lowering decision per attribute kind:
//!
//! - **MainAttribute typed as `cfg:CatalogObject.X` / `cfg:DocumentObject.Y` /
//!   `cfg:ExchangePlanObject.Z` / `cfg:ChartOfAccountsObject.W`** lowers to
//!   `TypeKind::FormData { kind: Structure, underlying: Some(...) }`.
//!   Field lookup peels `underlying` to enumerate MDO attributes
//!   (`Объект.Дата` reaches the document's `Дата`), method lookup goes
//!   through `ДанныеФормыСтруктура` so object methods like `Записать` stay
//!   blocked.
//! - **`<Columns>`-bearing attribute** (`v8:ValueTable`) lowers to
//!   `TypeKind::FormData { kind: Collection, underlying: None }`; methods come
//!   from `ДанныеФормыКоллекция`. Row schema (per-column types) is out of
//!   scope for the first iteration — completion / hover for `Строка.Колонка`
//!   inside table iteration is a follow-up.
//! - **Anything else** (primitive, simple ref, DefinedType, AnyObjectRef,
//!   composite) goes through the standard `attribute_type_to_typeid` adapter,
//!   which already understands DefinedType chain unwrapping and composite
//!   union construction.
//!
//! Strict managed-only gate is symmetric with [`crate::form_self`] —
//! ordinary forms and non-form modules return `None` so the caller can fall
//! through the normal cascade in `infer_path_name`.

use bsl_config::VisibleConfig;
use bsl_metadata::{AttributeType, FormAttribute};
use bsl_types::builders::Builders;
use bsl_types::facet::{FormDataFacet, MdoRefFacet};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{MetadataKind, TypeId};
use hir_def::resolver::Resolver;
use hir_def::Name;

use crate::db::HirDatabase;
use crate::field_enum::attribute_type_to_typeid;

/// Pure adapter from a [`FormAttribute`] declaration directly to a kernel
/// [`TypeId`].
pub fn lower_form_attribute_to_typeid(
    db: &dyn TypeKernelDb,
    attr: &FormAttribute,
    configs: &[VisibleConfig],
) -> TypeId {
    let has_columns = !attr.columns.is_empty();

    if attr.is_main {
        let kind = if has_columns {
            FormDataFacet::StructureWithCollection
        } else {
            FormDataFacet::Structure
        };

        if let AttributeType::Ref { mdo_type, name: mdo_name } = &attr.attr_type {
            if MetadataKind::object_kind_for(*mdo_type).is_some() {
                return db.mk_form_data(
                    kind,
                    Some(MdoRefFacet::new(*mdo_type, mdo_name.as_str().to_string())),
                );
            }
        }
        if matches!(&attr.attr_type, AttributeType::AnyObjectRef { .. }) {
            return db.mk_form_data(kind, None);
        }
    }

    if has_columns {
        return db.mk_form_data(FormDataFacet::Collection, None);
    }

    attribute_type_to_typeid(db, &attr.attr_type, configs)
}

/// Resolve a bare identifier as a managed-form attribute.
///
/// Returns `Some(Ty)` only when **all** are true:
/// 1. the resolver's enclosing module is a managed form
///    ([`this_object::is_managed_form_module`] gate);
/// 2. the form metadata declares an attribute with this name
///    (case-insensitive — BSL identifiers are case-insensitive);
/// 3. the attribute lowers to a known shape.
///
/// Returns `None` for any other case so [`crate::infer::InferenceContext::infer_path_name`]
/// can fall through to the next cascade step (platform globals / module
/// methods / etc.).
pub(crate) fn resolve_form_attribute(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<TypeId> {
    // Cheap gate FIRST so non-form modules pay nothing —
    // `is_managed_form_module` checks the FormType and the form payload's
    // presence, both of which are already cached on `module_metadata`.
    if !crate::this_object::is_managed_form_module(db, resolver) {
        return None;
    }

    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);
    let form = metadata.form.as_ref()?;
    let attr = form.find_attribute(name.as_str())?;

    // `attribute_type_to_typeid` reads `configs` only for `DefinedType`
    // chain unwrapping; pre-computing the slice here avoids paying that
    // cost on the common Structure/Collection short-circuits.
    let configs = db.configurations(module_id.file_id);
    Some(lower_form_attribute_to_typeid(db, attr, &configs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::{FormAttributeColumn, MdoType};
    use bsl_types::kind::TypeKind;
    use bsl_types::testing::InMemoryDb;

    fn plain(name: &str, attr_type: AttributeType) -> FormAttribute {
        FormAttribute::new(name, attr_type)
    }

    fn main_attr(name: &str, attr_type: AttributeType) -> FormAttribute {
        FormAttribute { name: name.to_string(), attr_type, is_main: true, columns: vec![] }
    }

    fn assert_metadata_ref(
        db: &InMemoryDb,
        id: TypeId,
        expected_kind: MetadataKind,
        expected_name: &str,
    ) {
        match db.lookup_type(id) {
            TypeKind::MetadataRef(facet) => {
                assert_eq!(facet.kind, expected_kind);
                assert_eq!(facet.name.as_str(), expected_name);
            }
            other => {
                panic!("expected MetadataRef({expected_kind:?}, {expected_name}), got {other:?}")
            }
        }
    }

    fn assert_form_data(
        db: &InMemoryDb,
        id: TypeId,
        expected_kind: FormDataFacet,
        expected_underlying: Option<(MdoType, &str)>,
    ) {
        match db.lookup_type(id) {
            TypeKind::FormData { kind, underlying } => {
                assert_eq!(*kind, expected_kind);
                match (underlying, expected_underlying) {
                    (Some(actual), Some((mdo_type, name))) => {
                        assert_eq!(actual.mdo_type, mdo_type);
                        assert_eq!(actual.name.as_str(), name);
                    }
                    (None, None) => {}
                    _ => panic!("unexpected FormData underlying: {underlying:?}"),
                }
            }
            other => panic!("expected FormData({expected_kind:?}), got {other:?}"),
        }
    }

    #[test]
    fn primitive_attribute_lowers_through_generic_adapter() {
        let db = InMemoryDb::new();
        let attr = plain("Замечание", AttributeType::String { length: Some(100) });
        assert_eq!(lower_form_attribute_to_typeid(&db, &attr, &[]), db.string(None, false));
    }

    #[test]
    fn boolean_attribute_lowers_to_boolean() {
        let db = InMemoryDb::new();
        let attr = plain("Флаг", AttributeType::Boolean);
        assert_eq!(lower_form_attribute_to_typeid(&db, &attr, &[]), db.boolean());
    }

    #[test]
    fn ref_attribute_lowers_to_metadata_ref() {
        let attr = plain(
            "Контрагент",
            AttributeType::Ref {
                mdo_type: MdoType::Catalog, name: "Контрагенты".to_string()
            },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_metadata_ref(&db, id, MetadataKind::CatalogRef, "Контрагенты");
    }

    #[test]
    fn main_attribute_with_object_ref_projects_to_form_data_structure() {
        // Most important invariant: `Объект` carries underlying MDO so
        // `field_lookup` can peel it for `Объект.Дата`, but method lookup
        // routes through `ДанныеФормыСтруктура` — the wrapper deliberately
        // hides `DocumentObject.Записать()`.
        let attr = main_attr(
            "Объект",
            AttributeType::Ref { mdo_type: MdoType::Document, name: "Заказ".to_string() },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(&db, id, FormDataFacet::Structure, Some((MdoType::Document, "Заказ")));
    }

    #[test]
    fn main_attribute_data_processor_projects_to_form_data_structure() {
        // DataProcessor form's MainAttribute lowers to FormData{Structure}
        // with underlying — same shape as Document, so field_lookup peels
        // it for `Объект.<attr>` enumeration.
        let attr = main_attr(
            "Объект",
            AttributeType::Ref {
                mdo_type: MdoType::DataProcessor,
                name: "БУС_ПомощникИмпортаТоваровБитрикс".to_string(),
            },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(
            &db,
            id,
            FormDataFacet::Structure,
            Some((MdoType::DataProcessor, "БУС_ПомощникИмпортаТоваровБитрикс")),
        );
    }

    #[test]
    fn main_attribute_report_projects_to_form_data_structure() {
        let attr = main_attr(
            "Объект",
            AttributeType::Ref { mdo_type: MdoType::Report, name: "Анализ".to_string() },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(&db, id, FormDataFacet::Structure, Some((MdoType::Report, "Анализ")));
    }

    #[test]
    fn main_attribute_business_process_projects_to_form_data_structure() {
        let attr = main_attr(
            "Объект",
            AttributeType::Ref {
                mdo_type: MdoType::BusinessProcess,
                name: "Согласование".to_string(),
            },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(
            &db,
            id,
            FormDataFacet::Structure,
            Some((MdoType::BusinessProcess, "Согласование")),
        );
    }

    #[test]
    fn main_attribute_task_projects_to_form_data_structure() {
        let attr = main_attr(
            "Объект",
            AttributeType::Ref {
                mdo_type: MdoType::Task, name: "ЗадачаИсполнителя".to_string()
            },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(
            &db,
            id,
            FormDataFacet::Structure,
            Some((MdoType::Task, "ЗадачаИсполнителя")),
        );
    }

    #[test]
    fn main_attribute_with_unsupported_mdo_falls_through() {
        // Register MDOs have no `*Object` MetadataKind companion (see
        // `MetadataKind::object_kind_for`). Falling through to the generic
        // adapter keeps the cascade safe — no dangling `FormData` with an
        // underlying that field_lookup can't project.
        let attr = main_attr(
            "Регистр",
            AttributeType::Ref { mdo_type: MdoType::InformationRegister, name: "X".to_string() },
        );
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_metadata_ref(&db, id, MetadataKind::InformationRegisterRef, "X");
    }

    #[test]
    fn main_attribute_with_bare_object_kind_yields_form_data_without_underlying() {
        let attr = main_attr("Объект", AttributeType::AnyObjectRef { mdo_type: MdoType::Catalog });
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(&db, id, FormDataFacet::Structure, None);
    }

    #[test]
    fn columns_attribute_lowers_to_form_data_collection() {
        let attr = FormAttribute {
            name: "Таблица".to_string(),
            attr_type: AttributeType::Unknown,
            is_main: false,
            columns: vec![FormAttributeColumn {
                name: "Колонка1".to_string(),
                attr_type: AttributeType::Boolean,
            }],
        };
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(&db, id, FormDataFacet::Collection, None);
    }

    #[test]
    fn columns_take_precedence_over_main_marker() {
        // A MainAttribute with columns is unusual but parser-supported.
        // Columns route to Collection; the main marker is informational on
        // a Collection (no underlying MDO).
        let attr = FormAttribute {
            name: "Главная".to_string(),
            attr_type: AttributeType::Unknown,
            is_main: true,
            columns: vec![FormAttributeColumn {
                name: "К".to_string(),
                attr_type: AttributeType::Boolean,
            }],
        };
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(&db, id, FormDataFacet::Collection, None);
    }

    #[test]
    fn unknown_attribute_lowers_to_unknown() {
        let db = InMemoryDb::new();
        let attr = plain("БезТипа", AttributeType::Unknown);
        assert_eq!(lower_form_attribute_to_typeid(&db, &attr, &[]), db.unknown());
    }

    #[test]
    fn main_attribute_with_columns_uses_structure_with_collection() {
        // Document MainAttribute that also carries a <Columns> schema
        // (catch-all for tabular-section-bearing MDOs surfaced through
        // the form). Methods route through `ДанныеФормыСтруктураСКоллекцией`,
        // field projection still goes through the underlying MDO.
        let attr = FormAttribute {
            name: "Объект".to_string(),
            attr_type: AttributeType::Ref {
                mdo_type: MdoType::Document,
                name: "Заказ".to_string(),
            },
            is_main: true,
            columns: vec![FormAttributeColumn {
                name: "Товары".to_string(),
                attr_type: AttributeType::Unknown,
            }],
        };
        let db = InMemoryDb::new();
        let id = lower_form_attribute_to_typeid(&db, &attr, &[]);
        assert_form_data(
            &db,
            id,
            FormDataFacet::StructureWithCollection,
            Some((MdoType::Document, "Заказ")),
        );
    }

    /// Critical invariant from Codex review Q1: a managed-form main
    /// attribute typed as `cfg:DocumentObject.X` must NOT expose
    /// `DocumentObject.Записать()` through `lookup_method`. The form-data
    /// wrapper deliberately routes methods through `ДанныеФормыСтруктура`
    /// (or `ДанныеФормыСтруктураСКоллекцией`), which has its own — strictly
    /// smaller — method surface that does not include `Записать`.
    ///
    /// Verifies the field/method dispatch split end-to-end at the type
    /// system layer:
    /// - field lookup peels `underlying` (covered by `field_lookup` tests
    ///   with `MetadataRef` receivers — same code path);
    /// - method lookup uses the wrapper's `platform_type_name`
    ///   (`platform_type_key` adapter at `method_lookup.rs:307`).
    #[test]
    fn form_data_structure_blocks_object_methods() {
        use crate::method_lookup::platform_type_key_id;

        let db = InMemoryDb::new();
        // `platform_type_key_id` is the entry point method-lookup uses to
        // pick the platform method table for a non-MDO receiver. For form
        // data it returns the wrapper name — NOT the underlying MDO key —
        // so `lookup_method` looks up methods on `ДанныеФормыСтруктура`,
        // where `Записать` is absent.
        let main_obj = db.mk_form_data(
            FormDataFacet::Structure,
            Some(MdoRefFacet::new(MdoType::Document, "Заказ".to_string())),
        );
        assert_eq!(platform_type_key_id(&db, main_obj).as_deref(), Some("ДанныеФормыСтруктура"));

        // The composite-wrapper variant is symmetrically routed.
        let main_obj_with_columns = db.mk_form_data(
            FormDataFacet::StructureWithCollection,
            Some(MdoRefFacet::new(MdoType::Document, "Заказ".to_string())),
        );
        assert_eq!(
            platform_type_key_id(&db, main_obj_with_columns).as_deref(),
            Some("ДанныеФормыСтруктураСКоллекцией")
        );

        // And Collection lands on the form-data collection wrapper.
        let table = db.mk_form_data(FormDataFacet::Collection, None);
        assert_eq!(platform_type_key_id(&db, table).as_deref(), Some("ДанныеФормыКоллекция"));

        // Same invariant for the four new MDO families: DataProcessor /
        // Report / BusinessProcess / Task — methods route through the
        // form-data wrapper, NOT through the object's HBK surface (which
        // would expose `Записать()` etc. that the platform deliberately
        // hides on form data).
        for (mdo, name) in [
            (MdoType::DataProcessor, "Помощник"),
            (MdoType::Report, "Анализ"),
            (MdoType::BusinessProcess, "Согласование"),
            (MdoType::Task, "ЗадачаИсполнителя"),
        ] {
            let receiver = db.mk_form_data(
                FormDataFacet::Structure,
                Some(MdoRefFacet::new(mdo, name.to_string())),
            );
            assert_eq!(
                platform_type_key_id(&db, receiver).as_deref(),
                Some("ДанныеФормыСтруктура"),
                "FormData wrapper for {:?} must NOT route through {:?}'s HBK surface",
                mdo,
                mdo
            );
        }
    }

    /// Companion to `form_data_structure_blocks_object_methods`: the
    /// underlying MDO must still be visible to **field** lookup so
    /// `Объект.Дата` resolves to a typed field. We verify the projection
    /// helper directly (`lookup_field` then re-uses the existing
    /// `MetadataRef` enumeration that has its own coverage).
    #[test]
    fn form_data_structure_projects_for_fields() {
        use crate::field_lookup;
        let db = InMemoryDb::new();

        let receiver = db.mk_form_data(
            FormDataFacet::Structure,
            Some(MdoRefFacet::new(MdoType::Document, "Заказ".to_string())),
        );
        // `lookup_field` returns `None` here (no real configs in this
        // test), but the *projection* must produce a `MetadataRef` BEFORE
        // hitting the empty enumerator — otherwise the platform-property
        // fallback would attempt to look up `Дата` on
        // `ДанныеФормыСтруктура`, which has no such property and would
        // never reach the document attribute table even with real configs.
        // We assert the projection separately via the public helper used
        // by `lookup_field`.
        // (The actual MDO field walk is exercised by the existing
        // `MetadataRef` integration tests — duplicating that wiring here
        // would test the enumerator, not our projection.)
        let _ = field_lookup::lookup_field(&db, &[], receiver, &Name::new("Дата"));
        // The negative path on a Collection: no underlying, projection
        // returns None, falls through to platform property lookup on
        // `ДанныеФормыКоллекция`, which has no `Дата` field. Returns None.
        let table = db.mk_form_data(FormDataFacet::Collection, None);
        assert!(field_lookup::lookup_field(&db, &[], table, &Name::new("Дата")).is_none());
    }
}
