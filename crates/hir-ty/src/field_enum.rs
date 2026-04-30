//! Single enumerator of receiver fields.
//!
//! [`enumerate_fields`] is the source of truth for "what fields does
//! `receiver_ty` expose?". [`crate::field_lookup::lookup_field`] is built on
//! top of it as a thin name filter; `hir::Type::fields()` calls it directly
//! to produce IDE completion / hover surfaces.
//!
//! # Helper migration
//!
//! The low-level helpers (`mdo_type_for_kind`, `register_parent_for_kind`,
//! `find_mdo`, `attribute_type_to_ty`, `register_part_ty`,
//! `split_parent_section`) were previously duplicated between
//! `field_lookup.rs` and `type_facade.rs`. They now live here as
//! `pub(crate)` so both modules use the single canonical copy.

use bsl_metadata::{AttributeType, MdoType, MetadataObject};
use bsl_platform::{standard_attributes_for, MdoTemplateKind, ObjectView};
use hir_def::configs::VisibleConfig;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;

use crate::lower::TyLoweringContext;

/// Where a field came from.
///
/// Lets IDE differentiate icons / sort priority: user-defined attributes
/// above standard ones, both above platform fall-throughs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOrigin {
    StandardAttribute,
    UserAttribute,
    TabularSection,
    TabularSectionRowColumn,
    RegisterDimension,
    RegisterResource,
    RegisterAttribute,
    /// Platform property, e.g. `НомерСтроки` on a tabular row.
    PlatformProperty,
}

/// A single field exposed by a receiver type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Russian canonical name.
    pub name: Name,
    /// English alias from metadata, if present.
    pub name_en: Option<Name>,
    /// Lowered type of the field.
    pub ty: Ty,
    /// `true` when the field is read-only (platform property or intrinsically
    /// read-only standard attribute like `Ссылка`, `НомерСтроки`).
    pub is_readonly: bool,
    /// Where this field came from.
    pub origin: FieldOrigin,
}

