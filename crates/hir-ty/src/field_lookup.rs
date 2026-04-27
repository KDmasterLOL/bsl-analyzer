//! Field lookup on a typed receiver.
//!
//! `FieldLookup::resolve(receiver_ty, field_name)` answers the question
//! "given `x: receiver_ty`, what does `x.field_name` evaluate to?"
//!
//! This is the semantic complement to [`crate::method_lookup`]: methods go
//! through `PlatformData`, fields go through the XML-derived
//! [`bsl_metadata::Configuration`]. Keeping the two adapters separate keeps
//! each source narrowly focused — platform methods never live in
//! configuration XML, MDO attributes never live in `platform_data.json`.
//!
//! # Coverage
//!
//! - **`Ty::MetadataRef { kind, name }`** where `kind` is one of
//!   `CatalogRef`, `CatalogObject`, `DocumentRef`, `DocumentObject`,
//!   `EnumRef`, `TaskRef`, `BusinessProcessRef` — lookup against the MDO
//!   identified by `(kind → MdoType, name)`. Custom attributes **and**
//!   standard attributes (`Ссылка`, `Код`, `Наименование`, …) both
//!   resolve through the same `mdo.attributes` vec because the XML
//!   parser pre-populates standard attributes during loading (see
//!   `crates/bsl-metadata/src/xml_parser/standard_attributes.rs`).
//!   Tabular-section names resolve to
//!   `Ty::MetadataRef { TabularSection, "Parent.Section" }` so chained
//!   access (`Д.Товары.Количество`) can continue through
//!   `TabularSectionRow` fields.
//! - **`Ty::MetadataRef { kind: TabularSectionRow { parent }, "Parent.Section" }`** —
//!   attribute lookup on a single row; uses the section's own attribute
//!   list. `parent` pins the MDO flavour that owns the section, so
//!   `Catalog "X".Товары` and `Document "X".Товары` resolve independently
//!   without probing a candidate list.
//! - **`Ty::MetadataRef { kind: TabularSection { parent }, "Parent.Section" }`** —
//!   `None` for field access. The section value is collection-shaped
//!   (iteration); attribute access on the collection itself doesn't
//!   exist in BSL. The 18 platform methods (`Добавить`, `НайтиСтроки`,
//!   `Количество`, …) are served by [`crate::method_lookup`] through
//!   `PlatformData["Tabular section"]`, which rebinds the generic
//!   `"Строка табличной части"` return to a `TabularSectionRow`
//!   receiver so chained calls keep resolving.
//!
//! # Deferred (M4+)
//!
//! - **`Ty::ObjectManager { kind, name }` predefined items / enum values.**
//!   Predefined items and enum values are plain references to their
//!   enclosing MDO (`EnumRef` / `CatalogRef`); lookup needs a manager-side
//!   adapter. Not in scope for M3.
//! - **Platform object fields** (`Ty::PlatformObject`, `Ty::ValueTable`,
//!   …). BSL platform objects only expose methods on their instances;
//!   "fields" like `ПараметрыСеанса.Х` model global access, not type
//!   membership — they belong in the resolver path.

use bsl_metadata::{AttributeType, MdoType, MetadataObject};
use hir_def::configs::VisibleConfig;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;

use crate::lower::TyLoweringContext;

/// Result of a successful field lookup.
///
/// Carries only the lowered type today. Future additions (docs, nullability,
/// owning MDO) should extend this struct rather than widening the return
/// signature of [`lookup_field`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldInfo {
    /// Type of the field after lowering its `AttributeType` through
    /// [`TyLoweringContext`].
    pub ty: Ty,
    /// `true` when the field is a platform property marked `"Только чтение"`
    /// in HBK. MDO attributes are always read-write in BSL, so this is
    /// `false` for any MetadataRef-backed `FieldInfo`. Consumed by the
    /// `ReadOnlyPropertyAssignment` diagnostic.
    pub is_readonly: bool,
}

