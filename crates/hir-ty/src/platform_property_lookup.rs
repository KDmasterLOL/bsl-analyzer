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
//! `Ty::MetadataRef` is deliberately **not** routed through this adapter —
//! metadata-ref property access walks the MDO's XML attributes in
//! [`crate::field_lookup::lookup_metadata_ref`]. The dispatcher in
//! `lookup_field` decides who owns a receiver; this module never sees a
//! `MetadataRef` input.

use bsl_platform::{PlatformData, PlatformProperty};
use hir_def::ty::Ty;
use hir_def::Name;

use crate::method_lookup::{platform_type_key, resolve_platform_type_name};

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
    let type_key = platform_type_key(receiver_ty)?;
    let data = PlatformData::instance();
    let prop = data.get_property(type_key, prop_name.as_str())?;
    Some(to_resolution(prop))
}

/// Convert a `PlatformProperty` into the semantic [`PlatformPropertyResolution`].
///
/// Mirrors `method_lookup::to_method_info` in shape — same path through
/// `resolve_platform_type_name` for each declared value type, then
/// `Ty::union` when the HBK page declared more than one (e.g.
/// `МенеджерВременныхТаблиц, Неопределено`).
fn to_resolution(prop: &PlatformProperty) -> PlatformPropertyResolution {
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
/// - **1 entry** — direct `resolve_platform_type_name` call (the same
///   mapper method returns use).
/// - **2+ entries** — `Ty::union(...)` of each mapped type. Ensures the
///   TempTablesManager-style `"…, Неопределено"` declarations become
///   `Ty::Union({МенеджерВременныхТаблиц, Неопределено})` instead of a
///   single stringly-typed `PlatformObject("…, Неопределено")`.
fn map_property_type_list(types: &[smol_str::SmolStr]) -> Ty {
    match types.len() {
        0 => Ty::Unknown,
        1 => resolve_platform_type_name(types[0].as_str()),
        _ => {
            let mapped: Vec<Ty> =
                types.iter().map(|s| resolve_platform_type_name(s.as_str())).collect();
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
}