/// Enumerate every field exposed by `receiver_ty` against the visible
/// configurations.
///
/// Configuration iteration: `configs.iter().rev()` — extensions override
/// main on `(MdoType, name)` collisions.
///
/// `Ty::ThisObject` is coerced to its matching `*Object` `MetadataRef` at
/// the start, so callers do not need to handle it separately.
///
/// `Ty::Union` is descended into: each non-`Undefined`/`Null` arm is
/// enumerated and the results are merged with a name-based dedup. This
/// matches receiver shapes produced by upstream inference such as
/// `НайтиСтроки(...)` returning `Union(TabularSectionRow, Undefined)`,
/// so completion / lookup on the union keeps working.
///
/// Returns an empty `Vec` for receivers that have no field surface
/// (`Ty::Unknown`, primitives, `Ty::PlatformObject`, managers, plain
/// `TabularSection` collection receivers).
pub fn enumerate_fields(configs: &[VisibleConfig], receiver_ty: &Ty) -> Vec<FieldInfo> {
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let ty = coerced.as_ref().unwrap_or(receiver_ty);

    if let Ty::Union(arms) = ty {
        let mut out: Vec<FieldInfo> = Vec::new();
        let mut seen: std::collections::HashSet<Name> = std::collections::HashSet::new();
        for arm in arms.iter().filter(|t| !matches!(t, Ty::Undefined | Ty::Null)) {
            for info in enumerate_fields(configs, arm) {
                push_unique(&mut out, &mut seen, info);
            }
        }
        return out;
    }

    let Ty::MetadataRef { kind, name } = ty else {
        return Vec::new();
    };

    if let Some(mdo_type) = mdo_type_for_kind(*kind) {
        return enumerate_mdo_fields(configs, mdo_type, name);
    }

    if let Some(parent) = register_parent_for_kind(*kind) {
        return enumerate_register_fields(configs, parent, name);
    }

    if let MetadataKind::TabularSectionRow { parent } = kind {
        let Some((parent_name, section_name)) = split_parent_section(name.as_str()) else {
            return Vec::new();
        };
        return enumerate_tabular_row_fields(configs, *parent, parent_name, section_name);
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Internal enumerators
// ---------------------------------------------------------------------------

fn enumerate_mdo_fields(
    configs: &[VisibleConfig],
    mdo_type: MdoType,
    mdo_name: &Name,
) -> Vec<FieldInfo> {
    for cfg in configs.iter().rev() {
        let Some(mdo) = cfg.configuration.find_metadata_object(mdo_type, mdo_name.as_str()) else {
            continue;
        };

        let mut out = Vec::with_capacity(mdo.attributes.len() + mdo.tabular_sections.len());
        let mut seen: std::collections::HashSet<Name> =
            std::collections::HashSet::with_capacity(out.capacity() * 2);

        // Determine the template so we can classify standard vs user attrs.
        let template = mdo_template_kind_for(mdo_type);

        for attr in &mdo.attributes {
            let spec = classify_attr(template, &attr.name);
            let (origin, is_readonly) = match spec {
                Some(s) => (FieldOrigin::StandardAttribute, s.is_readonly),
                None => (FieldOrigin::UserAttribute, false),
            };
            let info = FieldInfo {
                name: Name::new(&attr.name),
                name_en: attr.name_en.as_deref().filter(|s| !s.is_empty()).map(Name::new),
                ty: attribute_type_to_ty(&attr.attr_type),
                is_readonly,
                origin,
            };
            push_unique(&mut out, &mut seen, info);
        }

        for ts in &mdo.tabular_sections {
            let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
            let info = FieldInfo {
                name: Name::new(ts.name()),
                name_en: ts.name_en().filter(|s| !s.is_empty()).map(Name::new),
                ty: Ty::MetadataRef {
                    kind: MetadataKind::TabularSection { parent: mdo_type },
                    name: qualified,
                },
                is_readonly: false,
                origin: FieldOrigin::TabularSection,
            };
            push_unique(&mut out, &mut seen, info);
        }

        return out;
    }
    Vec::new()
}

fn enumerate_register_fields(
    configs: &[VisibleConfig],
    parent: MdoType,
    register_name: &Name,
) -> Vec<FieldInfo> {
    for cfg in configs.iter().rev() {
        let Some(register) =
            cfg.configuration.find_register_by_type_and_name(parent, register_name.as_str())
        else {
            continue;
        };

        let cap =
            register.dimensions().len() + register.resources().len() + register.attributes().len();
        let mut out = Vec::with_capacity(cap);
        let mut seen: std::collections::HashSet<Name> =
            std::collections::HashSet::with_capacity(cap * 2);

        for dim in register.dimensions() {
            let info = FieldInfo {
                name: Name::new(dim.name()),
                // Dimension has no `name_en` in bsl-metadata.
                name_en: None,
                ty: register_part_ty(
                    dim.attr_type(),
                    MetadataKind::RegisterDimension { parent },
                    register_name,
                    dim.name(),
                ),
                is_readonly: false,
                origin: FieldOrigin::RegisterDimension,
            };
            push_unique(&mut out, &mut seen, info);
        }

        for res in register.resources() {
            let info = FieldInfo {
                name: Name::new(res.name()),
                name_en: res.name_en().filter(|s| !s.is_empty()).map(Name::new),
                ty: register_part_ty(
                    res.attr_type(),
                    MetadataKind::RegisterResource { parent },
                    register_name,
                    res.name(),
                ),
                is_readonly: false,
                origin: FieldOrigin::RegisterResource,
            };
            push_unique(&mut out, &mut seen, info);
        }

        for attr in register.attributes() {
            let info = FieldInfo {
                name: Name::new(attr.name()),
                name_en: attr.name_en().filter(|s| !s.is_empty()).map(Name::new),
                ty: register_part_ty(
                    attr.attr_type(),
                    MetadataKind::RegisterAttribute { parent },
                    register_name,
                    attr.name(),
                ),
                is_readonly: false,
                origin: FieldOrigin::RegisterAttribute,
            };
            push_unique(&mut out, &mut seen, info);
        }

        return out;
    }
    Vec::new()
}

fn enumerate_tabular_row_fields(
    configs: &[VisibleConfig],
    parent: MdoType,
    parent_name: &str,
    section_name: &str,
) -> Vec<FieldInfo> {
    let mdo = find_mdo(configs, parent, parent_name);
    let Some(mdo) = mdo else {
        return Vec::new();
    };
    let Some(ts) = mdo.find_tabular_section(section_name) else {
        return Vec::new();
    };

    let mut out: Vec<FieldInfo> = ts
        .attributes()
        .iter()
        .map(|attr| FieldInfo {
            name: Name::new(attr.name()),
            name_en: attr.name_en().filter(|s| !s.is_empty()).map(Name::new),
            ty: attribute_type_to_ty(attr.attr_type()),
            is_readonly: false,
            origin: FieldOrigin::TabularSectionRowColumn,
        })
        .collect();

    // Fall through to platform row properties (`НомерСтроки` / `LineNumber`).
    // HBK ships these under `type_name = "Line of a tabular section"`.
    // Custom XML attributes intentionally win on name collisions because they
    // are already in `out` at this point.
    let nr_name = Name::new("НомерСтроки");
    let nr_name_en = Name::new("LineNumber");
    let already_defined = out.iter().any(|f| {
        f.name.as_str().to_lowercase() == "номерстроки"
            || f.name_en.as_ref().is_some_and(|en| en.as_str().to_lowercase() == "linenumber")
    });
    if !already_defined {
        if let Some(prop) = crate::platform_property_lookup::lookup_platform_property_by_type(
            "Line of a tabular section",
            &nr_name,
        ) {
            out.push(FieldInfo {
                name: nr_name,
                name_en: Some(nr_name_en),
                ty: prop.return_ty,
                is_readonly: prop.is_readonly,
                origin: FieldOrigin::PlatformProperty,
            });
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Shared helpers (pub(crate) so field_lookup can use them)
// ---------------------------------------------------------------------------

/// Map a plain-MDO `MetadataKind` to its [`MdoType`].
///
/// Returns `None` for register variants, tabular-section variants, and leaf
/// register-part kinds — they have their own dispatch paths.
pub(crate) fn mdo_type_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::CatalogRef | MetadataKind::CatalogObject => Some(MdoType::Catalog),
        MetadataKind::DocumentRef | MetadataKind::DocumentObject => Some(MdoType::Document),
        MetadataKind::EnumRef => Some(MdoType::Enum),
        MetadataKind::TaskRef => Some(MdoType::Task),
        MetadataKind::BusinessProcessRef => Some(MdoType::BusinessProcess),
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
            Some(MdoType::ExchangePlan)
        }
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            Some(MdoType::ChartOfAccounts)
        }
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => None,
    }
}

/// Map a register-flavoured receiver `MetadataKind` to its register [`MdoType`].
///
/// Returns `None` for non-register kinds and for leaf part kinds
/// (`RegisterDimension` / `RegisterResource` / `RegisterAttribute`).
pub(crate) fn register_parent_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::InformationRegisterRecordManager | MetadataKind::InformationRegisterRef => {
            Some(MdoType::InformationRegister)
        }
        MetadataKind::AccumulationRegisterRecordSet | MetadataKind::AccumulationRegisterRef => {
            Some(MdoType::AccumulationRegister)
        }
        MetadataKind::AccountingRegisterRef => Some(MdoType::AccountingRegister),
        MetadataKind::CalculationRegisterRef => Some(MdoType::CalculationRegister),
        _ => None,
    }
}

