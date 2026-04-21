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
//!   `None`. The section value is collection-shaped (iteration, `Добавить`,
//!   `НайтиСтроки`); field access on the collection itself resolves via
//!   `MethodLookup` once we ship a `PlatformObject("TabularSection")`
//!   fallback. For now the chain continues through indexing / iteration
//!   to a `TabularSectionRow`.
//!
//! # Deferred (M4+)
//!
//! - **Register refs / record-sets / record-managers.** `MetadataKind`
//!   already knows the register variants, but the `Configuration` stores
//!   registers in a separate `registers` vec with a different per-type
//!   shape (dimensions/resources/attributes instead of one `attributes`
//!   list). A dedicated adapter is tracked for M4.
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
    match receiver_ty {
        Ty::MetadataRef { kind, name } => lookup_metadata_ref(configs, *kind, name, field_name),
        _ => None,
    }
}

/// Dispatch entry for `Ty::MetadataRef` receivers.
///
/// Splits into two flavours:
/// - kinds backed by a plain MDO (Catalog/Document/Enum/Task/BusinessProcess)
///   look up the MDO and walk attributes + tabular sections;
/// - `TabularSectionRow { parent }` decodes the `"Parent.Section"` name and
///   walks the section's own attribute list using `parent` to disambiguate
///   identically named MDOs across categories;
/// - registers and `TabularSection` fall through to `None` per the
///   deferred-gap documentation on the module.
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
            return Some(FieldInfo { ty: attribute_type_to_ty(&attr.attr_type) });
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
            return Some(FieldInfo { ty: attribute_type_to_ty(attr.attr_type()) });
        }
    }
    None
}

/// Map a `MetadataKind` to the [`MdoType`] used for `find_metadata_object`.
///
/// Returns `None` for:
/// - register variants (stored in `Configuration::registers`, different shape);
/// - `TabularSection` / `TabularSectionRow` (their parent MDO type is not
///   knowable from the kind alone — resolved via the `"Parent.Section"`
///   name scan in [`lookup_on_tabular_row`]).
fn mdo_type_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::CatalogRef | MetadataKind::CatalogObject => Some(MdoType::Catalog),
        MetadataKind::DocumentRef | MetadataKind::DocumentObject => Some(MdoType::Document),
        MetadataKind::EnumRef => Some(MdoType::Enum),
        MetadataKind::TaskRef => Some(MdoType::Task),
        MetadataKind::BusinessProcessRef => Some(MdoType::BusinessProcess),
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => None,
    }
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
    fn field_lookup_register_kinds_return_none_deferred() {
        // Registers have a different storage shape in `Configuration`;
        // FieldLookup will gain a dedicated adapter in M4. Pins the
        // deferred behaviour: register refs must return None instead of
        // panicking or silently walking an empty `Configuration.metadata_objects`.
        let configs = wrap(Configuration::new("Test"));
        let r = Ty::MetadataRef {
            kind: MetadataKind::AccumulationRegisterRef,
            name: Name::new("ТоварыНаСкладах"),
        };
        assert!(lookup_field(&configs, &r, &Name::new("Количество")).is_none());
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
