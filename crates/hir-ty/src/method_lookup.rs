//! Method lookup on a typed receiver.
//!
//! `MethodLookup::resolve(receiver_ty, method_name)` answers the question
//! "given that `x: receiver_ty`, what does `x.method_name(...)` evaluate to?"
//!
//! Before M3 this logic lived in two places:
//! - `hir-ty::infer::resolve_method_return_type` (for inference, consulted
//!   only `PlatformData::get_method` on `platform_type_name()`-bearing `Ty`s);
//! - `ide::completion::platform_completion::resolve_call_expr_type` (for
//!   completion, walked syntax recursively and called `PlatformData` too).
//!
//! Both shared a single invariant — `receiver type + method name → return
//! type` — but each kept its own pipeline. M3 collapses the semantic half
//! into this adapter; the syntax-level completion resolver becomes a thin
//! veneer over it.
//!
//! # Coverage
//!
//! - **Platform value types** (`Ty::PlatformObject`, `Ty::Array`, `Ty::Map`,
//!   `Ty::ValueTable`, `Ty::ValueList`, `Ty::Structure`, `Ty::Type`)
//!   resolve via `PlatformData::get_method(type_name, method)`. Keys are
//!   the English canonical names; the platform index is bilingual, so a
//!   Russian method name still matches.
//! - **`Ty::ObjectManager` / `Ty::MetadataRef`** — **deferred**.
//!   Platform-data stores manager and ref methods with
//!   `type_name = "CatalogManager.<Catalog name>"` /
//!   `"CatalogRef.<Catalog name>"` and mangled per-method
//!   `name` fields (`"<Имя"`, `"<Catalog name>.CreateItem"`), so no
//!   direct method-name index exists. Returning `None` matches what the
//!   pre-M3 `resolve_method_return_type` effectively did; re-enabling
//!   requires either a richer platform-data shape or parsing
//!   `documentation.syntax`.
//! - **`Ty::ManagerCollection(_)`** / **`Ty::Union(_)`** / primitives
//!   (`Number`, `String`, `Boolean`, `Date`) — `None`. Collections only
//!   expose iteration, unions wait for M4 narrowing, and primitives have
//!   no instance methods in BSL (`СтрДлина`, `ДобавитьМесяц` are global
//!   functions, not receiver methods).
//!
//! User-written manager-module methods (`Документы.ПКО.СоздатьДокумент()`)
//! are **not** in scope here — those land as `Expr::Call` of a
//! `QualifiedPath` (3 segments) and already flow through
//! `method_resolution::resolve_three_level_call` → `Resolver`. This module
//! is the platform-side complement for `Expr::MethodCall { receiver, ... }`.

use bsl_platform::{PlatformData, PlatformMethod};
use hir_def::ty::Ty;
use hir_def::Name;

/// Result of a successful method lookup.
///
/// `params` holds the typed parameter list — empty `Vec` means "method
/// takes no arguments". `Ty::Unknown` slots appear when the platform
/// parameter type is omitted or not recognised; inference should treat
/// them as "any".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// Return type. `Ty::Undefined` for procedures (platform methods with
    /// no declared return type).
    pub return_ty: Ty,
    /// Parameter types, in declaration order.
    pub params: Vec<Ty>,
}

/// Resolve a method call on a typed receiver.
///
/// Returns `None` when:
/// - the receiver type carries no platform method table (e.g.
///   `Ty::Unknown`, `Ty::Number`-style primitives that are not platform
///   objects, unions, manager collectives);
/// - the method name does not exist in the resolved table.
pub fn lookup_method(receiver_ty: &Ty, method_name: &Name) -> Option<MethodInfo> {
    let type_key = platform_type_key(receiver_ty)?;
    let data = PlatformData::instance();
    let method = data.get_method(type_key, method_name.as_str())?;
    Some(to_method_info(method))
}

/// Pick the `PlatformData::get_method` key for a receiver.
///
/// The platform-data index uses **English** type names keyed through a
/// bilingual map, so passing the English canonical name resolves methods
/// whose `name` is Russian (and vice versa).
///
/// # What is not covered yet
///
/// `Ty::ObjectManager { kind, .. }` / `Ty::MetadataRef { kind, .. }`
/// method lookup is a deferred gap. Platform-data entries for manager and
/// reference types carry `type_name = "CatalogManager.<Catalog name>"` /
/// `"CatalogRef.<Catalog name>"`, and their per-method `name` / `english_name`
/// fields are placeholders (`"<Имя"`, `"<Catalog name>.CreateItem"`) —
/// no direct method-name index exists. Returning `None` here matches what
/// the pre-M3 `resolve_method_return_type` effectively did (via
/// `platform_type_name()` returning `None` for ObjectManager and a
/// mismatched `"MetadataRef"` key for MetadataRef). Re-enabling these
/// requires either richer platform-data indexing or parsing
/// `documentation.syntax`; tracked for a later milestone.
///
/// Similarly, primitives (`Ty::Number | String | Boolean | Date`) return
/// `None` because BSL exposes no instance methods on them — string/date
/// "methods" are global functions (`СтрДлина`, `ДобавитьМесяц`) reachable
/// only through free-function syntax, not `receiver.method()`.
fn platform_type_key(ty: &Ty) -> Option<&str> {
    match ty {
        // Value types — English canonical names hit the bilingual index.
        Ty::Array => Some("Array"),
        Ty::Structure => Some("Structure"),
        Ty::Map => Some("Map"),
        Ty::ValueTable => Some("ValueTable"),
        Ty::ValueList => Some("ValueList"),
        Ty::Type => Some("Type"),
        // Platform object name is stored as-authored in the `Ty` and the
        // bilingual index translates it.
        Ty::PlatformObject(name) => Some(name.as_str()),
        // Deferred: manager / ref method lookup, primitives, unions,
        // collectives, opaque types.
        Ty::ObjectManager { .. }
        | Ty::MetadataRef { .. }
        | Ty::Number
        | Ty::String
        | Ty::Boolean
        | Ty::Date
        | Ty::ManagerCollection(_)
        | Ty::Union(_)
        | Ty::Unknown
        | Ty::Undefined
        | Ty::Null
        | Ty::Function { .. } => None,
    }
}

