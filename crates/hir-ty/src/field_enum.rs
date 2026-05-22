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

use bsl_metadata::{AttributeType, MdoType, MetadataObject, RegisterPeriodicity};
use bsl_platform::{
    standard_attributes_for, MdoTemplateKind, ObjectView, PlatformData, StandardKind,
};
use hir_def::configs::VisibleConfig;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;

use crate::lower::metadata_resolver::ConfigsResolver;
use crate::lower::TyLoweringContext;

/// Where a field came from.
///
/// Lets IDE differentiate icons / sort priority: user-defined attributes
/// above standard ones, both above platform fall-throughs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldOrigin {
    StandardAttribute,
    UserAttribute,
    FormAttribute,
    MainFormAttribute,
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
    /// тип доменного значения, обёрнутого synthetic/platform wrapper'ом;
    /// `ty` остаётся фактическим типом доступа. Сейчас заполняется только
    /// для RegisterFilter-ключей. Когда появится второй wrapper-kind,
    /// рефакторить в FieldWrapperInfo { kind, value_ty }.
    pub value_ty: Option<Ty>,
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
    // Symmetric with `lookup_field`: `Ty::FormData{Structure | StructureWithCollection,
    // underlying: Some((mdo, name))}` projects to `MetadataRef{*Object, name}`
    // so the MDO's attributes/tabular sections are enumerable for hover and
    // completion on `Объект.|`. Without this projection `Type::fields()`
    // would return empty for FormData receivers, and IDE would only see the
    // bare `ДанныеФормыСтруктура` platform properties.
    let projected_form_data = crate::field_lookup::project_form_data_for_fields(receiver_ty);
    let receiver_ty = projected_form_data.as_ref().unwrap_or(receiver_ty);

    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let ty = coerced.as_ref().unwrap_or(receiver_ty);

    // `Ty::ThisManager` coerces to `Ty::ObjectManager`, which has no
    // enumerable attribute table here (managers only expose predefined
    // items via the `ManagerCollection` indexing path, not via field
    // lookup). The match below short-circuits non-MetadataRef receivers
    // to an empty Vec — same shape `Документы.ПКО` enumeration returned
    // pre-Step-J. Predefined-item enumeration is a separate enhancement.

    if let Some(infos) = enumerate_projection_fields(ty) {
        return infos;
    }

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
        return enumerate_mdo_fields(configs, *kind, mdo_type, name);
    }

    if let Some(parent) = register_parent_for_kind(*kind) {
        return enumerate_register_fields(configs, *kind, parent, name);
    }

    if let MetadataKind::RegisterFilter { parent } = kind {
        return enumerate_filter_fields(configs, *parent, name);
    }

    if let MetadataKind::TabularSectionRow { parent } = kind {
        let Some((parent_name, section_name)) = split_parent_section(name.as_str()) else {
            return Vec::new();
        };
        return enumerate_tabular_row_fields(configs, *parent, parent_name, section_name);
    }

    Vec::new()
}

/// Surface the SDBL projection columns of a
/// `Ty::QueryResultSelection { projection: Some(p) }` receiver as
/// IDE-visible fields.
///
/// Returns:
/// - `Some(fields)` — projection-typed receiver; the slice is the
///   per-column [`FieldInfo`]s in declaration order, marked read-only
///   (the cursor's columns are not assignable) with `UserAttribute`
///   origin so completion sorts them alongside other user-defined
///   columns.
/// - `Some(empty Vec)` — projection-typed receiver but the projection
///   carries no columns (`SELECT *` against an unresolved table, parse
///   error, …). Caller treats this as "no fields" — same as the
///   `Ty::Unknown` fallthrough below.
/// - `None` — receiver is anything else; caller falls through to the
///   existing union / MetadataRef / register dispatch.
///
/// Mirrors [`field_lookup::lookup_field_in_query_projection`] which
/// resolves a single named column on the same shape — the projection
/// arm is the IDE-completion sibling of the inference-time field
/// lookup.
fn enumerate_projection_fields(ty: &Ty) -> Option<Vec<FieldInfo>> {
    let projection = match ty {
        Ty::QueryResultSelection { projection: Some(p) } => p,
        // Phase H Slice 3 — projected `Ty::ValueTableRow` surfaces
        // its columns through the same completion / hover pipe as
        // `Ty::QueryResultSelection`, keeping the SDBL projection
        // visible after the `.Выгрузить()` chain.
        Ty::ValueTableRow { projection: Some(p) } => p,
        _ => return None,
    };
    let fields = projection
        .fields
        .iter()
        .map(|(name, field_ty)| FieldInfo {
            name: name.clone(),
            name_en: None,
            ty: field_ty.clone(),
            value_ty: None,
            is_readonly: true,
            origin: FieldOrigin::UserAttribute,
        })
        .collect();
    Some(fields)
}

/// Whether `kind` represents a register **record-set** receiver — the only
/// shape that exposes the synthetic `.Отбор` (Filter) field.
///
/// Record-manager / value-key (`*Ref`) kinds are excluded: their 1С runtime
/// surface does not expose `.Отбор`.
fn is_record_set_kind(kind: MetadataKind) -> bool {
    matches!(
        kind,
        MetadataKind::InformationRegisterRecordSet
            | MetadataKind::AccumulationRegisterRecordSet
            | MetadataKind::AccountingRegisterRecordSet
            | MetadataKind::CalculationRegisterRecordSet
    )
}

