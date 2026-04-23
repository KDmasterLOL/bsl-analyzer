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
//! - **`Ty::ObjectManager`** / **`Ty::MetadataRef`** (object / ref
//!   flavours) — route through
//!   [`crate::platform_manager_lookup`]. That adapter walks the
//!   platform-data table by `type_name`-prefix (`"CatalogManager.*"`,
//!   `"CatalogObject.*"`, …) and matches the method name against
//!   `docs.syntax` / the `english_name` tail — the shape that pairs
//!   with placeholder `name = "<Имя"` entries. Return types that arrive
//!   as generics (`"СправочникОбъект"`) are rebound to
//!   `Ty::MetadataRef { <kind>, <current mdo_name> }` there.
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
    // `Ty::ThisObject` is coerced to its matching `*Object`
    // `Ty::MetadataRef` at adapter entry — see [`crate::this_object`].
    // The coercion lands on `MetadataRef { *Object, .. }`, which is then
    // picked up by the `platform_manager_lookup` branch below.
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    // Route manager / metadata-ref receivers through the dedicated
    // platform-manager adapter — platform data indexes them with
    // composite `type_name` (`"CatalogManager.<Имя>"`) and
    // placeholder per-method `name`, so the scalar `get_method` path
    // below never hits. Workspace `ManagerModule.bsl` overrides win
    // via `Resolver::resolve_three_level_method` at the 3-segment
    // call site in `infer.rs` — this fallback only runs through
    // `Expr::MethodCall` / aliased-manager shapes, where there is no
    // CFE resolver to consult.
    // `Ty::Union` receivers are the common "happy path + Неопределено"
    // shape from platform return types (e.g. `Запрос.Выполнить()` →
    // `Ty::Union([QueryResult, Undefined])`). Strip `Undefined` / `Null`
    // sentinels (they have no instance methods) and dispatch on each
    // remaining branch; the caller sees a unioned return type so chained
    // calls like `Запрос.Выполнить().Выгрузить()` resolve without waiting
    // for M4 full narrowing.
    if let Ty::Union(members) = receiver_ty {
        let live: Vec<&Ty> =
            members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).collect();
        let mut returns: Vec<Ty> = Vec::with_capacity(live.len());
        let mut params: Vec<Ty> = Vec::new();
        let mut hit_any = false;
        for m in live {
            if let Some(info) = lookup_method(m, method_name) {
                hit_any = true;
                returns.push(info.return_ty);
                // Parameter lists are taken from the first successful
                // branch — platform overloads with differing signatures
                // across union branches are vanishingly rare in HBK data,
                // and inference only consults `params` for arity-style
                // checks where widening is harmless.
                if params.is_empty() {
                    params = info.params;
                }
            }
        }
        return hit_any.then(|| MethodInfo { return_ty: Ty::union(returns), params });
    }

    match receiver_ty {
        Ty::ObjectManager { kind, name } => {
            if let Some(res) = crate::platform_manager_lookup::resolve_platform_manager_method(
                *kind,
                name,
                method_name,
            ) {
                return Some(MethodInfo {
                    return_ty: res.return_ty,
                    params: res.signature.params.to_vec(),
                });
            }
            return None;
        }
        Ty::MetadataRef { kind, name } => {
            if let Some(res) = crate::platform_manager_lookup::resolve_platform_metadata_ref_method(
                *kind,
                name,
                method_name,
            ) {
                return Some(MethodInfo {
                    return_ty: res.return_ty,
                    params: res.signature.params.to_vec(),
                });
            }
            // MetadataRef flavours without a platform surface (register
            // dimensions, tabular sections) fall through `None` — same
            // behaviour as before the adapter existed.
            return None;
        }
        _ => {}
    }

    let type_key = platform_type_key(receiver_ty)?;
    let data = PlatformData::instance();
    let method = data.get_method(type_key, method_name.as_str())?;
    Some(to_method_info(method))
}

/// Pick the `PlatformData::get_method` key for a scalar receiver.
///
/// The platform-data index uses **English** type names keyed through a
/// bilingual map, so passing the English canonical name resolves methods
/// whose `name` is Russian (and vice versa).
///
/// `Ty::ObjectManager` / `Ty::MetadataRef` are **not** routed through
/// this key — their platform entries carry composite
/// `type_name = "CatalogManager.<Имя>"` with placeholder per-method
/// `name = "<Имя"`, which the scalar index cannot serve. Those
/// receivers are handled upstream in [`lookup_method`] via
/// [`crate::platform_manager_lookup`].
///
/// Primitives (`Ty::Number | String | Boolean | Date`) return `None`
/// because BSL exposes no instance methods on them — string/date
/// "methods" are global functions (`СтрДлина`, `ДобавитьМесяц`) reachable
/// only through free-function syntax, not `receiver.method()`.
pub(crate) fn platform_type_key(ty: &Ty) -> Option<&str> {
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
        // Manager / ref receivers are resolved earlier in
        // [`lookup_method`] via `platform_manager_lookup` — this arm
        // is unreachable in practice, returning `None` preserves the
        // invariant "scalar key only" for any future fall-through.
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
        // `ThisObject` is coerced to its matching `Ty::MetadataRef`
        // companion at the entry of [`lookup_method`] (see
        // `crate::this_object::coerce_to_metadata_ref`), which the
        // `MetadataRef` branch above then routes through
        // `platform_manager_lookup`. A receiver that still lands here
        // did not match a coercible MDO kind — `None` is the safe
        // fallback.
        Ty::ThisObject { .. } => None,
    }
}

