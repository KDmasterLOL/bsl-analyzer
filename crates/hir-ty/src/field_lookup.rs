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
//! - **`Ty::MetadataRef { kind: TabularSectionRow, "Parent.Section" }`** —
//!   attribute lookup on a single row; uses the section's own attribute
//!   list.
//! - **`Ty::MetadataRef { kind: TabularSection, "Parent.Section" }`** —
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
/// - `TabularSectionRow` decodes the `"Parent.Section"` name and walks the
///   section's own attribute list;
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
        return lookup_on_mdo(mdo, mdo_name, field_name);
    }

    if kind == MetadataKind::TabularSectionRow {
        let (parent, section) = split_parent_section(mdo_name.as_str())?;
        return lookup_on_tabular_row(configs, parent, section, field_name);
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
fn lookup_on_mdo(mdo: &MetadataObject, mdo_name: &Name, field_name: &Name) -> Option<FieldInfo> {
    for attr in &mdo.attributes {
        if matches_bilingual(&attr.name, attr.name_en.as_deref(), field_name) {
            return Some(FieldInfo { ty: attribute_type_to_ty(&attr.attr_type) });
        }
    }

    for ts in &mdo.tabular_sections {
        if matches_bilingual(ts.name(), ts.name_en(), field_name) {
            let qualified = Name::new(&format!("{}.{}", mdo_name.as_str(), ts.name()));
            return Some(FieldInfo {
                ty: Ty::MetadataRef { kind: MetadataKind::TabularSection, name: qualified },
            });
        }
    }

    None
}

/// Walk the attributes of a tabular section identified by
/// `"Parent.Section"`.
///
/// The parent MDO type is not encoded in the `Ty::MetadataRef` name, so we
/// probe each candidate that can legitimately own a tabular section
/// (Catalog, Document, BusinessProcess, Task, ChartOf*). Iteration stops
/// on the first match; configurations rarely host the same MDO name across
/// categories, and per-name collisions still produce a well-defined answer
/// (the first candidate in list order wins — deterministic, not
/// configuration-order-dependent).
fn lookup_on_tabular_row(
    configs: &[VisibleConfig],
    parent_name: &str,
    section_name: &str,
    field_name: &Name,
) -> Option<FieldInfo> {
    const CANDIDATES: &[MdoType] = &[
        MdoType::Catalog,
        MdoType::Document,
        MdoType::BusinessProcess,
        MdoType::Task,
        MdoType::ChartOfCharacteristicTypes,
        MdoType::ChartOfAccounts,
        MdoType::ChartOfCalculationTypes,
    ];
    for &candidate in CANDIDATES {
        let Some(mdo) = find_mdo(configs, candidate, parent_name) else { continue };
        let Some(ts) = mdo.find_tabular_section(section_name) else { continue };
        for attr in ts.attributes() {
            if matches_bilingual(attr.name(), attr.name_en(), field_name) {
                return Some(FieldInfo { ty: attribute_type_to_ty(attr.attr_type()) });
            }
        }
        return None;
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
        | MetadataKind::TabularSection
        | MetadataKind::TabularSectionRow => None,
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
/// `to_lowercase` is used intentionally instead of `eq_ignore_ascii_case`:
/// BSL identifiers are Cyrillic, and ASCII-case-folding would leave
/// `"Цена"` and `"цена"` distinct.
fn matches_bilingual(russian: &str, english: Option<&str>, target: &Name) -> bool {
    let want = target.as_str().to_lowercase();
    russian.to_lowercase() == want || english.map(|en| en.to_lowercase() == want).unwrap_or(false)
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
        // `Ссылка.Код` — the XML loader pushes `StandardAttributeKind::Code`
        // into `mdo.attributes` with `AttributeType::String { length }`.
        // FieldLookup must therefore resolve `Код` with the same codepath
        // it uses for custom attributes — no special-case branch.
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
        // `Ty::MetadataRef { TabularSection, "ПКО.Товары" }` so a chained
        // `Д.Товары[0].Количество` can continue resolving through
        // `TabularSectionRow` in the row-attribute test below.
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
                kind: MetadataKind::TabularSection, name: Name::new("ПКО.Товары")
            }
        );
    }

    #[test]
    fn field_lookup_tabular_row_attribute() {
        // Row attribute: starting from `Ty::MetadataRef { TabularSectionRow,
        // "ПКО.Товары" }`, the adapter decodes the name, scans candidate
        // parent MDO types (Document here), finds the section and lowers
        // its attribute. Closes the chain `Д.Товары[0].Количество → Number`
        // that M4 narrowing will build on.
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
            kind: MetadataKind::TabularSectionRow,
            name: Name::new("ПКО.Товары"),
        };
        let info = lookup_field(&configs, &receiver, &Name::new("Количество"))
            .expect("row attribute Количество resolves to Number");
        assert_eq!(info.ty, Ty::Number);
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
