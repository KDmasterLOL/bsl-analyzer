//! Property lookup on platform value receivers.
//!
//! This is the field-lookup analogue of [`crate::platform_manager_lookup`]:
//! given a receiver typed as a platform value (`Ty::PlatformObject`, a
//! collection variant, or a primitive), resolve `receiver.field_name`
//! against the `PlatformProperty` catalogue in `bsl-platform`.
//!
//! # Why a separate adapter
//!
//! `method_lookup` and `field_lookup` sit in parallel: methods are indexed
//! by `(type_name, method_name)` in `platform_data.json → methods`, while
//! properties live in a dedicated `properties` array with an identically
//! shaped bilingual index. Keeping the adapters symmetrical means the two
//! dispatchers share nothing but the `platform_type_key` helper from
//! [`crate::method_lookup`], which turns a scalar `Ty` into the English
//! canonical name the platform index expects.
//!
//! # Coverage
//!
//! - **`Ty::PlatformObject(name)`** — the primary case. `Запрос`, `Запись`,
//!   `РезультатЗапроса`, `ТаблицаЗначений`, and the long tail of types that
//!   expose instance properties. The `name` is passed straight to the
//!   bilingual index, so both `Ty::PlatformObject("Запрос")` and a
//!   `PlatformObject("Query")` resolve to the same property row.
//! - **Collection variants** (`Ty::Array`, `Ty::Map`, `Ty::Structure`,
//!   `Ty::ValueTable`, `Ty::ValueList`, `Ty::Type`) — when the HBK describes
//!   an instance property on them (e.g. `ТаблицаЗначений.Колонки`).
//! - **Primitives** (`Ty::Number`, `Ty::String`, `Ty::Boolean`, `Ty::Date`)
//!   — `None`. BSL primitives have no declared properties; returning `None`
//!   here keeps the "scalar key only" invariant of
//!   [`crate::method_lookup::platform_type_key`] intact.
//!
//! `Ty::MetadataRef` is deliberately **not** routed through
//! [`lookup_platform_property`] — metadata-ref property access goes
//! through [`crate::field_enum::enumerate_fields`], which composes the
//! MDO's user/standard attributes, tabular sections, and the HBK
//! platform-property cascade keyed by [`hir_def::ty::MetadataKind::platform_prefix`].
//! The dispatcher in `lookup_field` decides who owns a receiver, and
//! [`lookup_platform_property`] returns `None` for `MetadataRef` /
//! `ObjectManager` inputs as defense-in-depth (see
//! `metadata_ref_and_manager_receivers_return_none` test).
//!
//! What IS shared with the metadata-ref path is the small
//! [`to_resolution`] helper: both [`crate::field_enum::enumerate_mdo_fields`]
//! and [`crate::field_enum::enumerate_register_fields`] call it to convert
//! a [`PlatformProperty`] into a `(Ty, is_readonly)` pair. That keeps the
//! type-mapping logic (single-element list collapse, multi-element list
//! union) in one place without re-exposing the full adapter to receivers
//! it does not own.

use bsl_platform::{PlatformData, PlatformProperty};
use hir_def::ty::Ty;
use hir_def::Name;

use crate::lower::type_string::lower_platform_type_name;
use crate::method_lookup::platform_type_key;

/// Result of a successful platform-property lookup.
///
/// Unlike [`crate::field_lookup::FieldInfo`] — which is returned from the
/// `lookup_field` façade for any kind of field (MDO attribute or platform
/// property) — this struct stays local to the adapter. Its fields feed the
/// `FieldInfo` that the façade assembles, keeping the two layers decoupled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPropertyResolution {
    /// Resolved value type. Derived from `PlatformProperty::property_types`
    /// via [`map_property_type_list`]: single-element lists collapse to the
    /// matching scalar `Ty`, multi-element lists become `Ty::Union(...)`.
    /// Empty lists map to `Ty::Unknown` (the HBK page omitted the `Тип:`
    /// marker — free-prose description only).
    pub return_ty: Ty,
    /// `true` when `Использование:` reads `"Только чтение"` on the property
    /// page. Feeds the `ReadOnlyPropertyAssignment` diagnostic.
    pub is_readonly: bool,
}