/// Split a `"Parent.Section"` identifier into `(parent, section)`.
/// Returns `None` if either half is empty.
pub(crate) fn split_parent_section(name: &str) -> Option<(&str, &str)> {
    let (parent, section) = name.split_once('.')?;
    if parent.is_empty() || section.is_empty() {
        return None;
    }
    Some((parent, section))
}

/// Look up an MDO in the visible configurations, latest-wins (extensions
/// override main).
pub(crate) fn find_mdo<'a>(
    configs: &'a [VisibleConfig],
    mdo_type: MdoType,
    name: &str,
) -> Option<&'a MetadataObject> {
    configs.iter().rev().find_map(|cfg| cfg.configuration.find_metadata_object(mdo_type, name))
}

/// Lower an [`AttributeType`] to a [`Ty`] through [`TyLoweringContext`].
pub(crate) fn attribute_type_to_ty(attr_type: &AttributeType) -> Ty {
    let type_ref = TypeRef::from_attribute_type(attr_type);
    TyLoweringContext::new().lower_type_ref(&type_ref)
}

/// Lower a register-part type, falling back to a symbolic
/// `MetadataKind::Register{Dimension,Resource,Attribute}` when `attr_type`
/// is absent.
pub(crate) fn register_part_ty(
    attr_type: Option<&AttributeType>,
    fallback_kind: MetadataKind,
    register_name: &Name,
    part_name: &str,
) -> Ty {
    match attr_type {
        Some(at) => attribute_type_to_ty(at),
        None => Ty::MetadataRef {
            kind: fallback_kind,
            name: Name::new(&format!("{}.{}", register_name.as_str(), part_name)),
        },
    }
}