/// Resolve a field access on a typed receiver.
///
/// Returns `None` when:
/// - the receiver type has no backing MDO (`Ty::Number`, `Ty::Unknown`,
///   managers, collectives, platform objects);
/// - the MDO exists but does not declare the requested attribute or
///   tabular section;
/// - the `Ty::MetadataRef` points at an MDO kind whose field lookup is
///   deferred (registers).
///
/// `configs` should be the visible configurations for the receiver's file
/// (`db.configurations(file_id)`). Passed as a slice so the adapter stays
/// db-free and unit-testable; iteration order follows the slice, with
/// later configurations winning on name collisions (extensions override
/// main configuration).
pub fn lookup_field(
    configs: &[VisibleConfig],
    receiver_ty: &Ty,
    field_name: &Name,
) -> Option<FieldInfo> {
    // `Ty::ThisObject` is the `ЭтотОбъект` / `ThisObject` receiver.
    // The [`crate::this_object`] helper rewrites it to its matching
    // `Ty::MetadataRef { *Object, name }` shape for MDO kinds that
    // have an `*Object` companion (Catalog, Document, ExchangePlan,
    // ChartOfAccounts). Doing the coercion here keeps downstream
    // dispatch and every helper below ignorant of the variant.
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    match receiver_ty {
        // MetadataRef receivers own the MDO-attribute surface and MUST NOT
        // be routed through platform-property lookup — the `Ссылка` /
        // `Код` / `Наименование` fields for `CatalogRef.X` live in the
        // XML `Configuration`, not `platform_data.json`. Platform
        // properties only apply to generic receivers (`Ty::PlatformObject`,
        // value types, primitives).
        Ty::MetadataRef { kind, name } => lookup_metadata_ref(configs, *kind, name, field_name),
        // Every other receiver type delegates to the platform-property
        // adapter. `lookup_platform_property` itself decides whether the
        // shape is supported (primitives return `None` — BSL exposes no
        // declared properties on Число/Строка/Булево/Дата), so we can
        // safely call it for any non-MetadataRef receiver.
        //
        // `UnresolvedField` semantics are preserved: the diagnostic in
        // `infer_field_lookup` only fires for MetadataRef receivers, so
        // a `None` here for a platform type keeps the existing
        // "don't over-report on platform opaque" behaviour.
        _ => crate::platform_property_lookup::lookup_platform_property(receiver_ty, field_name)
            .map(|res| FieldInfo { ty: res.return_ty, is_readonly: res.is_readonly }),
    }
}

/// Dispatch entry for `Ty::MetadataRef` receivers.
///
/// Splits into three flavours:
/// - kinds backed by a plain MDO (Catalog/Document/Enum/Task/BusinessProcess/
///   ExchangePlan/ChartOfAccounts) look up the MDO and walk attributes +
///   tabular sections;
/// - register receiver kinds (`*RecordManager`, `*RecordSet`, `*Ref`) route
///   through [`lookup_on_register`] against `Configuration.registers` —
///   registers live in a separate vec with a per-part shape
///   (dimensions / resources / attributes) rather than the flat attribute
///   list used by regular MDOs;
/// - `TabularSectionRow { parent }` decodes the `"Parent.Section"` name and
///   walks the section's own attribute list using `parent` to disambiguate
///   identically named MDOs across categories;
/// - `TabularSection` and the leaf register-part kinds
///   (`RegisterDimension` / `RegisterResource` / `RegisterAttribute`) fall
///   through to `None`: they are collection- or leaf-shaped, so field
///   access on them goes through the row receiver or stays unresolved.
fn lookup_metadata_ref(
    configs: &[VisibleConfig],
    kind: MetadataKind,
    mdo_name: &Name,
    field_name: &Name,
) -> Option<FieldInfo> {
    if let Some(mdo_type) = mdo_type_for_kind(kind) {
        let mdo = find_mdo(configs, mdo_type, mdo_name.as_str())?;
        return lookup_on_mdo(mdo, mdo_type, mdo_name, field_name);
    }

    if let Some(register_parent) = register_parent_for_kind(kind) {
        return lookup_on_register(configs, register_parent, mdo_name, field_name);
    }

    if let MetadataKind::TabularSectionRow { parent } = kind {
        let (parent_name, section) = split_parent_section(mdo_name.as_str())?;
        return lookup_on_tabular_row(configs, parent, parent_name, section, field_name);
    }

    None
}