fn is_record_kind(kind: MetadataKind) -> bool {
    matches!(
        kind,
        MetadataKind::InformationRegisterRecord
            | MetadataKind::AccumulationRegisterRecord
            | MetadataKind::AccountingRegisterRecord
            | MetadataKind::CalculationRegisterRecord
    )
}

/// Push every HBK-declared platform property indexed under
/// `kind.platform_prefix()` into `out`, deduped by `seen`. Shared by
/// [`enumerate_mdo_fields`] and [`enumerate_register_fields`] — single
/// source of truth for the bilingual `rsplit('.').next()` alias rule and
/// the `to_resolution → FieldInfo` mapping.
///
/// **Presence-condition gating.** Standard-attribute names that
/// [`bsl_platform::standard_attributes_for`] knows about for this
/// `mdo_type` (`Код`/`HasCode`, `Номер`/`HasNumber`, `ЭтоГруппа` /
/// `Родитель` (`Hierarchical`), `Владелец`/`HasOwners`,
/// `Период`/`IsPeriodic`, …) are skipped: their visibility is a
/// configuration-dependent decision that lives in the spec and is
/// materialised by `bsl-metadata::xml_parser::standard_attributes`. The
/// HBK cascade must never push a presence-gated standard attribute,
/// because HBK has no knowledge of the gate and would surface (e.g.)
/// `Номер` on a document without a configured number length.
///
/// `ty_override` lets the caller override the platform-declared `Ty` for
/// specific property names. The register caller uses it to widen the
/// recorder property's type into a union of concrete document refs (see
/// [`recorder_union_ty`]); MDO caller passes a closure that always
/// returns `None`.
///
/// Pushed AFTER caller-specific entries so a real attribute / dimension
/// / standard attribute always wins on a name collision (`push_unique`
/// keeps the first push). This preserves the priority `mdo.attributes`
/// → tabular sections → HBK platform properties.
fn push_platform_prefix_properties(
    kind: MetadataKind,
    mdo_type: MdoType,
    mdo_name: &Name,
    out: &mut Vec<FieldInfo>,
    seen: &mut std::collections::HashSet<Name>,
    mut ty_override: impl FnMut(&str) -> Option<Ty>,
) {
    let Some(prefix) = kind.platform_prefix() else {
        return;
    };
    let spec_names = standard_attribute_names_for(mdo_type);
    for prop in PlatformData::instance().get_manager_properties(prefix) {
        let en_tail =
            prop.english_name.as_str().rsplit('.').next().unwrap_or(prop.english_name.as_str());
        if name_in_spec(&spec_names, prop.name.as_str(), en_tail) {
            // Spec owns this name's presence — defer to mdo.attributes
            // (which `xml_parser/standard_attributes` populates per
            // `PresenceCondition`). If the spec says "absent for this
            // config", `mdo.attributes` is empty and the cascade must
            // honour that absence, not paper over it from HBK.
            continue;
        }
        let res = crate::platform_property_lookup::to_resolution(prop);
        // HBK declares self-typed properties (`ЭтотОбъект` →
        // `ДокументОбъект`, `Ссылка` → `ДокументСсылка`, …) with the
        // base platform-type name, not the composite `<Prefix>.<MDO>`
        // shape. `to_resolution` therefore yields a generic
        // `Ty::PlatformObject(base)`, which kills chain typing:
        // `Док.ЭтотОбъект.Записать()` would not see `Записать` because
        // the receiver type lost its MDO anchor. Specialize the
        // self-base name back to a concrete `MetadataRef` pinned to
        // this receiver's `mdo_name` so the chain stays typed.
        let specialized = specialize_self_ref_ty(mdo_type, mdo_name, &res.return_ty);
        let ty = ty_override(prop.name.as_str()).or(specialized).unwrap_or(res.return_ty);
        let info = FieldInfo {
            name: Name::new(prop.name.as_str()),
            // english_name shape: `<Type>.<Name>.<Property>` (composite).
            // Take the rightmost segment so the bilingual lookup matches
            // a bare `Filter` / `WriteDataHistory` / `AdditionalProperties`.
            // A dot-free `english_name` returns itself via `rsplit`,
            // matching the bilingual-key convention used elsewhere.
            name_en: Some(Name::new(en_tail)),
            ty,
            value_ty: None,
            is_readonly: res.is_readonly,
            origin: FieldOrigin::PlatformProperty,
        };
        push_unique(out, seen, info);
    }
}

/// Promote a HBK self-typed `Ty::PlatformObject(base)` to a concrete
/// `Ty::MetadataRef { kind, name: receiver_mdo_name }` when `base`
/// matches the receiver MDO's Object or Ref companion display label.
///
/// Covers every Object/Ref family pair (Document, Catalog, Task,
/// BusinessProcess, ExchangePlan, ChartOfAccounts), the Object-only
/// families (DataProcessor, Report), and the Ref-only Enum family —
/// every entry where [`MetadataKind::object_kind_for`] or
/// [`ref_kind_for_mdo`] returns `Some(_)`.
///
/// Comparison folds `ё ↔ е` via [`eq_yo_insensitive`]. The HBK dumps
/// `ОтчетОбъект` (without `ё`) while [`MetadataKind::display_label`]
/// returns `ОтчётОбъект`; without the fold, Report objects would silently
/// stay generic.
///
/// Restores chain typing for `<receiver>.ЭтотОбъект.<…>` and any HBK
/// property whose declared type is the receiver's own family base
/// (`ДокументОбъект`, `СправочникСсылка`, `ЗадачаОбъект`, …). Returns
/// `None` for cross-family bases (e.g. `Владелец: СправочникСсылка` on
/// a Catalog points at the *owner* catalog, not self — those are
/// configurator-conditional and handled by the spec's `HasOwners`
/// path, not by this cascade).
fn specialize_self_ref_ty(mdo_type: MdoType, mdo_name: &Name, ty: &Ty) -> Option<Ty> {
    let Ty::PlatformObject(base) = ty else {
        return None;
    };
    let base = base.as_str();
    let candidates = [MetadataKind::object_kind_for(mdo_type), ref_kind_for_mdo(mdo_type)];
    for candidate in candidates.into_iter().flatten() {
        let ru = candidate.display_label(base_db::Locale::Ru);
        let en = candidate.display_label(base_db::Locale::En);
        if eq_yo_insensitive(base, ru) || base == en {
            return Some(Ty::MetadataRef { kind: candidate, name: mdo_name.clone() });
        }
    }
    None
}