/// Convert a `PlatformMethod` entry into the semantic `MethodInfo`.
///
/// - `return_type = Some("Число")` → `Ty::Number` (via
///   `Ty::from_type_name`); unrecognised names fall back to
///   `Ty::PlatformObject(name)` so hover / chained calls still carry a
///   meaningful type.
/// - `return_type = Some("РезультатЗапроса, Неопределено")` — comma-joined
///   union strings emitted by the HBK scraper when a method returns one of
///   several types (happy-path + null sentinel). Split on `,`, map each
///   segment via `resolve_platform_type_name`, and feed the list to
///   `Ty::union` so chained calls see `Ty::Union([QueryResult, Undefined])`
///   instead of a poisoned `Ty::PlatformObject("... ,Неопределено")`.
/// - `return_type = None` → procedure; `Ty::Undefined`.
/// - Parameter types are kept as raw scalars for now; malformed comma-heavy
///   HBK prose stays a single `Ty::PlatformObject(...)` instead of poisoning
///   argument checks with bogus union members.
fn to_method_info(method: &PlatformMethod) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| resolve_platform_type_union(ret))
        .unwrap_or(Ty::Undefined);

    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| p.param_type.as_ref().map(|t| Ty::from_type_name(t)).unwrap_or(Ty::Unknown))
        .collect();

    MethodInfo { return_ty, params }
}

/// Split a comma-joined platform type string into a `Ty::union(...)`.
///
/// Only comma-joined strings whose every segment resolves to a known structured
/// type or sentinel are treated as a union. Free-prose commas fall back to a
/// single raw platform object type.
fn resolve_platform_type_union(raw: &str) -> Ty {
    let segments: Vec<&str> = raw.split(',').map(str::trim).collect();
    if segments.iter().any(|segment| !segment_is_valid_type(segment)) {
        resolve_platform_type_name(raw)
    } else {
        Ty::union(segments.into_iter().map(resolve_platform_type_name).collect())
    }
}

fn segment_is_valid_type(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let ty = Ty::from_type_name(s);
    !ty.is_unknown() || PlatformData::instance().get_type(s).is_some()
}