/// Walk an MDO's attribute list and tabular sections.
///
/// Standard attributes (`Ссылка`, `Код`, `Наименование`, `Дата`, …) are
/// already present in `mdo.attributes`: the XML parser pushes them in via
/// `add_*_standard_attributes`, so this lookup handles custom and
/// standard attributes uniformly without re-deriving
/// [`bsl_metadata::StandardAttributeKind`] here.
///
/// `parent_mdo_type` is threaded through because a tabular-section hit
/// promotes the receiver to `Ty::MetadataRef { TabularSection { parent }, … }`;
/// threading `parent` here is what disambiguates `Catalog "X".Товары`
/// from `Document "X".Товары` at the row-lookup step.
fn lookup_on_mdo(
    mdo: &MetadataObject,
    parent_mdo_type: MdoType,
    mdo_name: &Name,
    field_name: &Name,
) -> Option<FieldInfo> {
    // Lowercase the needle once: the haystack strings (attr names, TS
    // names) must still be lowercased per iteration because they live in
    // the metadata, but the needle is fixed for this whole lookup.
    let needle = field_name.as_str().to_lowercase();

    for attr in &mdo.attributes {
        if matches_bilingual(&attr.name, attr.name_en.as_deref(), &needle) {
            return Some(FieldInfo {
                ty: attribute_type_to_ty(&attr.attr_type),
                is_readonly: false,
            });
        }
    }

    for ts in &mdo.tabular_sections {
        if matches_bilingual(ts.name(), ts.name_en(), &needle) {
            let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
            return Some(FieldInfo {
                ty: Ty::MetadataRef {
                    kind: MetadataKind::TabularSection { parent: parent_mdo_type },
                    name: qualified,
                },
                is_readonly: false,
            });
        }
    }

    None
}

/// Walk the attributes of a tabular section identified by
/// `parent` + `"Parent.Section"`.
///
/// The parent [`MdoType`] is supplied by the `TabularSectionRow` variant
/// payload, so no candidate-probing is needed: the lookup targets exactly
/// one MDO category. Under a config containing `Catalog "X"` **and**
/// `Document "X"` each with a tabular section `"Товары"`, the two
/// `TabularSectionRow` types are structurally distinct (they differ in
/// `parent`) and resolve to their own attribute lists.
fn lookup_on_tabular_row(
    configs: &[VisibleConfig],
    parent: MdoType,
    parent_name: &str,
    section_name: &str,
    field_name: &Name,
) -> Option<FieldInfo> {
    let mdo = find_mdo(configs, parent, parent_name)?;
    let ts = mdo.find_tabular_section(section_name)?;
    let needle = field_name.as_str().to_lowercase();
    for attr in ts.attributes() {
        if matches_bilingual(attr.name(), attr.name_en(), &needle) {
            return Some(FieldInfo {
                ty: attribute_type_to_ty(attr.attr_type()),
                is_readonly: false,
            });
        }
    }
    // Fall through to platform row properties (`НомерСтроки` / `LineNumber`).
    // HBK ships these under `type_name = "Line of a tabular section"`.
    // Custom XML attributes intentionally win on name collisions because
    // they are checked first above — matches the convention that a
    // user-declared MDO attribute always wins over a platform default.
    let prop = crate::platform_property_lookup::lookup_platform_property_by_type(
        "Line of a tabular section",
        field_name,
    )?;
    Some(FieldInfo { ty: prop.return_ty, is_readonly: prop.is_readonly })
}