/// Compare two Russian platform-type names ignoring the `ё` ↔ `е`
/// spelling difference. HBK pages mix both spellings — `display_label`
/// uses `ОтчётОбъект`, but `platform_data.json` ships `ОтчетОбъект`. The
/// sibling normaliser in [`crate::platform_manager_lookup`] solves the
/// same problem by listing both spellings; we fold them here once so
/// future ё-bearing labels (e.g. CalcReg `РегистрРасчётаКлючЗаписи`)
/// don't need per-call enumeration.
fn eq_yo_insensitive(lhs: &str, rhs: &str) -> bool {
    if lhs == rhs {
        return true;
    }
    if lhs.len() != rhs.len() {
        return false;
    }
    lhs.chars().map(fold_yo).eq(rhs.chars().map(fold_yo))
}

fn fold_yo(c: char) -> char {
    match c {
        'ё' => 'е',
        'Ё' => 'Е',
        _ => c,
    }
}

fn ref_kind_for_mdo(mdo: MdoType) -> Option<MetadataKind> {
    Some(match mdo {
        MdoType::Catalog => MetadataKind::CatalogRef,
        MdoType::Document => MetadataKind::DocumentRef,
        MdoType::Task => MetadataKind::TaskRef,
        MdoType::BusinessProcess => MetadataKind::BusinessProcessRef,
        MdoType::ExchangePlan => MetadataKind::ExchangePlanRef,
        MdoType::ChartOfAccounts => MetadataKind::ChartOfAccountsRef,
        MdoType::Enum => MetadataKind::EnumRef,
        _ => return None,
    })
}

/// Lowercased set of standard-attribute names whose **presence is
/// configurator-conditional** for `mdo_type` (`HasCode`, `HasNumber`,
/// `Hierarchical`, `HasOwners`, `IsPeriodic`). These are the names the
/// HBK cascade must NOT push, because HBK has no knowledge of the gate
/// and would surface (e.g.) `Номер` on a document without a configured
/// number length.
///
/// `Always`-condition spec entries (e.g. `Ссылка`, `Дата`, `Проведен`,
/// `Активность`) are intentionally NOT included: the cascade is allowed
/// to push them. When `xml_parser/standard_attributes` materialised
/// them into `mdo.attributes` the cascade entry is shadowed by
/// `push_unique`; when it didn't (synthesised test configs), the
/// cascade provides a typed fall-through via [`specialize_self_ref_ty`].
///
/// Returns an empty set when the `MdoType` has no template in
/// [`mdo_template_kind_for`] — the cascade then runs unfiltered.
fn standard_attribute_names_for(mdo_type: MdoType) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let Some(template) = mdo_template_kind_for(mdo_type) else {
        return names;
    };
    for view in [ObjectView::Object, ObjectView::Ref, ObjectView::RecordSet] {
        for spec in standard_attributes_for(template, view) {
            if matches!(spec.condition, bsl_platform::PresenceCondition::Always) {
                continue;
            }
            insert_standard_kind_names(&mut names, spec.kind);
        }
    }
    names
}

fn insert_standard_kind_names(set: &mut std::collections::HashSet<String>, kind: StandardKind) {
    set.insert(kind.russian_name().to_lowercase());
    set.insert(kind.english_name().to_lowercase());
}

fn name_in_spec(spec_names: &std::collections::HashSet<String>, ru: &str, en: &str) -> bool {
    spec_names.contains(&ru.to_lowercase()) || spec_names.contains(&en.to_lowercase())
}

// ---------------------------------------------------------------------------
// Internal enumerators
// ---------------------------------------------------------------------------

fn enumerate_mdo_fields(
    configs: &[VisibleConfig],
    kind: MetadataKind,
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
                ty: attribute_type_to_ty(&attr.attr_type, configs),
                value_ty: None,
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
                value_ty: None,
                is_readonly: false,
                origin: FieldOrigin::TabularSection,
            };
            push_unique(&mut out, &mut seen, info);
        }

        // HBK platform-property cascade. Surfaces `ДополнительныеСвойства`,
        // `Движения`, `ОбменДанными`, `ВерсияДанных`, `ЗаписьИсторииДанных`,
        // `ПринадлежностьПоследовательностям`, `ЭтотОбъект`, etc., that the
        // HBK declares per `<Prefix>.<MDO>` composite (Document/Catalog/
        // Task/BusinessProcess/ExchangePlan/ChartOfAccounts, both Object
        // and Ref views, plus DataProcessor/Report). Pushed last so user
        // and standard attributes keep their typed entries on a name
        // collision. MDO side has no recorder rebind — that's register-only.
        // `mdo_type` is forwarded so the helper can gate out standard
        // attribute names whose presence is config-conditional (the spec
        // owns those — see `push_platform_prefix_properties` docs).
        // `mdo_name` is forwarded so self-typed HBK properties
        // (`ЭтотОбъект` → `ДокументОбъект`, `Ссылка` → `ДокументСсылка`,
        // …) can be re-typed to a concrete `MetadataRef` anchored on
        // this receiver, restoring chain typing.
        push_platform_prefix_properties(kind, mdo_type, mdo_name, &mut out, &mut seen, |_| None);

        return out;
    }
    Vec::new()
}