/// Resolve `receiver.prop_name` against the platform-property catalogue.
///
/// Returns `None` when:
/// - the receiver is not keyed by [`platform_type_key`] (managers, metadata
///   refs, unions, primitives, `Ty::Unknown`);
/// - the platform type has no property with this name.
///
/// Uses the `PlatformData::instance()` singleton the same way
/// [`crate::method_lookup::lookup_method`] does, so callers stay `db`-free
/// (a Salsa wrapper lives in `bsl-platform` and is used by IDE completion
/// where `db` is already available).
pub fn lookup_platform_property(
    receiver_ty: &Ty,
    prop_name: &Name,
) -> Option<PlatformPropertyResolution> {
    // Form-control receivers carry an ordered platform-type chain
    // `[base, extension?]` (e.g. `Pages → ["ГруппаФормы", "Расширение
    // группы формы для страниц"]`). The reverse-walk precedence
    // (extension overrides base, single-entry chains collapse, `Other`
    // is empty → `None`) is shared with method_lookup and lives in
    // [`hir_def::ty::form_control_chain_first_hit`].
    if let Ty::FormControl { kind, .. } = receiver_ty {
        return hir_def::ty::form_control_chain_first_hit(*kind, |type_name| {
            lookup_platform_property_by_type(type_name, prop_name)
        });
    }
    let type_key = platform_type_key(receiver_ty)?;
    lookup_platform_property_by_type(type_key, prop_name)
}

/// Same as [`lookup_platform_property`] but keyed directly by an English
/// platform `type_name`. Used by callers whose receiver type does not map
/// to a `platform_type_key` (e.g. `Ty::MetadataRef { TabularSectionRow, .. }`
/// borrowing the standard row properties from
/// `"Line of a tabular section"`).
pub(crate) fn lookup_platform_property_by_type(
    type_name: &str,
    prop_name: &Name,
) -> Option<PlatformPropertyResolution> {
    let data = PlatformData::instance();
    let prop = data.get_property(type_name, prop_name.as_str())?;
    Some(to_resolution(prop))
}

/// Convert a `PlatformProperty` into the semantic [`PlatformPropertyResolution`].
///
/// Mirrors `method_lookup::to_method_info` in shape — same path through
/// `lower_platform_type_name` for each declared value type, then
/// `Ty::union` when the HBK page declared more than one (e.g.
/// `МенеджерВременныхТаблиц, Неопределено`).
pub(crate) fn to_resolution(prop: &PlatformProperty) -> PlatformPropertyResolution {
    PlatformPropertyResolution {
        return_ty: map_property_type_list(&prop.property_types),
        is_readonly: prop.is_readonly,
    }
}