/// Map a `MetadataKind` to the [`MdoType`] used for `find_metadata_object`.
///
/// Returns `None` for:
/// - register variants (receivers — stored in `Configuration::registers`,
///   routed through [`register_parent_for_kind`] + [`lookup_on_register`]);
/// - `TabularSection` / `TabularSectionRow` (their parent MDO type is not
///   knowable from the kind alone — resolved via the `"Parent.Section"`
///   name scan in [`lookup_on_tabular_row`]);
/// - leaf register-part kinds (`RegisterDimension` / `RegisterResource` /
///   `RegisterAttribute`) — opaque symbolic fallbacks with no further
///   field surface.
fn mdo_type_for_kind(kind: MetadataKind) -> Option<MdoType> {
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

/// Map a register-flavoured receiver `MetadataKind` to the corresponding
/// register [`MdoType`].
///
/// Covers the six register receivers that actually own a per-register
/// field surface (RecordManager / RecordSet / the four Ref variants).
/// Leaf part kinds (`RegisterDimension` / `RegisterResource` /
/// `RegisterAttribute`) are deliberately excluded: they carry their
/// `parent` explicitly but they are not lookup targets — field access
/// on a part value returns `None`, and the register is recovered only
/// for provenance (hover / rename).
fn register_parent_for_kind(kind: MetadataKind) -> Option<MdoType> {
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

/// Resolve a field access on a register receiver.
///
/// Registers live in `Configuration::registers` (separate from the
/// generic `metadata_objects` vec used by Catalog/Document/…) and split
/// their per-register surface into three buckets: dimensions, resources,
/// attributes. The lookup walks them in order; iteration stops at the
/// first hit, and the resulting [`Ty`] is either the lowered
/// [`bsl_metadata::AttributeType`] (when the XML parser populated
/// `attr_type`) or the symbolic
/// [`MetadataKind::RegisterDimension`] / `::RegisterResource` /
/// `::RegisterAttribute` fallback, with the register and part name
/// encoded as `"Register.Part"` for downstream provenance.
///
/// `parent` pins the register flavour so [`Configuration::find_register_by_type_and_name`]
/// can filter on it — two register families with an identically named
/// register (e.g. an InformationRegister and an AccumulationRegister
/// both called `"X"`) stay disambiguated at the lookup level.
///
/// [`Configuration::find_register_by_type_and_name`]: bsl_metadata::Configuration::find_register_by_type_and_name
fn lookup_on_register(
    configs: &[VisibleConfig],
    parent: MdoType,
    register_name: &Name,
    field_name: &Name,
) -> Option<FieldInfo> {
    let register = find_register(configs, parent, register_name.as_str())?;
    let needle = field_name.as_str().to_lowercase();

    // Dimensions — bsl-metadata's `Dimension` has no `name_en`, so we
    // only match the Russian name.
    for dim in register.dimensions() {
        if matches_bilingual(dim.name(), None, &needle) {
            return Some(FieldInfo {
                ty: register_part_ty(
                    dim.attr_type(),
                    MetadataKind::RegisterDimension { parent },
                    register_name,
                    dim.name(),
                ),
                is_readonly: false,
            });
        }
    }

    for res in register.resources() {
        if matches_bilingual(res.name(), res.name_en(), &needle) {
            return Some(FieldInfo {
                ty: register_part_ty(
                    res.attr_type(),
                    MetadataKind::RegisterResource { parent },
                    register_name,
                    res.name(),
                ),
                is_readonly: false,
            });
        }
    }

    for attr in register.attributes() {
        if matches_bilingual(attr.name(), attr.name_en(), &needle) {
            return Some(FieldInfo {
                ty: register_part_ty(
                    attr.attr_type(),
                    MetadataKind::RegisterAttribute { parent },
                    register_name,
                    attr.name(),
                ),
                is_readonly: false,
            });
        }
    }

    None
}

/// Lower a register-part type, falling back to a symbolic
/// `MetadataKind::Register{Dimension,Resource,Attribute}` when
/// `attr_type` is absent.
///
/// The XML loader populates `attr_type` for every part it parses, but
/// some downstream constructors (notably tests and manual builders)
/// leave it as `None`. Instead of lying to callers with `Ty::Unknown`,
/// the fallback keeps provenance — the name encodes `"Register.Part"`
/// and the variant payload pins the register flavour, so hover and
/// rename tooling still have something to display.
fn register_part_ty(
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

/// Look up a register in the visible configurations, latest-wins.
///
/// Same "extensions override main" order as [`find_mdo`]: iterate the
/// visible-configs list in reverse so the last one to redeclare a
/// register by `(MdoType, Name)` is the one returned.
fn find_register<'a>(
    configs: &'a [VisibleConfig],
    parent: MdoType,
    name: &str,
) -> Option<&'a bsl_metadata::Register> {
    configs
        .iter()
        .rev()
        .find_map(|cfg| cfg.configuration.find_register_by_type_and_name(parent, name))
}

/// Look up an MDO in the visible configurations, latest-wins.
///
/// `VisibleConfig` order is "main first, extensions after" per the
/// `ConfigsDatabase` contract. Extensions overriding a main MDO must win,
/// so we iterate in reverse.
fn find_mdo<'a>(
    configs: &'a [VisibleConfig],
    mdo_type: MdoType,
    name: &str,
) -> Option<&'a MetadataObject> {
    configs.iter().rev().find_map(|cfg| cfg.configuration.find_metadata_object(mdo_type, name))
}