fn enumerate_register_fields(
    configs: &[VisibleConfig],
    kind: MetadataKind,
    parent: MdoType,
    register_name: &Name,
) -> Vec<FieldInfo> {
    for cfg in configs.iter().rev() {
        let Some(register) =
            cfg.configuration.find_register_by_type_and_name(parent, register_name.as_str())
        else {
            continue;
        };

        let cap = register.dimensions().len()
            + register.resources().len()
            + register.attributes().len()
            + 1;
        let mut out = Vec::with_capacity(cap);
        let mut seen: std::collections::HashSet<Name> =
            std::collections::HashSet::with_capacity(cap * 2);

        // Synthetic `.Отбор` (Filter) on record-set receivers. The HBK
        // does not declare this property on any RecordSet `type_name`
        // (gap of `shcntx_ru.hbk`, not a scraper bug), so we synthesize
        // it from 1С runtime semantics. Pushed BEFORE dimensions so a
        // collision with a dimension named `Отбор` is resolved in
        // favour of the synthetic Filter — matches 1С behaviour, where
        // the platform property always wins (the dimension stays
        // reachable as `<recordSet>.Отбор.Отбор`).
        if is_record_set_kind(kind) {
            let info = FieldInfo {
                name: Name::new("Отбор"),
                name_en: Some(Name::new("Filter")),
                ty: Ty::MetadataRef {
                    kind: MetadataKind::RegisterFilter { parent },
                    name: register_name.clone(),
                },
                value_ty: None,
                is_readonly: true,
                origin: FieldOrigin::PlatformProperty,
            };
            push_unique(&mut out, &mut seen, info);
        }

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
                    configs,
                ),
                value_ty: None,
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
                    configs,
                ),
                value_ty: None,
                is_readonly: false,
                origin: FieldOrigin::RegisterResource,
            };
            push_unique(&mut out, &mut seen, info);
        }

        for attr in register.attributes() {
            let mut ty = register_part_ty(
                attr.attr_type(),
                MetadataKind::RegisterAttribute { parent },
                register_name,
                attr.name(),
                configs,
            );
            if is_record_kind(kind) && is_recorder_name(attr.name()) {
                if let Some(recorders_ty) = recorder_union_ty(configs, parent, register_name) {
                    ty = recorders_ty;
                }
            }
            let info = FieldInfo {
                name: Name::new(attr.name()),
                name_en: attr.name_en().filter(|s| !s.is_empty()).map(Name::new),
                ty,
                value_ty: None,
                is_readonly: false,
                origin: FieldOrigin::RegisterAttribute,
            };
            push_unique(&mut out, &mut seen, info);
        }

        // Platform properties indexed under the composite type prefix
        // (`InformationRegisterRecordSet.<Имя>` etc.). Surfaces `Записывать`,
        // `ОбменДанными`, `ДополнительныеСвойства`, `БлокироватьДляИзменения`
        // (Accounting only), etc. Pushed AFTER user-defined parts so a real
        // dimension/resource/attribute wins on a name collision; the
        // synthetic `.Отбор` pushed earlier already won over any platform
        // `Filter`. Recorder override widens the platform-declared base
        // ref into a union of concrete document refs for record kinds.
        push_platform_prefix_properties(
            kind,
            parent,
            register_name,
            &mut out,
            &mut seen,
            |prop_name| {
                if is_record_kind(kind) && is_recorder_name(prop_name) {
                    recorder_union_ty(configs, parent, register_name)
                } else {
                    None
                }
            },
        );

        return out;
    }
    Vec::new()
}

fn is_recorder_name(name: &str) -> bool {
    if name.eq_ignore_ascii_case("Recorder") {
        return true;
    }
    !name.is_ascii() && name.to_lowercase() == "регистратор"
}

fn recorder_union_ty(
    configs: &[VisibleConfig],
    parent: MdoType,
    register_name: &Name,
) -> Option<Ty> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut docs: Vec<Ty> = Vec::new();
    for cfg in configs {
        for name in cfg.configuration.recorders_for_register(parent, register_name.as_str()) {
            if seen.insert(name.to_lowercase()) {
                docs.push(Ty::MetadataRef {
                    kind: MetadataKind::DocumentRef,
                    name: Name::new(name),
                });
            }
        }
    }
    if docs.is_empty() {
        None
    } else {
        Some(Ty::union(docs))
    }
}

