//! Managed-form attribute → [`Ty`] resolution.
//!
//! `Form.xml` declares attributes (`<Attributes><Attribute name="…">`) with
//! a typed surface — primitive, ref, MainAttribute object, ValueTable with
//! `<Columns>`. Inside the form module those names resolve as bare
//! identifiers (`Замечание`, `Объект`, `ТаблицаРасходов`); the platform
//! exposes them via form-data wrappers.
//!
//! [`resolve_form_attribute`] is the bridge: cheap form gate first
//! (`Resolver::resolve_this_form`), then a metadata read, then a small
//! lowering decision per attribute kind:
//!
//! - **MainAttribute typed as `cfg:CatalogObject.X` / `cfg:DocumentObject.Y` /
//!   `cfg:ExchangePlanObject.Z` / `cfg:ChartOfAccountsObject.W`** lowers to
//!   [`Ty::FormData { kind: Structure, underlying: Some((mdo, name)) }`].
//!   Field lookup peels `underlying` to enumerate MDO attributes
//!   (`Объект.Дата` reaches the document's `Дата`), method lookup goes
//!   through `ДанныеФормыСтруктура` so object methods like `Записать` stay
//!   blocked.
//! - **`<Columns>`-bearing attribute** (`v8:ValueTable`) lowers to
//!   [`Ty::FormData { kind: Collection, underlying: None }`]; methods come
//!   from `ДанныеФормыКоллекция`. Row schema (per-column types) is out of
//!   scope for the first iteration — completion / hover for `Строка.Колонка`
//!   inside table iteration is a follow-up.
//! - **Anything else** (primitive, simple ref, DefinedType, AnyObjectRef,
//!   composite) goes through the standard [`attribute_type_to_ty`] adapter,
//!   which already understands DefinedType chain unwrapping and composite
//!   union construction.
//!
//! Strict managed-only gate is symmetric with [`crate::form_self`] —
//! ordinary forms and non-form modules return `None` so the caller can fall
//! through the normal cascade in `infer_path_name`.

use bsl_metadata::{AttributeType, FormAttribute};
use hir_def::configs::VisibleConfig;
use hir_def::resolver::Resolver;
use hir_def::ty::{FormDataKind, MetadataKind, Ty};
use hir_def::Name;

use crate::db::HirDatabase;
use crate::field_enum::attribute_type_to_ty;

/// Pure adapter from a [`FormAttribute`] declaration to a [`Ty`].
///
/// Split out from [`resolve_form_attribute`] so the lowering rules
/// (MainAttribute → `FormData::Structure`, columns → `FormData::Collection`,
/// otherwise generic [`attribute_type_to_ty`]) can be unit-tested without
/// spinning up a Salsa database. The full resolver path layers the
/// managed-form gate and the metadata read on top of this function.
pub fn lower_form_attribute_to_ty(attr: &FormAttribute, configs: &[VisibleConfig]) -> Ty {
    let has_columns = !attr.columns.is_empty();

    if attr.is_main {
        // MainAttribute typed as `cfg:CatalogObject.X` etc. projects to
        // a structure wrapper. If the same attribute *also* carries a
        // `<Columns>` schema (the document/catalog declares tabular
        // sections accessible through the form), pick the platform's
        // composite wrapper `ДанныеФормыСтруктураСКоллекцией` so method
        // lookup picks up the structure-and-collection method table.
        // Field projection still uses the underlying MDO.
        let kind = if has_columns {
            FormDataKind::StructureWithCollection
        } else {
            FormDataKind::Structure
        };

        if let AttributeType::Ref { mdo_type, name: mdo_name } = &attr.attr_type {
            if MetadataKind::object_kind_for(*mdo_type).is_some() {
                return Ty::FormData {
                    kind,
                    underlying: Some((*mdo_type, Name::new(mdo_name.as_str()))),
                };
            }
        }
        if matches!(&attr.attr_type, AttributeType::AnyObjectRef { .. }) {
            // Bare `cfg:CatalogObject` (no name) — methods still live on
            // `ДанныеФормыСтруктура` (or the composite if columns exist),
            // but field projection has nothing to anchor to.
            return Ty::FormData { kind, underlying: None };
        }
        // Exotic MainAttribute (primitive / DefinedType / composite) —
        // unusual but not invalid; fall through to the generic adapter
        // unless columns force the collection wrapper below.
    }

    if has_columns {
        return Ty::FormData { kind: FormDataKind::Collection, underlying: None };
    }

    attribute_type_to_ty(&attr.attr_type, configs)
}