/// Map a platform type-name string to a `Ty`. Primitives / collections
/// take their canonical variant; anything else becomes
/// `Ty::PlatformObject` so fluent chains like
/// `Запрос.Выполнить().Выбрать()` can continue resolving.
pub(crate) fn resolve_platform_type_name(name: &str) -> Ty {
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
    use bsl_platform::MethodParam;
    use hir_def::ty::MetadataKind;

    fn test_method(return_type: Option<&str>, param_type: Option<&str>) -> PlatformMethod {
        PlatformMethod {
            id: 0,
            type_name: "TestType".into(),
            name: "Тест".into(),
            english_name: "Test".into(),
            return_type: return_type.map(Into::into),
            parameters: vec![MethodParam {
                name: "Параметр".into(),
                param_type: param_type.map(Into::into),
                is_optional: false,
            }],
            min_version: None,
            context: None,
        }
    }

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
    fn method_lookup_returns_none_for_union_without_live_method() {
        // Primitives expose no instance methods → union of primitives still
        // returns `None` for any method name (every branch misses).
        let u = Ty::union(vec![Ty::Number, Ty::String]);
        assert!(lookup_method(&u, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_union_narrows_past_undefined_sentinel() {
        // `Запрос.Выполнить()` returns `"РезультатЗапроса, Неопределено"`
        // → `Ty::Union([QueryResult, Undefined])`. Chained
        // `.Выгрузить()` must resolve against the live branch (QueryResult
        // has a `Выгрузить` method in platform data) — the `Undefined`
        // sentinel is stripped before dispatch so the chain survives.
        let u = Ty::union(vec![Ty::PlatformObject(Name::new("РезультатЗапроса")), Ty::Undefined]);
        let info = lookup_method(&u, &Name::new("Выгрузить")).expect(
            "Union([QueryResult, Undefined]).Выгрузить must resolve through the live branch",
        );
        // HBK declares `QueryResult.Выгрузить` as `"ТаблицаЗначений,
        // ДеревоЗначений"` — the narrowing must at least preserve
        // `Ty::ValueTable` in the result so chained `.Добавить()` works.
        let contains_value_table = match &info.return_ty {
            Ty::ValueTable => true,
            Ty::Union(members) => members.iter().any(|m| matches!(m, Ty::ValueTable)),
            _ => false,
        };
        assert!(
            contains_value_table,
            "return type must include Ty::ValueTable, got {:?}",
            info.return_ty,
        );
    }

    #[test]
    fn method_lookup_platform_object_query_execute_direct() {
        // Direct check: `Ty::PlatformObject("Запрос").Выполнить` must
        // resolve — the receiver shape that inference produces when
        // `Зап = Новый Запрос`. If this asserts None while the bilingual
        // test below passes, inference is hitting a different code path.
        let info = lookup_method(&Ty::PlatformObject(Name::new("Запрос")), &Name::new("Выполнить"));
        assert!(info.is_some(), "PlatformObject(Запрос).Выполнить must resolve");
    }

    #[test]
    fn method_lookup_query_execute_returns_union_with_undefined() {
        // Pins the HBK shape for `Запрос.Выполнить` — return_type is the
        // comma-joined string `"РезультатЗапроса, Неопределено"`. After
        // `to_method_info` splits it we must see both branches as the
        // receiver for any downstream chain.
        let info = lookup_method(&Ty::PlatformObject(Name::new("Запрос")), &Name::new("Выполнить"))
            .expect("Запрос.Выполнить must resolve in platform data");
        match info.return_ty {
            Ty::Union(members) => {
                assert!(
                    members.iter().any(|m| matches!(
                        m,
                        Ty::PlatformObject(n) if n.as_str().eq_ignore_ascii_case("РезультатЗапроса")
                    )),
                    "union must include РезультатЗапроса, got {members:?}",
                );
                assert!(
                    members.iter().any(|m| matches!(m, Ty::Undefined)),
                    "union must include Ty::Undefined, got {members:?}",
                );
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn resolve_platform_type_union_falls_back_for_prose_commas() {
        assert_eq!(
            resolve_platform_type_union("ТабличныйДокумент, ТекстовыйДокумент; другой объект"),
            Ty::PlatformObject(Name::new("ТабличныйДокумент, ТекстовыйДокумент; другой объект",)),
        );
        assert_eq!(
            resolve_platform_type_union("Ссылка на объект, либо"),
            Ty::PlatformObject(Name::new("Ссылка на объект, либо")),
        );
        assert_eq!(
            resolve_platform_type_union("Метаданные,"),
            Ty::PlatformObject(Name::new("Метаданные,")),
        );
        assert_eq!(resolve_platform_type_union(", ,"), Ty::PlatformObject(Name::new(", ,")));
    }

    #[test]
    fn to_method_info_keeps_param_type_as_single_raw_value() {
        let info = to_method_info(&test_method(Some("Число, Неопределено"), Some("Метаданные,")));
        assert_eq!(info.return_ty, Ty::union(vec![Ty::Number, Ty::Undefined]));
        assert_eq!(info.params, vec![Ty::Unknown]);
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
    fn method_lookup_object_manager_resolves_through_platform_manager_adapter() {
        // `ObjectManager { Catalog, "Номенклатура" }.СоздатьЭлемент()`
        // routes through `platform_manager_lookup` — the generic
        // `СправочникОбъект` return must rebind to
        // `MetadataRef { CatalogObject, "Номенклатура" }`.
        let om = Ty::ObjectManager {
            kind: MdoType::Catalog, name: Name::new("Номенклатура")
        };
        let info = lookup_method(&om, &Name::new("СоздатьЭлемент"))
            .expect("ObjectManager.СоздатьЭлемент must resolve via platform adapter");
        assert_eq!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogObject, name: Name::new("Номенклатура")
            }
        );
    }

    #[test]
    fn method_lookup_object_manager_unknown_method_returns_none() {
        // Fabricated method — no platform entry, lookup still returns
        // `None` so the caller emits `UnresolvedMethodCall`.
        let om = Ty::ObjectManager {
            kind: MdoType::Catalog, name: Name::new("Номенклатура")
        };
        assert!(lookup_method(&om, &Name::new("НетТакогоМетода")).is_none());
    }

    #[test]
    fn method_lookup_metadata_ref_catalog_object_resolves_write() {
        // `MetadataRef { CatalogObject, .. }.Записать()` routes to the
        // CatalogObject platform method (procedure — return is
        // `Ty::Undefined`).
        let r = Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Номенклатура"),
        };
        let info = lookup_method(&r, &Name::new("Записать"))
            .expect("MetadataRef CatalogObject.Записать must resolve");
        assert_eq!(info.return_ty, Ty::Undefined);
    }
}