/// Convert a `PlatformMethod` entry into the semantic `MethodInfo`.
///
/// - `return_type = Some("Число")` → `Ty::Number` (via
///   `Ty::from_type_name`); unrecognised names fall back to
///   `Ty::PlatformObject(name)` so hover / chained calls still carry a
///   meaningful type.
/// - `return_type = None` → procedure; `Ty::Undefined`.
/// - Parameter types follow the same path; missing `param_type` becomes
///   `Ty::Unknown` to signal "any".
fn to_method_info(method: &PlatformMethod) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| resolve_platform_type_name(ret))
        .unwrap_or(Ty::Undefined);

    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type.as_ref().map(|t| resolve_platform_type_name(t)).unwrap_or(Ty::Unknown)
        })
        .collect();

    MethodInfo { return_ty, params }
}

/// Map a platform type-name string to a `Ty`. Primitives / collections
/// take their canonical variant; anything else becomes
/// `Ty::PlatformObject` so fluent chains like
/// `Запрос.Выполнить().Выбрать()` can continue resolving.
fn resolve_platform_type_name(name: &str) -> Ty {
    let ty = Ty::from_type_name(name);
    if ty.is_unknown() {
        Ty::PlatformObject(Name::new(name))
    } else {
        ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;
    use hir_def::ty::MetadataKind;

    #[test]
    fn method_lookup_platform_type_hit() {
        // `Массив.Добавить` is a staple platform method — proves the
        // type-name key resolves through `PlatformData::get_method`.
        let info = lookup_method(&Ty::Array, &Name::new("Добавить"))
            .expect("Массив.Добавить must resolve in platform data");
        // Returns nothing (procedure) in platform data.
        assert_eq!(info.return_ty, Ty::Undefined);
    }

    #[test]
    fn method_lookup_unknown_method_returns_none() {
        // A nonsensical method name never hits — the lookup is None, not a
        // poisoned `Ty::Unknown` signature.
        assert!(lookup_method(&Ty::Array, &Name::new("НеСуществуетТакогоМетода")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_unknown_receiver() {
        // Unknown receiver has no method table.
        assert!(lookup_method(&Ty::Unknown, &Name::new("Любой")).is_none());
        assert!(lookup_method(&Ty::Undefined, &Name::new("Любой")).is_none());
        assert!(lookup_method(&Ty::Null, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_union() {
        // Unions defer to M4 narrowing — today no method lookup happens.
        let u = Ty::union(vec![Ty::Number, Ty::String]);
        assert!(lookup_method(&u, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_manager_collection() {
        // Collectives (`Документы`) expose iteration, not per-object
        // methods — until collective methods land in bsl-platform this
        // returns None.
        let doc =
            Ty::manager_collection(MdoType::Document).expect("Document has a manager collection");
        assert!(lookup_method(&doc, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_value_table_english_key_hits_russian_method_name() {
        // `ValueTable` entries in platform_data.json use
        // `type_name = "ValueTable"` (English only); the method-lookup
        // index is bilingual, so asking for the Russian method name
        // `Добавить` must still resolve. Pins the fix that replaced the
        // broken Russian-keyed lookup (pre-M3 would have resolved via a
        // `display_name()` fallback, and the Task 5 draft used
        // `"ТаблицаЗначений"` — both miss because platform-data stores
        // English).
        let info = lookup_method(&Ty::ValueTable, &Name::new("Добавить"))
            .expect("ValueTable.Добавить must resolve via bilingual platform index");
        assert!(!matches!(info.return_ty, Ty::Unknown));
    }

    #[test]
    fn method_lookup_object_manager_returns_none_deferred() {
        // ObjectManager method lookup is deferred: platform-data's
        // per-method `name` / `english_name` fields are placeholders
        // (`"<Имя"`, `"<Catalog name>.CreateItem"`) with no real
        // method-name index. Until that data-quality gap is bridged we
        // return None — matching the pre-M3 `resolve_method_return_type`
        // which also returned None via `platform_type_name()`.
        let om = Ty::ObjectManager {
            kind: MdoType::Catalog, name: Name::new("Номенклатура")
        };
        assert!(lookup_method(&om, &Name::new("СоздатьЭлемент")).is_none());
    }

    #[test]
    fn method_lookup_metadata_ref_returns_none_deferred() {
        // Same data-quality story as ObjectManager; tracked together.
        let r = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Номенклатура")
        };
        assert!(lookup_method(&r, &Name::new("ПолучитьОбъект")).is_none());
    }
}