/// Resolve a bare identifier as a managed-form attribute.
///
/// Returns `Some(Ty)` only when **all** are true:
/// 1. the resolver's enclosing module is a managed form
///    ([`Resolver::resolve_this_form`] gate);
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
) -> Option<Ty> {
    // Cheap gate FIRST so non-form modules pay nothing — `resolve_this_form`
    // checks the FormType and the form payload's presence, both of which
    // are already cached on `module_metadata`.
    if !resolver.resolve_this_form(db) {
        return None;
    }

    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);
    let form = metadata.form.as_ref()?;
    let attr = form.find_attribute(name.as_str())?;

    // `attribute_type_to_ty` reads `configs` only for `DefinedType` chain
    // unwrapping; pre-computing the slice here avoids paying that cost on
    // the common Structure/Collection short-circuits but keeps the call
    // shape pure for testing (see [`lower_form_attribute_to_ty`]).
    let configs = db.configurations(module_id.file_id);
    Some(lower_form_attribute_to_ty(attr, &configs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::{FormAttributeColumn, MdoType};

    fn plain(name: &str, attr_type: AttributeType) -> FormAttribute {
        FormAttribute::new(name, attr_type)
    }

    fn main_attr(name: &str, attr_type: AttributeType) -> FormAttribute {
        FormAttribute { name: name.to_string(), attr_type, is_main: true, columns: vec![] }
    }

    #[test]
    fn primitive_attribute_lowers_through_generic_adapter() {
        let attr = plain("Замечание", AttributeType::String { length: Some(100) });
        assert_eq!(lower_form_attribute_to_ty(&attr, &[]), Ty::String);
    }

    #[test]
    fn boolean_attribute_lowers_to_boolean() {
        let attr = plain("Флаг", AttributeType::Boolean);
        assert_eq!(lower_form_attribute_to_ty(&attr, &[]), Ty::Boolean);
    }

    #[test]
    fn ref_attribute_lowers_to_metadata_ref() {
        let attr = plain(
            "Контрагент",
            AttributeType::Ref {
                mdo_type: MdoType::Catalog, name: "Контрагенты".to_string()
            },
        );
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::MetadataRef { kind, name } => {
                assert_eq!(kind, MetadataKind::CatalogRef);
                assert_eq!(name.as_str(), "Контрагенты");
            }
            other => panic!("expected MetadataRef{{CatalogRef,Контрагенты}}, got {:?}", other),
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: Some((mdo, name)) } => {
                assert_eq!(mdo, MdoType::Document);
                assert_eq!(name.as_str(), "Заказ");
            }
            other => {
                panic!("expected FormData{{Structure,Some((Document,Заказ))}}, got {:?}", other)
            }
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: Some((mdo, name)) } => {
                assert_eq!(mdo, MdoType::DataProcessor);
                assert_eq!(name.as_str(), "БУС_ПомощникИмпортаТоваровБитрикс");
            }
            other => {
                panic!("expected FormData{{Structure,Some((DataProcessor,..))}}, got {:?}", other)
            }
        }
    }

    #[test]
    fn main_attribute_report_projects_to_form_data_structure() {
        let attr = main_attr(
            "Объект",
            AttributeType::Ref { mdo_type: MdoType::Report, name: "Анализ".to_string() },
        );
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: Some((mdo, name)) } => {
                assert_eq!(mdo, MdoType::Report);
                assert_eq!(name.as_str(), "Анализ");
            }
            other => {
                panic!("expected FormData{{Structure,Some((Report,Анализ))}}, got {:?}", other)
            }
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: Some((mdo, name)) } => {
                assert_eq!(mdo, MdoType::BusinessProcess);
                assert_eq!(name.as_str(), "Согласование");
            }
            other => {
                panic!("expected FormData{{Structure,Some((BusinessProcess,..))}}, got {:?}", other)
            }
        }
    }

    #[test]
    fn main_attribute_task_projects_to_form_data_structure() {
        let attr = main_attr(
            "Объект",
            AttributeType::Ref {
                mdo_type: MdoType::Task, name: "ЗадачаИсполнителя".to_string()
            },
        );
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: Some((mdo, name)) } => {
                assert_eq!(mdo, MdoType::Task);
                assert_eq!(name.as_str(), "ЗадачаИсполнителя");
            }
            other => panic!("expected FormData{{Structure,Some((Task,..))}}, got {:?}", other),
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::MetadataRef { kind, .. } => {
                // Generic adapter produced a plain MetadataRef for the
                // register — not a FormData wrapper. The cascade stays
                // honest about an unusual configuration.
                assert!(matches!(kind, MetadataKind::InformationRegisterRef));
            }
            other => panic!("expected fall-through to MetadataRef, got {:?}", other),
        }
    }

    #[test]
    fn main_attribute_with_bare_object_kind_yields_form_data_without_underlying() {
        let attr = main_attr("Объект", AttributeType::AnyObjectRef { mdo_type: MdoType::Catalog });
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Structure, underlying: None } => {}
            other => panic!("expected FormData{{Structure,None}}, got {:?}", other),
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Collection, underlying: None } => {}
            other => panic!("expected FormData{{Collection,None}}, got {:?}", other),
        }
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData { kind: FormDataKind::Collection, .. } => {}
            other => panic!("columns must win for Collection wrapper, got {:?}", other),
        }
    }

    #[test]
    fn unknown_attribute_lowers_to_unknown() {
        let attr = plain("БезТипа", AttributeType::Unknown);
        assert_eq!(lower_form_attribute_to_ty(&attr, &[]), Ty::Unknown);
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
        match lower_form_attribute_to_ty(&attr, &[]) {
            Ty::FormData {
                kind: FormDataKind::StructureWithCollection,
                underlying: Some((mdo, name)),
            } => {
                assert_eq!(mdo, MdoType::Document);
                assert_eq!(name.as_str(), "Заказ");
            }
            other => panic!("expected StructureWithCollection with underlying, got {:?}", other),
        }
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
        use crate::method_lookup::platform_type_key;

        let main_obj = Ty::FormData {
            kind: FormDataKind::Structure,
            underlying: Some((MdoType::Document, Name::new("Заказ"))),
        };
        // `platform_type_key` is the entry point method-lookup uses to pick
        // the platform method table for a non-MDO receiver. For form data
        // it returns the wrapper name — NOT the underlying MDO key — so
        // `lookup_method` will look up methods on
        // `ДанныеФормыСтруктура`, where `Записать` is absent.
        assert_eq!(platform_type_key(&main_obj), Some("ДанныеФормыСтруктура"));

        // The composite-wrapper variant is symmetrically routed.
        let main_obj_with_columns = Ty::FormData {
            kind: FormDataKind::StructureWithCollection,
            underlying: Some((MdoType::Document, Name::new("Заказ"))),
        };
        assert_eq!(
            platform_type_key(&main_obj_with_columns),
            Some("ДанныеФормыСтруктураСКоллекцией")
        );

        // And Collection lands on the form-data collection wrapper.
        let table = Ty::FormData { kind: FormDataKind::Collection, underlying: None };
        assert_eq!(platform_type_key(&table), Some("ДанныеФормыКоллекция"));

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
            let receiver = Ty::FormData {
                kind: FormDataKind::Structure,
                underlying: Some((mdo, Name::new(name))),
            };
            assert_eq!(
                platform_type_key(&receiver),
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

        let receiver = Ty::FormData {
            kind: FormDataKind::Structure,
            underlying: Some((MdoType::Document, Name::new("Заказ"))),
        };
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
        let _ = field_lookup::lookup_field(&[], &receiver, &Name::new("Дата"));
        // The negative path on a Collection: no underlying, projection
        // returns None, falls through to platform property lookup on
        // `ДанныеФормыКоллекция`, which has no `Дата` field. Returns None.
        let table = Ty::FormData { kind: FormDataKind::Collection, underlying: None };
        assert!(field_lookup::lookup_field(&[], &table, &Name::new("Дата")).is_none());
    }
}