/// Lower an `AttributeType` into the semantic `Ty` through
/// [`TyLoweringContext`]. Single call site keeps XML-derived types on the
/// same path as JSDoc / `Новый` sources.
fn attribute_type_to_ty(attr_type: &AttributeType) -> Ty {
    let type_ref = TypeRef::from_attribute_type(attr_type);
    TyLoweringContext::new().lower_type_ref(&type_ref)
}

/// Case-insensitive match against a Russian name and optional English
/// alias.
///
/// `target_lowercase` is the pre-lowercased needle — callers lowercase
/// the field name once before the loop so this function only allocates
/// for the haystack side. `to_lowercase` is used intentionally instead
/// of `eq_ignore_ascii_case`: BSL identifiers are Cyrillic, and
/// ASCII-case-folding would leave `"Цена"` and `"цена"` distinct.
fn matches_bilingual(russian: &str, english: Option<&str>, target_lowercase: &str) -> bool {
    russian.to_lowercase() == target_lowercase
        || english.map(|en| en.to_lowercase() == target_lowercase).unwrap_or(false)
}

/// Split a `"Parent.Section"` identifier. Returns `None` if either half
/// is empty (guards against `"."` / `".X"` / `"X."` shapes).
fn split_parent_section(name: &str) -> Option<(&str, &str)> {
    let (parent, section) = name.split_once('.')?;
    if parent.is_empty() || section.is_empty() {
        return None;
    }
    Some((parent, section))
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
        // M4 Task 2b regression: the new `ExchangePlanRef` /
        // `ChartOfAccountsRef` kinds must flow through
        // `mdo_type_for_kind → find_metadata_object → attributes` with the
        // same shape as Catalog/Document. One probe per flavour keeps the
        // test deliberately narrow — the exhaustive lowering matrix lives
        // in `hir-ty::lower::tests::metadata_kind_exchange_plan_and_chart_of_accounts_lower_bilingual`.
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
        // Custom attribute on a catalog: `Номенклатура.Цена` must lower
        // `AttributeType::Number { 15, 2 }` through TypeRef → Ty::Number.
        // This is the baseline behaviour — prove the `configs →
        // find_metadata_object → AttributeType → Ty` pipeline end-to-end
        // before layering standard attributes or tabular sections on top.
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
        // Proves FieldLookup treats a standard attribute uniformly with
        // custom ones — no special-case branch — once the XML loader has
        // pushed `StandardAttributeKind::Code` into `mdo.attributes`. The
        // loader itself is covered by `xml_parser::mod::tests::
        // test_catalog_hierarchical_standard_attributes` and friends, so
        // this test only owns the "FieldLookup path" half of the pipeline.
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

        // English-alias lookup via the same bilingual check — proves
        // `matches_bilingual` isn't ASCII-only.
        let info_en = lookup_field(&configs, &receiver, &Name::new("Code"))
            .expect("Code (en) resolves through bilingual match");
        assert_eq!(info_en.ty, Ty::String);
    }

    #[test]
    fn field_lookup_tabular_section() {
        // `Д.Товары` (on `ДокументСсылка.ПКО`) must become
        // `Ty::MetadataRef { TabularSection { parent: Document }, "ПКО.Товары" }`
        // so a chained `Д.Товары[0].Количество` can continue resolving
        // through `TabularSectionRow` in the row-attribute test below.
        // The `parent` MdoType carried by the variant is what keeps the
        // row-lookup step free of candidate-probing.
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
        // Row attribute: starting from `Ty::MetadataRef {
        // TabularSectionRow { parent: Document }, "ПКО.Товары" }`, the
        // adapter decodes the name and targets exactly one MDO (Document
        // ПКО) using the `parent` payload — no candidate-probing. Closes
        // the chain `Д.Товары[0].Количество → Number` that M4 narrowing
        // will build on.
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
        // Regression guard for the Codex MAJOR finding: a configuration
        // with `Catalog "X"` and `Document "X"` both carrying an
        // identically-named tabular section `"Товары"` (with different
        // attribute types) must resolve to the right attribute under each
        // receiver. Before the MdoType-in-kind refactor the previous
        // candidate-probe silently picked `Catalog` first and returned
        // wrong types for `Document "X"` rows.
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
        // Standard row property `НомерСтроки` lives in
        // `PlatformData["Line of a tabular section"]`, not in the MDO's
        // XML. The fall-through after `ts.attributes()` lets it resolve
        // even when the row carries no custom attributes.
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
        // If a user names a custom row attribute the same as a standard
        // platform property (`НомерСтроки`), the user's declaration
        // wins — XML attributes are checked first, the platform
        // fall-through only fires on a miss. Mirrors how MDO custom
        // attributes always win over platform standard ones.
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
        // Receivers without a backing MDO never resolve. Guards the
        // fall-through branches in `lookup_field` so we don't accidentally
        // hand out types for primitives / unions / managers.
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
        // A real MDO without the requested attribute must return None —
        // not a fabricated Ty::Unknown FieldInfo. Inference falls back to
        // Ty::Unknown at the call site; adapters should not pre-swallow
        // that distinction (M4 will want to emit an UnresolvedField
        // diagnostic here, and needs `None` as the precondition).
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
        // No register by that name lives in the config — lookup must
        // return None rather than panic or silently walk the MDO vec.
        // Keeps the "fail honestly" contract that
        // `lookup_field_unresolved_on_known_receiver` depends on to emit
        // `UnresolvedField` diagnostics only when the receiver is fully
        // resolved.
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
        // Baseline: an InformationRegister dimension with a parsed
        // `attr_type` must resolve to the lowered concrete `Ty`. This is
        // the "happy path" the XML loader sets up for every register
        // dimension it parses, so the symbolic fallback only kicks in
        // for builder-constructed or partially-parsed registers.
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
        // Resources live on AccumulationRegister; exercise the per-family
        // routing by using `AccumulationRegisterRecordSet` as the
        // receiver. `Количество` is the classic Accum resource.
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
        // Attributes are the InformationRegister's custom columns
        // (distinct from dimensions / resources). Exercise via the
        // `InformationRegisterRecordManager` receiver.
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
        // When `attr_type` is None (builder constructed the part without
        // calling `set_attr_type`, or the XML parser hit an unknown type
        // token), the lookup returns the symbolic
        // `RegisterDimension { parent }` variant with a `"Register.Part"`
        // name. Keeps provenance alive so downstream tooling can still
        // say "dimension of InformationRegister X".
        let mut config = Configuration::new("Test");
        // `register_with` uses the dimension builder without calling
        // `set_attr_type` — `attr_type` stays None.
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
        // One register per flavour; pick the kind that actually surfaces
        // the per-family routing (Accounting / Calculation only have Ref
        // variants). Typed resources on each so we assert the lowered
        // concrete Ty.
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
        // Leaf part kinds (`RegisterDimension` / `::Resource` / `::Attribute`)
        // are opaque symbolic fallbacks — field access on them returns
        // None. Register name is irrelevant; the point is that the
        // symbolic variant doesn't accidentally re-enter the lookup
        // pipeline.
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
        // A receiver kind must target a register of the matching flavour.
        // Declaring an `InformationRegister` named "X" while the receiver
        // is `AccumulationRegisterRef("X")` must not fold the two
        // families together: `find_register_by_type_and_name` filters on
        // `(MdoType, name)`, so this stays `None`.
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
        // Main config declares `Номенклатура.Цена: Number`; an extension
        // redeclares it as `String`. The resolver iterates main→ext, and
        // the extension must win — matches `ConfigsDatabase` semantics
        // where extensions override main-config MDOs. Pins the reverse
        // iteration inside `find_mdo`.
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
        // Parity with `field_lookup_extension_wins_on_collision` but for
        // registers: main config declares `РегистрСведений1.R: Number`,
        // an extension redeclares the resource as `String`. `find_register`
        // iterates `configs.iter().rev()`, so the extension must win —
        // registers live in a separate `Configuration.registers` vec and
        // need their own override-order guard.
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
        // Guards the name-parsing helper that the TabularSectionRow path
        // depends on. A malformed `"Parent.Section"` must bail to None
        // rather than producing a half-valid lookup.
        assert_eq!(split_parent_section("ПКО.Товары"), Some(("ПКО", "Товары")));
        assert_eq!(split_parent_section("ПКО"), None);
        assert_eq!(split_parent_section(""), None);
        assert_eq!(split_parent_section("."), None);
        assert_eq!(split_parent_section("ПКО."), None);
        assert_eq!(split_parent_section(".Товары"), None);
    }
}