/// Enumerate the members of a record-set's `Отбор` (Filter) — one
/// `ЭлементОтбора` (FilterItem) per register dimension.
///
/// 1С runtime exposes the register's dimensions as the keyed members
/// of the Filter object on a record-set: `НаборЗаписей.Отбор.<Имя>`
/// returns a FilterItem you can call `.Установить(...)` on. This is
/// not declared in HBK (`platform_data.json` has no `Отбор` property
/// on any RecordSet `type_name`), so we synthesize it directly from
/// the register's XML metadata.
///
/// Resources / attributes are intentionally excluded — only dimensions
/// participate in the Filter member surface.
///
/// Returns an empty `Vec` when the register name does not resolve
/// against any visible configuration; same fallthrough policy as
/// [`enumerate_register_fields`].
fn enumerate_filter_fields(
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

        let standard_keys = standard_filter_keys(parent, register.periodicity());
        let mut out = Vec::with_capacity(register.dimensions().len() + standard_keys.len());
        let mut seen: std::collections::HashSet<Name> =
            std::collections::HashSet::with_capacity(out.capacity() * 2);

        for dim in register.dimensions() {
            let info = FieldInfo {
                name: Name::new(dim.name()),
                name_en: None,
                ty: Ty::PlatformObject(Name::new("ЭлементОтбора")),
                value_ty: Some(
                    dim.attr_type()
                        .map(|attr_type| attribute_type_to_ty(attr_type, configs))
                        .unwrap_or(Ty::Unknown),
                ),
                is_readonly: false,
                origin: FieldOrigin::RegisterDimension,
            };
            push_unique(&mut out, &mut seen, info);
        }

        for key in standard_keys {
            let info = FieldInfo {
                name: Name::new(key),
                name_en: None,
                ty: Ty::PlatformObject(Name::new("ЭлементОтбора")),
                value_ty: Some(standard_filter_key_value_ty(configs, parent, register_name, key)),
                is_readonly: false,
                origin: FieldOrigin::PlatformProperty,
            };
            push_unique(&mut out, &mut seen, info);
        }

        return out;
    }
    Vec::new()
}

fn standard_filter_key_value_ty(
    configs: &[VisibleConfig],
    parent: MdoType,
    register_name: &Name,
    key: &str,
) -> Ty {
    match key {
        "Регистратор" => {
            recorder_union_ty(configs, parent, register_name).unwrap_or(Ty::Unknown)
        }
        "Период" | "ПериодРегистрации" => Ty::Date,
        "Активность" => Ty::Boolean,
        "НомерСтроки" => Ty::Number,
        // CalcReg-specific, separate slice
        "ВидРасчета" => Ty::Unknown,
        _ => Ty::Unknown,
    }
}

fn standard_filter_keys(
    parent: MdoType,
    periodicity: Option<RegisterPeriodicity>,
) -> &'static [&'static str] {
    match parent {
        MdoType::AccumulationRegister | MdoType::AccountingRegister => {
            &["Период", "Регистратор", "НомерСтроки", "Активность"]
        }
        // ITS dump index.json content/130 "Регистры расчета" (html/chapter_130.html)
        // lists calculation-register fields Регистратор, НомерСтроки, Активность,
        // ВидРасчета, ПериодРегистрации, and ПериодДействия only for registers
        // with the "Период действия" property; plain `Период` is not listed.
        //
        // `Register::periodicity()` only captures `<InformationRegisterPeriodicity>`,
        // so the `<ActionPeriod>` flag that gates `ПериодДействия` is not yet
        // available on the parsed metadata. Until the XML parser is extended,
        // emit the unconditional core (which covers the vast majority of real
        // calculation registers, since they are recorder-driven by design); the
        // optional `ПериодДействия` filter key is intentionally omitted to keep
        // completion accurate rather than over-inclusive.
        MdoType::CalculationRegister => {
            let _ = periodicity;
            &["Регистратор", "НомерСтроки", "Активность", "ВидРасчета", "ПериодРегистрации"]
        }
        MdoType::InformationRegister => match periodicity {
            Some(RegisterPeriodicity::RecorderPosition) => &["Регистратор", "Активность", "Период"],
            Some(
                RegisterPeriodicity::Second | RegisterPeriodicity::Day | RegisterPeriodicity::Month,
            ) => &["Период", "Активность"],
            Some(RegisterPeriodicity::Nonperiodical) | None => &[],
        },
        _ => &[],
    }
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
            ty: attribute_type_to_ty(attr.attr_type(), configs),
            value_ty: None,
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
                value_ty: None,
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
        MetadataKind::TaskRef | MetadataKind::TaskObject => Some(MdoType::Task),
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            Some(MdoType::BusinessProcess)
        }
        MetadataKind::DataProcessorObject => Some(MdoType::DataProcessor),
        MetadataKind::ReportObject => Some(MdoType::Report),
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
            Some(MdoType::ExchangePlan)
        }
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            Some(MdoType::ChartOfAccounts)
        }
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRecordSet
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRecordSet
        | MetadataKind::CalculationRegisterRecord
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => None,
    }
}