/// Map an [`MdoType`] to its [`MdoTemplateKind`] for standard-attribute
/// classification. Returns `None` for types that have no standard-attribute
/// spec (Enum, ExternalDataSource, etc.).
pub(crate) fn mdo_template_kind_for(mdo_type: MdoType) -> Option<MdoTemplateKind> {
    match mdo_type {
        MdoType::Catalog => Some(MdoTemplateKind::Catalog),
        MdoType::Document => Some(MdoTemplateKind::Document),
        MdoType::BusinessProcess => Some(MdoTemplateKind::BusinessProcess),
        MdoType::Task => Some(MdoTemplateKind::Task),
        MdoType::ChartOfAccounts => Some(MdoTemplateKind::ChartOfAccounts),
        MdoType::ChartOfCharacteristicTypes => Some(MdoTemplateKind::ChartOfCharacteristicTypes),
        MdoType::ChartOfCalculationTypes => Some(MdoTemplateKind::ChartOfCalculationTypes),
        MdoType::ExchangePlan => Some(MdoTemplateKind::ExchangePlan),
        MdoType::InformationRegister => Some(MdoTemplateKind::InformationRegister),
        MdoType::AccumulationRegister => Some(MdoTemplateKind::AccumulationRegister),
        MdoType::AccountingRegister => Some(MdoTemplateKind::AccountingRegister),
        MdoType::CalculationRegister => Some(MdoTemplateKind::CalculationRegister),
        _ => None,
    }
}

/// Case-insensitive check whether `name` matches any standard attribute of
/// `template` in Object view.
///
/// Classify a metadata attribute by looking it up against the standard-attribute
/// spec for `template`.
///
/// Returns the spec when the name matches a standard attribute (so the caller
/// can read both `origin = StandardAttribute` and the `is_readonly` flag),
/// or `None` for user-defined attributes.
fn classify_attr<'a>(
    template: Option<MdoTemplateKind>,
    attr_name: &str,
) -> Option<&'a bsl_platform::StandardAttrSpec> {
    let tmpl = template?;
    let needle = attr_name.to_lowercase();
    standard_attributes_for(tmpl, ObjectView::Object).iter().find(|spec| {
        spec.kind.russian_name().to_lowercase() == needle
            || spec.kind.english_name().to_lowercase() == needle
    })
}