/// Map the parsed list of declared property types to a `Ty`.
///
/// - **0 entries** — the HBK page didn't carry a `Тип:` marker. Return
///   `Ty::Unknown` so downstream inference doesn't claim a type we don't
///   actually know.
/// - **1 entry** — direct `lower_platform_type_name` call (the same
///   mapper method returns use).
/// - **2+ entries** — `Ty::union(...)` of each mapped type. Ensures the
///   TempTablesManager-style `"…, Неопределено"` declarations become
///   `Ty::Union({МенеджерВременныхТаблиц, Неопределено})` instead of a
///   single stringly-typed `PlatformObject("…, Неопределено")`.
fn map_property_type_list(types: &[smol_str::SmolStr]) -> Ty {
    match types.len() {
        0 => Ty::Unknown,
        1 => lower_platform_type_name(types[0].as_str()),
        _ => {
            let mapped: Vec<Ty> =
                types.iter().map(|s| lower_platform_type_name(s.as_str())).collect();
            Ty::union(mapped)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_text_resolves_to_string_writable() {
        // `Запрос.Текст` is the canonical scalar read-write property —
        // platform property_types = ["Строка"], is_readonly = false.
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let res = lookup_platform_property(&receiver, &Name::new("Текст"))
            .expect("Query.Текст must resolve through platform property data");
        assert_eq!(res.return_ty, Ty::String);
        assert!(!res.is_readonly);
    }

    #[test]
    fn query_parameters_resolves_to_structure_readonly() {
        // The headline user scenario — `Запрос.Параметры` must come back as
        // `Ty::Structure` so the chained `.Вставить` lookup in method_lookup
        // can find `Структура.Вставить`. Platform flag: read-only.
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let res = lookup_platform_property(&receiver, &Name::new("Параметры"))
            .expect("Query.Параметры must resolve");
        assert_eq!(res.return_ty, Ty::Structure);
        assert!(res.is_readonly);
    }

    #[test]
    fn query_temp_tables_manager_resolves_to_union() {
        // `Запрос.МенеджерВременныхТаблиц` declares a union
        // `МенеджерВременныхТаблиц, Неопределено`; the mapper must produce
        // `Ty::Union(...)` with both members rather than a stringly-typed
        // `PlatformObject("…, Неопределено")`.
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let res = lookup_platform_property(&receiver, &Name::new("МенеджерВременныхТаблиц"))
            .expect("Query.МенеджерВременныхТаблиц must resolve");
        match res.return_ty {
            Ty::Union(members) => {
                assert_eq!(members.len(), 2);
            }
            other => panic!("Expected Ty::Union for TempTablesManager, got {other:?}"),
        }
        assert!(!res.is_readonly);
    }

    #[test]
    fn bilingual_english_property_name_resolves() {
        // Bilingual keying — `Query.Parameters` (English on both sides) must
        // hit the same property row as `Запрос.Параметры`.
        let receiver = Ty::PlatformObject(Name::new("Query"));
        let res = lookup_platform_property(&receiver, &Name::new("Parameters"))
            .expect("Query.Parameters must resolve via bilingual index");
        assert_eq!(res.return_ty, Ty::Structure);
        assert!(res.is_readonly);
    }

    #[test]
    fn unknown_property_returns_none() {
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        assert!(lookup_platform_property(&receiver, &Name::new("ЗаведомоНесуществующее")).is_none());
    }

    #[test]
    fn metadata_ref_and_manager_receivers_return_none() {
        // MetadataRef / ObjectManager are owned by the MDO / manager
        // adapters. The dispatcher in `field_lookup` routes them there
        // before ever calling us, but as defense-in-depth the adapter
        // itself must also say "not my receiver".
        let mdo = Ty::MetadataRef {
            kind: hir_def::ty::MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };
        assert!(lookup_platform_property(&mdo, &Name::new("Код")).is_none());

        let mgr = Ty::ObjectManager {
            kind: bsl_metadata::MdoType::Catalog,
            name: Name::new("Валюты"),
        };
        assert!(lookup_platform_property(&mgr, &Name::new("Любой")).is_none());
    }

    #[test]
    fn primitive_receivers_return_none() {
        // Primitives have no declared instance properties — `platform_type_key`
        // returns None for them, so we propagate None without ever touching
        // the platform index.
        assert!(lookup_platform_property(&Ty::Number, &Name::new("Любая")).is_none());
        assert!(lookup_platform_property(&Ty::String, &Name::new("Любая")).is_none());
        assert!(lookup_platform_property(&Ty::Boolean, &Name::new("Любая")).is_none());
        assert!(lookup_platform_property(&Ty::Date, &Name::new("Любая")).is_none());
    }

    // ---------- Phase 12: form-control chain walk ----------

    #[test]
    fn form_control_pages_resolves_extension_only_property() {
        // `<Pages>.ТекущаяСтраница` lives in the extension type
        // `Расширение группы формы для страниц` (5 props), NOT in the
        // shared `ГруппаФормы` base. Without the chain walk this would
        // miss — chain.iter().rev() hits the extension first and wins.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Pages, binding: None };
        let res = lookup_platform_property(&receiver, &Name::new("ТекущаяСтраница"))
            .expect("<Pages>.ТекущаяСтраница must resolve through extension chain");
        // ТекущаяСтраница on Pages is writable (per platform_data.json).
        assert!(!res.is_readonly);
    }

    #[test]
    fn form_control_pages_falls_through_to_base_for_shared_property() {
        // `Видимость` lives on the base `ГруппаФормы` — chain walk's
        // extension hit misses, fall through to base wins. Confirms the
        // chain doesn't *only* surface extension members.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Pages, binding: None };
        let res = lookup_platform_property(&receiver, &Name::new("Видимость"))
            .expect("<Pages>.Видимость must fall through to ГруппаФормы base");
        assert_eq!(res.return_ty, Ty::Boolean);
    }

    #[test]
    fn form_control_usual_group_does_not_see_pages_extension() {
        // `ТекущаяСтраница` is exclusive to `<Pages>` extension. A
        // `<UsualGroup>` receiver must NOT resolve it — its chain only
        // includes "Расширение группы формы для обычной группы" which
        // doesn't carry the property.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::UsualGroup, binding: None };
        assert!(
            lookup_platform_property(&receiver, &Name::new("ТекущаяСтраница")).is_none(),
            "UsualGroup chain must not borrow Pages-extension properties"
        );
    }

    #[test]
    fn form_control_input_field_still_resolves_base_only() {
        // Non-regression for scope guard #11 in plan v3.1: kinds that
        // were NOT split (Field/Decoration/Button/Addition) must keep
        // their pre-chain behaviour. `<InputField>.Видимость` resolves
        // through the base `ПолеФормы` table — single-entry chain.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Field, binding: None };
        let res = lookup_platform_property(&receiver, &Name::new("Видимость"))
            .expect("<InputField>.Видимость must resolve via base ПолеФормы");
        assert_eq!(res.return_ty, Ty::Boolean);
    }

    #[test]
    fn form_control_other_returns_none_with_empty_chain() {
        // `Other` chain is &[] — the rev-walk loop runs zero iterations
        // and we return None safely (no panic on empty chain).
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Other, binding: None };
        assert!(lookup_platform_property(&receiver, &Name::new("Видимость")).is_none());
    }
}