/// Map a register-flavoured receiver `MetadataKind` to its register [`MdoType`].
///
/// Returns `None` for non-register kinds, leaf part kinds
/// (`RegisterDimension` / `RegisterResource` / `RegisterAttribute`),
/// and the synthetic `RegisterFilter` (which is dispatched separately
/// in [`enumerate_fields`]).
///
/// # Per-flavour platform surface
///
/// All four `*Record` kinds route through this function so the
/// platform-properties / platform-methods arms of [`enumerate_fields`]
/// resolve their composite prefix (`InformationRegisterRecord.<Имя>`,
/// etc.). The platform method `МоментВремени()` is exposed by HBK
/// 8.3.27 on three of the four record flavours —
/// `InformationRegisterRecord`, `AccumulationRegisterRecord`,
/// `AccountingRegisterRecord` — but **not** on
/// `CalculationRegisterRecord`, whose only composite-prefix methods
/// are `ПолучитьДанныеГрафика` / `ПолучитьБазу`. The asymmetry comes
/// from the syntax help, not from anything we do here; this comment
/// documents the upstream divergence so a future contributor doesn't
/// look for `МоментВремени()` coverage on CalcReg records and
/// (mis)conclude that something is missing locally.
pub(crate) fn register_parent_for_kind(kind: MetadataKind) -> Option<MdoType> {
    match kind {
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::InformationRegisterRef => Some(MdoType::InformationRegister),
        MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccumulationRegisterRef => Some(MdoType::AccumulationRegister),
        MetadataKind::AccountingRegisterRecordSet
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::AccountingRegisterRef => Some(MdoType::AccountingRegister),
        MetadataKind::CalculationRegisterRecordSet
        | MetadataKind::CalculationRegisterRecord
        | MetadataKind::CalculationRegisterRef => Some(MdoType::CalculationRegister),
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
///
/// `configs` is forwarded so `ОпределяемыйТип`-typed attributes can be
/// expanded to their underlying `Ty` (e.g. `СуммаДокумента` typed as
/// `cfg:DefinedType.ДенежнаяСуммаЛюбогоЗнака` lowers to `Ty::Number`).
/// Without the visible configurations the resolver could not look up
/// the DefinedType chain — every field-enumeration call site already has
/// `&[VisibleConfig]` in scope, so the dependency is thread-through, not new.
pub(crate) fn attribute_type_to_ty(attr_type: &AttributeType, configs: &[VisibleConfig]) -> Ty {
    let type_ref = TypeRef::from_attribute_type(attr_type);
    let resolver = ConfigsResolver(configs);
    TyLoweringContext::with_resolver(&resolver).lower_type_ref(&type_ref)
}

/// Lower a register-part type, falling back to a symbolic
/// `MetadataKind::Register{Dimension,Resource,Attribute}` when `attr_type`
/// is absent.
pub(crate) fn register_part_ty(
    attr_type: Option<&AttributeType>,
    fallback_kind: MetadataKind,
    register_name: &Name,
    part_name: &str,
    configs: &[VisibleConfig],
) -> Ty {
    match attr_type {
        Some(at) => attribute_type_to_ty(at, configs),
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
    fn enumerate_register_record_recorder_uses_document_recorders_union() {
        let mut config = Configuration::new("Test");
        let mut doc1 = MetadataObject::new(MdoType::Document, "Документ1");
        doc1.set_register_records(vec![(MdoType::InformationRegister, "РегистрСведений1".into())]);
        let mut doc2 = MetadataObject::new(MdoType::Document, "Документ2");
        doc2.set_register_records(vec![(MdoType::InformationRegister, "РегистрСведений1".into())]);
        config.add_metadata_object(doc1);
        config.add_metadata_object(doc2);
        config.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![],
            vec![attribute_typed(
                "Регистратор",
                AttributeType::AnyObjectRef { mdo_type: MdoType::Document },
            )],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecord,
            name: Name::new("РегистрСведений1"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let recorder = fields
            .iter()
            .find(|f| f.name.as_str() == "Регистратор")
            .expect("Регистратор must appear on register record");

        assert_eq!(
            recorder.ty,
            Ty::union(vec![
                Ty::MetadataRef {
                    kind: MetadataKind::DocumentRef, name: Name::new("Документ1")
                },
                Ty::MetadataRef {
                    kind: MetadataKind::DocumentRef, name: Name::new("Документ2")
                },
            ]),
        );
    }

    #[test]
    fn enumerate_register_record_recorder_singleton_collapses_to_metadata_ref() {
        let mut config = Configuration::new("Test");
        let mut document = MetadataObject::new(MdoType::Document, "Документ1");
        document
            .set_register_records(vec![(MdoType::InformationRegister, "РегистрСведений1".into())]);
        config.add_metadata_object(document);
        config.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![],
            vec![attribute_typed(
                "регистратор",
                AttributeType::AnyObjectRef { mdo_type: MdoType::Document },
            )],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecord,
            name: Name::new("РегистрСведений1"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let recorder = fields
            .iter()
            .find(|f| f.name.as_str() == "регистратор")
            .expect("регистратор must appear on register record");

        assert_eq!(
            recorder.ty,
            Ty::MetadataRef {
                kind: MetadataKind::DocumentRef, name: Name::new("Документ1")
            },
        );
    }

    #[test]
    fn enumerate_register_record_recorder_aggregates_across_visible_extensions() {
        // Extension scenario: base configuration declares the register and
        // one recorder; an extension configuration declares an additional
        // document that records into the same register. The recorder union
        // must contain documents from BOTH configurations.
        let mut base = Configuration::new("Base");
        let mut doc1 = MetadataObject::new(MdoType::Document, "Документ1");
        doc1.set_register_records(vec![(MdoType::InformationRegister, "РегистрСведений1".into())]);
        base.add_metadata_object(doc1);
        base.add_register(register_with(
            "РегистрСведений1",
            MdoType::InformationRegister,
            vec![],
            vec![],
            vec![attribute_typed(
                "Регистратор",
                AttributeType::AnyObjectRef { mdo_type: MdoType::Document },
            )],
        ));

        let mut ext = Configuration::new("Extension");
        let mut doc2 = MetadataObject::new(MdoType::Document, "Документ2");
        doc2.set_register_records(vec![(MdoType::InformationRegister, "РегистрСведений1".into())]);
        ext.add_metadata_object(doc2);

        let configs = vec![
            VisibleConfig { name: None, configuration: Arc::new(base) },
            VisibleConfig { name: Some("Extension".to_string()), configuration: Arc::new(ext) },
        ];

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::InformationRegisterRecord,
            name: Name::new("РегистрСведений1"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let recorder = fields
            .iter()
            .find(|f| f.name.as_str() == "Регистратор")
            .expect("Регистратор must appear on register record");

        let Ty::Union(parts) = &recorder.ty else {
            panic!("expected Ty::Union for cross-config recorders, got {:?}", recorder.ty);
        };
        let names: std::collections::HashSet<&str> = parts
            .iter()
            .filter_map(|t| match t {
                Ty::MetadataRef { kind: MetadataKind::DocumentRef, name } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains("Документ1"), "base recorder must be present, got {names:?}");
        assert!(names.contains("Документ2"), "extension recorder must be present, got {names:?}");
        assert_eq!(names.len(), 2, "exactly two recorders expected, got {names:?}");
    }

    #[test]
    fn enumerate_filter_fields_adds_standard_accumulation_keys_after_dimensions() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегистрНакопления1",
            MdoType::AccumulationRegister,
            vec![dimension_typed("Регистратор", AttributeType::String { length: Some(10) })],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::AccumulationRegister },
            name: Name::new("РегистрНакопления1"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        for key in ["Период", "Регистратор", "НомерСтроки", "Активность"]
        {
            assert!(
                fields.iter().any(|f| f.name.as_str() == key),
                "{key} must be exposed on accumulation register filter",
            );
        }
        let recorder = fields
            .iter()
            .find(|f| f.name.as_str() == "Регистратор")
            .expect("Регистратор filter member");
        assert_eq!(
            recorder.origin,
            FieldOrigin::RegisterDimension,
            "dimension must win over the standard filter key with the same name",
        );
    }

    #[test]
    fn enumerate_filter_fields_value_ty_for_information_register() {
        let register = bsl_metadata::Register::builder()
            .name("Курсы")
            .mdo_type(MdoType::InformationRegister)
            .periodicity(Some(RegisterPeriodicity::Second))
            .add_dimension(dimension_typed(
                "Валюта",
                AttributeType::Ref { mdo_type: MdoType::Catalog, name: "Валюты".into() },
            ))
            .add_dimension(dimension_typed(
                "Цена",
                AttributeType::Number { precision: 15, scale: 2 },
            ))
            .build();
        let mut config = Configuration::new("Test");
        config.add_register(register);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("Курсы"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        let period = fields.iter().find(|f| f.name.as_str() == "Период").expect("Период");
        assert_eq!(period.value_ty, Some(Ty::Date));
        let active = fields.iter().find(|f| f.name.as_str() == "Активность").expect("Активность");
        assert_eq!(active.value_ty, Some(Ty::Boolean));
        let currency = fields.iter().find(|f| f.name.as_str() == "Валюта").expect("Валюта");
        assert_eq!(
            currency.value_ty,
            Some(Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Валюты")
            }),
        );
        let price = fields.iter().find(|f| f.name.as_str() == "Цена").expect("Цена");
        assert_eq!(price.value_ty, Some(Ty::Number));
    }

    #[test]
    fn enumerate_filter_fields_value_ty_for_accumulation_register() {
        let mut config = Configuration::new("Test");
        let mut document = MetadataObject::new(MdoType::Document, "Поступление");
        document.set_register_records(vec![(MdoType::AccumulationRegister, "Остатки".into())]);
        config.add_metadata_object(document);
        config.add_register(register_with(
            "Остатки",
            MdoType::AccumulationRegister,
            vec![],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::AccumulationRegister },
            name: Name::new("Остатки"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        let recorder =
            fields.iter().find(|f| f.name.as_str() == "Регистратор").expect("Регистратор");
        assert_eq!(
            recorder.value_ty,
            Some(Ty::MetadataRef {
                kind: MetadataKind::DocumentRef,
                name: Name::new("Поступление"),
            }),
        );
    }

    #[test]
    fn enumerate_filter_fields_dimension_named_period_wins_over_standard_key() {
        let register = bsl_metadata::Register::builder()
            .name("Срез")
            .mdo_type(MdoType::InformationRegister)
            .periodicity(Some(RegisterPeriodicity::Second))
            .add_dimension(dimension_typed(
                "Период",
                AttributeType::Number { precision: 10, scale: 0 },
            ))
            .build();
        let mut config = Configuration::new("Test");
        config.add_register(register);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("Срез"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let period = fields.iter().find(|f| f.name.as_str() == "Период").expect("Период");

        assert_eq!(period.origin, FieldOrigin::RegisterDimension);
        assert_eq!(period.value_ty, Some(Ty::Number));
    }

    #[test]
    fn enumerate_filter_fields_calculation_register_uses_correct_standard_keys() {
        let mut config = Configuration::new("Test");
        config.add_register(register_with(
            "РегистрРасчета1",
            MdoType::CalculationRegister,
            vec![],
            vec![],
            vec![],
        ));
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::CalculationRegister },
            name: Name::new("РегистрРасчета1"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();

        assert_eq!(
            names,
            vec!["Регистратор", "НомерСтроки", "Активность", "ВидРасчета", "ПериодРегистрации"],
        );
        // `Register::periodicity()` does not populate from `<ActionPeriod>` yet,
        // so the CalcReg branch must ignore the periodicity argument to stay
        // accurate on real parsed metadata.
        assert_eq!(
            standard_filter_keys(MdoType::CalculationRegister, Some(RegisterPeriodicity::Month)),
            standard_filter_keys(MdoType::CalculationRegister, None),
        );
    }

    #[test]
    fn enumerate_filter_fields_recorder_position_information_register_uses_register_keys() {
        let register = bsl_metadata::Register::builder()
            .name("ПозицияРегистратора")
            .mdo_type(MdoType::InformationRegister)
            .periodicity(Some(RegisterPeriodicity::RecorderPosition))
            .build();
        let mut config = Configuration::new("Test");
        config.add_register(register);
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("ПозицияРегистратора"),
        };
        let fields = enumerate_fields(&configs, &receiver);
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();

        assert_eq!(names, vec!["Регистратор", "Активность", "Период"]);
    }

    #[test]
    fn enumerate_filter_fields_adds_period_for_periodic_information_register() {
        let config = {
            let register = bsl_metadata::Register::builder()
                .name("ПериодическийРегистр")
                .mdo_type(MdoType::InformationRegister)
                .periodicity(Some(RegisterPeriodicity::Second))
                .build();
            let mut config = Configuration::new("Test");
            config.add_register(register);
            config
        };
        let configs = wrap(config);

        let receiver = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("ПериодическийРегистр"),
        };
        let fields = enumerate_fields(&configs, &receiver);

        assert!(fields.iter().any(|f| f.name.as_str() == "Период"));
        assert!(fields.iter().any(|f| f.name.as_str() == "Активность"));
        assert!(!fields.iter().any(|f| f.name.as_str() == "Регистратор"));
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
    fn document_attribute_typed_via_defined_type_lowers_to_underlying() {
        // niagara_ut bug repro at the field-enumeration layer.
        //
        // Mirror of `Documents/ПриобретениеТоваровУслуг.xml`, where
        // `СуммаДокумента` is typed via
        // `<v8:TypeSet>cfg:DefinedType.ДенежнаяСуммаЛюбогоЗнака</v8:TypeSet>`,
        // and `DefinedTypes/ДенежнаяСуммаЛюбогоЗнака.xml` declares the
        // underlying as `xs:decimal`. The enumerator must follow the
        // DefinedType reference all the way to `Ty::Number` instead of
        // collapsing to `Ty::Unknown`.
        let mut config = Configuration::new("Test");
        config.add_defined_type(
            bsl_metadata::DefinedType::builder()
                .uuid(Uuid::new_v4())
                .name("ДенежнаяСуммаЛюбогоЗнака")
                .underlying_type(AttributeType::Number { precision: 15, scale: 2 })
                .build(),
        );
        config.add_metadata_object({
            let mut doc = MetadataObject::new(MdoType::Document, "ПКО");
            doc.add_attribute(attr(
                "СуммаДокумента",
                None,
                AttributeType::DefinedType {
                    name: "ДенежнаяСуммаЛюбогоЗнака".to_string()
                },
            ));
            doc
        });
        let configs = wrap(config);

        let receiver =
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") };
        let fields = enumerate_fields(&configs, &receiver);
        let sum =
            fields.iter().find(|f| f.name.as_str() == "СуммаДокумента").expect("СуммаДокумента");
        assert_eq!(
            sum.ty,
            Ty::Number,
            "DefinedType-typed attribute must resolve to its underlying `Ty::Number`"
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

    /// HBK 8.3.27 documents the platform method `МоментВремени()`
    /// (`PointInTime()`) for three of the four `*Record` flavours but
    /// **not** for `CalculationRegisterRecord` — its only composite
    /// methods are `ПолучитьДанныеГрафика` / `ПолучитьБазу`. This test
    /// pins that asymmetry so a future regeneration of
    /// `platform_data.json` (or a refactor of how composite methods
    /// load) can't silently start surfacing a phantom `МоментВремени()`
    /// on CalcReg records, or — in the other direction — drop it from
    /// the three flavours that legitimately expose it.
    #[test]
    fn point_in_time_present_on_three_record_flavours_absent_on_calc() {
        let pd = PlatformData::instance();
        // The HBK page header for these methods lives in the
        // composite-prefix block: their `name` is the truncated
        // placeholder `<Имя` and the resolvable token is the english
        // suffix after the last `.` (`PointInTime`). That suffix is
        // what `english_name.rsplit('.').next()` exposes everywhere
        // else in the resolver, so the regression check applies the
        // same projection here.
        let has_pit = |prefix: &str| {
            pd.get_manager_methods(prefix).iter().any(|m| {
                m.english_name
                    .as_str()
                    .rsplit('.')
                    .next()
                    .map(|tail| tail == "PointInTime")
                    .unwrap_or(false)
            })
        };
        assert!(has_pit("InformationRegisterRecord"), "InfoReg record must expose МоментВремени");
        assert!(has_pit("AccumulationRegisterRecord"), "AccumReg record must expose МоментВремени");
        assert!(has_pit("AccountingRegisterRecord"), "AcctReg record must expose МоментВремени");
        assert!(
            !has_pit("CalculationRegisterRecord"),
            "CalcReg record must NOT expose МоментВремени per HBK 8.3.27 — \
             surfacing one indicates a regression in platform_data or a wrong-prefix lookup",
        );
    }
}
