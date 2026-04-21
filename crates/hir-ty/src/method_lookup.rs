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
//!   …) → `PlatformData::get_method(type_name, method)`.
//! - **`Ty::ObjectManager { kind, name }`** → look up the method on the
//!   manager type (`DocumentManager`, `CatalogManager`, …) via
//!   `PlatformData::get_method(prefix, method)`. Supersedes the `None`
//!   branch the old `resolve_method_return_type` took on manager values.
//! - **`Ty::MetadataRef { kind, name }`** → look up the method on the
//!   reference / object type (`CatalogRef`, `DocumentObject`, …).
//! - **`Ty::ManagerCollection(_)`** / **`Ty::Union(_)`** — `None` today.
//!   Collections have only iteration semantics; unions wait for M4
//!   narrowing to pick a concrete branch before method lookup makes sense.
//!
//! User-written manager-module methods (`Документы.ПКО.СоздатьДокумент()`)
//! are **not** in scope here — those land as `Expr::Call` of a
//! `QualifiedPath` (3 segments) and already flow through
//! `method_resolution::resolve_three_level_call` → `Resolver`. This module
//! is the platform-side complement for `Expr::MethodCall { receiver, ... }`.

use bsl_platform::{PlatformData, PlatformMethod};
use hir_def::ty::{MetadataKind, Ty};
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
    let platform_method = data.get_method(type_key, method_name.as_str())?;
    Some(to_method_info(platform_method))
}

/// Pick a `&str` key the platform's `get_method` can consume. The slice
/// borrows from either the `Ty` itself (for `PlatformObject`) or from a
/// table of static prefixes (primitives, managers, metadata-refs).
fn platform_type_key(ty: &Ty) -> Option<&str> {
    match ty {
        // Concrete value types — primitives that have platform methods.
        Ty::Array => Some("Массив"),
        Ty::Structure => Some("Структура"),
        Ty::Map => Some("Соответствие"),
        Ty::ValueTable => Some("ТаблицаЗначений"),
        Ty::ValueList => Some("СписокЗначений"),
        Ty::Type => Some("Тип"),
        // Platform object name comes from the `Ty` itself.
        Ty::PlatformObject(name) => Some(name.as_str()),
        // Manager values — the key is the manager type prefix (matches
        // `MdoType::manager_type_prefix` for `ObjectManager` and a
        // MetadataKind-keyed table for refs / objects).
        Ty::ObjectManager { kind, .. } => kind.manager_type_prefix(),
        Ty::MetadataRef { kind, .. } => Some(metadata_kind_platform_name(*kind)),
        // Collections / unions / primitives-without-methods / unknown →
        // no method table.
        Ty::ManagerCollection(_)
        | Ty::Union(_)
        | Ty::Unknown
        | Ty::Undefined
        | Ty::Null
        | Ty::Number
        | Ty::String
        | Ty::Boolean
        | Ty::Date
        | Ty::Function { .. } => None,
    }
}

/// Platform-data type name for a `MetadataKind`. Each ref/object kind maps
/// to the same English token `bsl-platform` uses in
/// `PlatformMethod::type_name`.
fn metadata_kind_platform_name(kind: MetadataKind) -> &'static str {
    match kind {
        MetadataKind::CatalogRef => "CatalogRef",
        MetadataKind::CatalogObject => "CatalogObject",
        MetadataKind::DocumentRef => "DocumentRef",
        MetadataKind::DocumentObject => "DocumentObject",
        MetadataKind::EnumRef => "EnumRef",
        MetadataKind::TaskRef => "TaskRef",
        MetadataKind::BusinessProcessRef => "BusinessProcessRef",
        MetadataKind::InformationRegisterRef => "InformationRegisterRef",
        MetadataKind::InformationRegisterRecordManager => "InformationRegisterRecordManager",
        MetadataKind::AccumulationRegisterRef => "AccumulationRegisterRef",
        MetadataKind::AccumulationRegisterRecordSet => "AccumulationRegisterRecordSet",
        MetadataKind::AccountingRegisterRef => "AccountingRegisterRef",
        MetadataKind::CalculationRegisterRef => "CalculationRegisterRef",
        // Tabular sections surface methods through `PlatformData`'s own
        // collection tables — use the canonical Russian names here, matching
        // the `platform_data.json` entries for `ТабличнаяЧасть` and
        // `СтрокаТабличнойЧасти`.
        MetadataKind::TabularSection => "ТабличнаяЧасть",
        MetadataKind::TabularSectionRow => "СтрокаТабличнойЧасти",
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
    fn platform_type_key_for_object_manager_uses_manager_prefix() {
        // The key path for an ObjectManager receiver must be the manager
        // prefix from bsl-metadata, matching how platform method tables
        // store the data.
        let om = Ty::ObjectManager { kind: MdoType::Document, name: Name::new("ПКО") };
        assert_eq!(platform_type_key(&om), Some("DocumentManager"));
    }

    #[test]
    fn platform_type_key_for_metadata_ref_uses_english_prefix() {
        // `MetadataRef { CatalogRef, "Номенклатура" }` is looked up under
        // the canonical English token `CatalogRef` — matches the platform
        // method table.
        let r = Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Номенклатура")
        };
        assert_eq!(platform_type_key(&r), Some("CatalogRef"));

        let tab = Ty::MetadataRef {
            kind: MetadataKind::TabularSection,
            name: Name::new("ПКО.Товары"),
        };
        assert_eq!(platform_type_key(&tab), Some("ТабличнаяЧасть"));
    }
}