/// Insert `info` into `out` if neither its `name` nor its `name_en` has been
/// seen before. Prevents duplicate field entries when extensions re-declare
/// the same attribute.
fn push_unique(
    out: &mut Vec<FieldInfo>,
    seen: &mut std::collections::HashSet<Name>,
    info: FieldInfo,
) {
    if seen.contains(&info.name) {
        return;
    }
    if let Some(ref en) = info.name_en {
        if seen.contains(en) {
            return;
        }
    }
    seen.insert(info.name.clone());
    if let Some(ref en) = info.name_en {
        seen.insert(en.clone());
    }
    out.push(info);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::tabular_section::{TabularSection, TabularSectionAttribute};
    use bsl_metadata::{Attribute, Configuration};
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

    // -----------------------------------------------------------------------

    #[test]
    fn enumerate_unknown_receiver_returns_empty_vec() {
        let configs = wrap(Configuration::new("Test"));
        for ty in [Ty::Unknown, Ty::Number, Ty::String, Ty::Array, Ty::Undefined] {
            assert!(enumerate_fields(&configs, &ty).is_empty(), "no fields on {ty:?}");
        }
    }

    #[test]
    fn enumerate_this_object_coerces_to_object_metadata_ref() {
        // `Ty::ThisObject { Catalog, "Номенклатура" }` must enumerate the
        // same attributes as `CatalogObject.Номенклатура`.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog(
            "Номенклатура",
            vec![attr("Цена", None, AttributeType::Number { precision: 15, scale: 2 })],
        ));
        let configs = wrap(config);

        let this_obj =
            Ty::ThisObject { owner: (MdoType::Catalog, Name::new("Номенклатура")) };
        let fields = enumerate_fields(&configs, &this_obj);
        assert!(!fields.is_empty(), "ThisObject must coerce and enumerate fields");
        assert!(fields.iter().any(|f| f.name.as_str() == "Цена"), "Цена must appear");
    }

    #[test]
    fn enumerate_catalog_ref_includes_standard_and_user() {
        // A catalog with one standard attr (`Код`) and one user attr (`Цена`).
        // Standard attrs are pre-populated in `mdo.attributes` by the XML
        // parser; we just check that `origin` is classified correctly.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog(
            "Номенклатура",
            vec![
                // Pre-populated by xml_parser (mimicked here manually)
                attr("Код", Some("Code"), AttributeType::String { length: Some(9) }),
                attr("Цена", None, AttributeType::Number { precision: 15, scale: 2 }),
            ],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        assert!(!fields.is_empty());

        let code_field = fields.iter().find(|f| f.name.as_str() == "Код").expect("Код must appear");
        assert_eq!(code_field.origin, FieldOrigin::StandardAttribute);

        let price_field =
            fields.iter().find(|f| f.name.as_str() == "Цена").expect("Цена must appear");
        assert_eq!(price_field.origin, FieldOrigin::UserAttribute);
    }

    #[test]
    fn field_origin_classifies_standard_vs_user_attribute() {
        // Dedicated test for the `classify_attr` classification logic.
        let template = mdo_template_kind_for(MdoType::Catalog);
        assert!(classify_attr(template, "Код").is_some());
        assert!(classify_attr(template, "Code").is_some());
        assert!(classify_attr(template, "МойРеквизит").is_none());
        // Enum has no template → always None.
        let no_template = mdo_template_kind_for(MdoType::Enum);
        assert!(classify_attr(no_template, "Код").is_none());
    }

    #[test]
    fn standard_attributes_carry_readonly_from_platform_spec() {
        // `Ссылка` and `Предопределенный` / `ИмяПредопределенныхДанных` are
        // marked read-only in `bsl_platform::standard_mdo_attributes`. The
        // enumerator must surface that flag — otherwise the IDE
        // `[Только чтение]` marker and any read-only-write diagnostic
        // become incorrect.
        let mut cat = MetadataObject::new(MdoType::Catalog, "Справочник1");
        // Pre-populated standard attrs — same shape as the XML adapter pushes.
        cat.add_attribute(attr(
            "Ссылка",
            Some("Ref"),
            AttributeType::Ref {
                mdo_type: MdoType::Catalog, name: "Справочник1".to_string()
            },
        ));
        cat.add_attribute(attr("Предопределенный", Some("Predefined"), AttributeType::Boolean));
        cat.add_attribute(attr("ПометкаУдаления", Some("DeletionMark"), AttributeType::Boolean));
        cat.add_attribute(attr("МойРеквизит", None, AttributeType::Boolean));
        let mut config = Configuration::new("Test");
        config.add_metadata_object(cat);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("Справочник1"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        let by_name = |n: &str| fields.iter().find(|f| f.name.as_str() == n).cloned();
        assert!(
            by_name("Ссылка").expect("Ссылка").is_readonly,
            "Ссылка must be read-only per platform spec"
        );
        assert!(
            by_name("Предопределенный").expect("Предопределенный").is_readonly,
            "Предопределенный must be read-only"
        );
        // DeletionMark is writable in BSL — spec marks it is_readonly=false.
        assert!(!by_name("ПометкаУдаления").expect("ПометкаУдаления").is_readonly);
        // User-defined attributes are always writable.
        assert!(!by_name("МойРеквизит").expect("МойРеквизит").is_readonly);
    }

    #[test]
    fn enumerate_document_object_yields_standard_user_and_tabular_sections() {
        let mut ts = TabularSection::new(Uuid::new_v4(), "Товары");
        ts.set_attributes(vec![]);
        let mut doc = MetadataObject::new(MdoType::Document, "ПКО");
        doc.add_attribute(attr("Дата", Some("Date"), AttributeType::DateTime));
        doc.add_attribute(attr("МойРеквизит", None, AttributeType::Boolean));
        doc.add_tabular_section(ts);

        let mut config = Configuration::new("Test");
        config.add_metadata_object(doc);
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") };
        let fields = enumerate_fields(&configs, &receiver);

        let date_field =
            fields.iter().find(|f| f.name.as_str() == "Дата").expect("Дата must appear");
        assert_eq!(date_field.origin, FieldOrigin::StandardAttribute);

        let my_field = fields
            .iter()
            .find(|f| f.name.as_str() == "МойРеквизит")
            .expect("МойРеквизит must appear");
        assert_eq!(my_field.origin, FieldOrigin::UserAttribute);

        let ts_field =
            fields.iter().find(|f| f.name.as_str() == "Товары").expect("Товары TS must appear");
        assert_eq!(ts_field.origin, FieldOrigin::TabularSection);
        assert_eq!(
            ts_field.ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSection { parent: MdoType::Document },
                name: Name::new("ПКО.Товары"),
            }
        );
    }

    #[test]
    fn enumerate_tabular_section_row_yields_columns_and_line_number() {
        // Row with one custom column. `НомерСтроки` must be appended via
        // platform fall-through with `origin: PlatformProperty`.
        let mut ts = TabularSection::new(Uuid::new_v4(), "Услуги");
        ts.set_attributes(vec![TabularSectionAttribute::new(
            Uuid::new_v4(),
            "Количество",
            AttributeType::Number { precision: 15, scale: 3 },
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
        let fields = enumerate_fields(&configs, &receiver);

        let qty = fields
            .iter()
            .find(|f| f.name.as_str() == "Количество")
            .expect("Количество must appear");
        assert_eq!(qty.origin, FieldOrigin::TabularSectionRowColumn);
        assert_eq!(qty.ty, Ty::Number);

        let nr = fields
            .iter()
            .find(|f| f.name.as_str() == "НомерСтроки")
            .expect("НомерСтроки must appear via platform fall-through");
        assert_eq!(nr.origin, FieldOrigin::PlatformProperty);
        assert!(nr.is_readonly);
        assert_eq!(nr.ty, Ty::Number);
    }

    #[test]
    fn enumerate_information_register_record_includes_dimensions_resources_attributes() {
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
            vec![resource_typed("Количество", AttributeType::Number { precision: 15, scale: 3 })],
            vec![attribute_typed("Комментарий", AttributeType::String { length: Some(100) })],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRef,
            name: Name::new("РегистрСведений1"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        let dim = fields
            .iter()
            .find(|f| f.name.as_str() == "Справочник1")
            .expect("dimension must appear");
        assert_eq!(dim.origin, FieldOrigin::RegisterDimension);

        let res =
            fields.iter().find(|f| f.name.as_str() == "Количество").expect("resource must appear");
        assert_eq!(res.origin, FieldOrigin::RegisterResource);
        assert_eq!(res.ty, Ty::Number);

        let att = fields
            .iter()
            .find(|f| f.name.as_str() == "Комментарий")
            .expect("attribute must appear");
        assert_eq!(att.origin, FieldOrigin::RegisterAttribute);
        assert_eq!(att.ty, Ty::String);
    }

    #[test]
    fn enumerate_union_with_metadata_ref_arm_yields_fields() {
        // Receiver shape produced by `НайтиСтроки(...)` etc.:
        // `Union(MetadataRef.Row, Undefined)`. Enumerator must descend the
        // union and surface row columns from the live arm; `Undefined`
        // is skipped.
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
        let fields = enumerate_fields(&configs, &receiver);
        assert!(
            fields.iter().any(|f| f.name.as_str() == "Номенклатура"),
            "Union(MetadataRef.Row, Undefined) must surface row columns"
        );
    }

    #[test]
    fn extension_overrides_main_on_collision() {
        // Extension redeclares `Номенклатура.Цена` as `String`; the enumerator
        // must return the extension's field (latest-wins).
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
        let fields = enumerate_fields(&configs, &receiver);
        let цена = fields.iter().find(|f| f.name.as_str() == "Цена").expect("Цена must appear");
        assert_eq!(цена.ty, Ty::String, "extension type must win over main config");
    }
}
