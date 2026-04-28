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

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformMethod};
use hir_def::ty::{MetadataKind, Ty};
use hir_def::Name;

/// Result of a successful method lookup.
///
/// `params` holds the typed parameter list — empty `Vec` means "method
/// takes no arguments". `Ty::Unknown` slots appear when the platform
/// parameter type is omitted or not recognised; inference should treat
/// them as "any".
///
/// `overloads` carries per-variant parameter lists for multi-overload
/// methods — populated when the platform JSON declares multiple
/// `Вариант синтаксиса:` sections (e.g. `ЧтениеXML.ПолучитьАтрибут`,
/// `ТаблицаЗначений.Скопировать`). Empty otherwise — the single
/// signature lives in `params`. Argument-type checks accept the call
/// when ANY overload accepts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// Return type. `Ty::Undefined` for procedures (platform methods with
    /// no declared return type).
    pub return_ty: Ty,
    /// Parameter types, in declaration order — flat union across
    /// overloads. Used by hover / completion / single-signature
    /// fallbacks.
    pub params: Vec<Ty>,
    /// Per-overload parameter lists. Empty for single-overload methods.
    pub overloads: Vec<Vec<Ty>>,
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
        // `params` and `overloads` MUST come from the same branch — if
        // we let `params` win on the first hit and `overloads` on the
        // second, callers would type-check args against overloads from
        // a receiver shape that the chosen `params` does not represent.
        // Cohesion rule: bind the FIRST successful branch's signature
        // wholesale and ignore later branches' signatures (only their
        // return types contribute to the union).
        let mut chosen_signature: Option<(Vec<Ty>, Vec<Vec<Ty>>)> = None;
        let mut hit_any = false;
        for m in live {
            if let Some(info) = lookup_method(m, method_name) {
                hit_any = true;
                returns.push(info.return_ty);
                if chosen_signature.is_none() {
                    chosen_signature = Some((info.params, info.overloads));
                }
            }
        }
        let (params, overloads) = chosen_signature.unwrap_or_default();
        return hit_any.then(|| MethodInfo { return_ty: Ty::union(returns), params, overloads });
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
                    // Manager-method resolution feeds `MethodResolution`
                    // (single-signature); per-variant data lives only on
                    // `PlatformMethod` and reaches us through the scalar
                    // path below.
                    overloads: Vec::new(),
                });
            }
            return None;
        }
        Ty::MetadataRef { kind, name } => {
            // TabularSection has a flat `type_name = "Tabular section"` in
            // platform_data — no `"Prefix.<MDO>"` shape — so it cannot be
            // served by `platform_manager_lookup::find_prefixed_method`
            // (which requires a dot-separated prefix). Route directly to
            // the bilingual scalar index and rebind the generic
            // `"Строка табличной части"` return to a row receiver so the
            // chain `ТЧ.Добавить().Атрибут` continues resolving via
            // `field_lookup::lookup_on_tabular_row`.
            if let MetadataKind::TabularSection { parent } = *kind {
                let method =
                    PlatformData::instance().get_method("Tabular section", method_name.as_str())?;
                return Some(build_tabular_section_method_info(method, parent, name));
            }
            if let Some(res) = crate::platform_manager_lookup::resolve_platform_metadata_ref_method(
                *kind,
                name,
                method_name,
            ) {
                return Some(MethodInfo {
                    return_ty: res.return_ty,
                    params: res.signature.params.to_vec(),
                    overloads: Vec::new(),
                });
            }
            // MetadataRef flavours without a platform surface (register
            // dimensions, the bare `TabularSectionRow` row receiver) fall
            // through `None`. Row methods do not exist in HBK data —
            // `Удалить(Индекс)` and friends are methods on the section,
            // not on rows.
            return None;
        }
        _ => {}
    }

    let type_key = platform_type_key(receiver_ty)?;
    let data = PlatformData::instance();
    let method = data.get_method(type_key, method_name.as_str())?;
    Some(to_method_info(method))
}

/// Like [`lookup_method`], but additionally returns the `PlatformData`
/// type-name key that was used to resolve the method.
///
/// Consumers (IDE hover, goto, etc.) need the type-name string both to
/// build a [`hir_def::Definition::BuiltinMethod`]-equivalent identifier
/// and to feed `MethodLookupInput` for hover-markdown rendering. The
/// public entry point is intentionally separate from [`lookup_method`]
/// so the inference path keeps its narrow `Option<MethodInfo>` shape.
///
/// For [`Ty::Union`] receivers, returns the **first live member's** key
/// alongside the unioned [`MethodInfo`] (matching `lookup_method`'s
/// "first hit owns the params" semantics).
///
/// [`Ty::ObjectManager`] / [`Ty::MetadataRef`] receivers return [`None`]
/// — those resolve through [`crate::platform_manager_lookup`] which
/// keys on composite `"<Kind>Manager.<Имя>"` strings that the scalar
/// hover path does not understand. Hover for those receivers is served
/// by other paths (see `crates/ide/src/hover.rs`); this entry point is
/// reserved for value-type receivers.
pub fn lookup_method_with_key(
    receiver_ty: &Ty,
    method_name: &Name,
) -> Option<(String, MethodInfo)> {
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    if let Ty::Union(members) = receiver_ty {
        let live: Vec<&Ty> =
            members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).collect();
        let mut returns: Vec<Ty> = Vec::with_capacity(live.len());
        // Same cohesion rule as in `lookup_method`: bind params /
        // overloads / key to the FIRST successful union branch, so the
        // signature shapes always match the receiver they describe.
        let mut chosen_signature: Option<(Vec<Ty>, Vec<Vec<Ty>>)> = None;
        let mut first_key: Option<String> = None;
        for m in live {
            if let Some((k, info)) = lookup_method_with_key(m, method_name) {
                returns.push(info.return_ty);
                if chosen_signature.is_none() {
                    chosen_signature = Some((info.params, info.overloads));
                    first_key = Some(k);
                }
            }
        }
        let (params, overloads) = chosen_signature.unwrap_or_default();
        return first_key
            .map(|k| (k, MethodInfo { return_ty: Ty::union(returns), params, overloads }));
    }

    if matches!(receiver_ty, Ty::ObjectManager { .. } | Ty::MetadataRef { .. }) {
        return None;
    }

    let type_key = platform_type_key(receiver_ty)?;
    let data = PlatformData::instance();
    let method = data.get_method(type_key, method_name.as_str())?;
    Some((type_key.to_string(), to_method_info(method)))
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
        .map(|p| p.param_type.as_ref().map(|t| lower_param_type(t)).unwrap_or(Ty::Unknown))
        .collect();

    // Per-overload param lists for multi-overload methods
    // (`ЧтениеXML.ПолучитьАтрибут` etc.). Empty when the platform JSON
    // declares a single signature — `params` already covers it.
    let overloads: Vec<Vec<Ty>> = method
        .variants
        .iter()
        .map(|v| {
            v.parameters
                .iter()
                .map(|p| p.param_type.as_ref().map(|t| lower_param_type(t)).unwrap_or(Ty::Unknown))
                .collect()
        })
        .collect();

    MethodInfo { return_ty, params, overloads }
}

/// Lower a platform-method `param_type` string to a [`Ty`].
///
/// **Asymmetric with return-type lowering, on purpose.** Returns route
/// through [`resolve_platform_type_union`] so chained receivers carry
/// every possible shape (e.g. `Запрос.Выполнить()` →
/// `Ty::Union([QueryResult, Undefined])`). Params live on the **left**
/// of `is_assignable` checks at the call site, where structural
/// equality on `Ty::PlatformObject` would false-fire on legitimate
/// looser actuals — so we keep gradual-typing (`Ty::Unknown`)
/// reachable.
///
/// Three cases, matching what BSL's loose param semantics expect:
///
/// 1. **Single recognised primitive / collection** (`"Строка"`,
///    `"ТаблицаЗначений"`, …): `Ty::from_type_name` lowers to the
///    canonical variant (`Ty::String`, `Ty::ValueTable`, …).
/// 2. **Single unrecognised name** (`"Строка табличной части"`,
///    `"Произвольный"`): stays `Ty::Unknown`. Gradual typing
///    (`Unknown ≤ A`) accepts any actual — mirrors the pre-fix
///    `Ty::from_type_name` behaviour and keeps cross-section transfers
///    (`ТЧ1.Индекс(ТЧ2.Получить(0))`) and `Undefined`-tolerant unions
///    (`ТЧ.Индекс(ТЧ.Найти(…))`) quiet.
/// 3. **Comma-joined string with EVERY segment a recognised type** —
///    the headline fix. `"Число, Строка, КолонкаТаблицыЗначений"` lowers
///    to `Ty::union([Number, String, PlatformObject("…")])` so
///    `ТаблицаЗначений.ВыгрузитьКолонку(<Колонка>)` accepts every
///    legitimate variant. `is_assignable` distributes correctly over a
///    right-side `Ty::Union`.
///
/// Comma-joined strings where any segment fails validation
/// (`"Ссылка на объект, либо"`, `"Метаданные,"`) collapse to
/// `Ty::Unknown` — they are prose-with-commas or scraper garbage, not
/// a real type list, and routing them through `resolve_platform_type_union`
/// would lift the whole raw string to a strict `Ty::PlatformObject` and
/// false-fire `TypeMismatch`.
fn lower_param_type(raw: &str) -> Ty {
    if !raw.contains(',') {
        return Ty::from_type_name(raw);
    }

    // Multi-segment: only treat as a union when EVERY segment is a
    // recognised type. `is_arbitrary_type_name` collapses BSL's
    // "any value" placeholder anywhere in the list back to
    // `Ty::Unknown` (delegated to `resolve_platform_type_union`).
    let segments: Vec<&str> = raw.split(',').map(str::trim).collect();
    let all_valid = !segments.is_empty()
        && segments.iter().all(|seg| !seg.is_empty() && segment_is_valid_type(seg));

    if all_valid {
        resolve_platform_type_union(raw)
    } else {
        Ty::Unknown
    }
}

/// Build a `MethodInfo` from a `PlatformData["Tabular section"]` entry,
/// rebinding the generic `"Строка табличной части"` (or its English alias
/// `"Line of a tabular section"`) **in the return type** to the concrete
/// `Ty::MetadataRef { TabularSectionRow { parent }, section_name.clone() }`
/// so chained calls (`ТЧ.Добавить().Реквизит` etc.) keep resolving.
///
/// `Ty::Union` walks recursively so the `Найти` return
/// `"Строка табличной части, Неопределено"` becomes
/// `Ty::Union([MetadataRef { TabularSectionRow { parent }, section_name }, Undefined])`.
/// Other types (`Ty::Number`, `Ty::Array`, `Ty::ValueTable`,
/// `Ty::Undefined`) pass through untouched.
///
/// **Parameters are deliberately *not* rebound.** Mirrors the
/// `to_method_info` baseline (`Ty::from_type_name`) so unrecognised
/// platform names like `"Произвольный"` (Найти's first arg, "any
/// value") and `"Строка табличной части"` (Индекс's arg) lower to
/// `Ty::Unknown`. Rebinding params would narrow them to
/// `Ty::MetadataRef { TabularSectionRow { parent }, "<this section>" }`
/// — and [`crate::subtype::is_assignable`] uses structural equality on
/// `MetadataRef`, so any legitimate cross-section transfer
/// (`ТЧ1.Индекс(ТЧ2.Получить(0))`) or possibly-Undefined `Ty::Union`
/// argument (`ТЧ.Индекс(ТЧ.Найти(...))`) would be falsely rejected. The
/// gradual-typing rule (`Unknown ≤ A`) keeps these calls quiet.
fn build_tabular_section_method_info(
    method: &PlatformMethod,
    parent: MdoType,
    section_name: &Name,
) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| rewrite_row_generic(resolve_platform_type_union(ret), parent, section_name))
        .unwrap_or(Ty::Undefined);

    // Same conservative param lowering as `to_method_info` — see
    // [`lower_param_type`] for the multi-type-vs-single-type
    // asymmetry rationale. We deliberately do NOT pipe through
    // `rewrite_row_generic` for params (function-doc on
    // `build_tabular_section_method_info` above explains why).
    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| p.param_type.as_ref().map(|t| lower_param_type(t.as_str())).unwrap_or(Ty::Unknown))
        .collect();

    // Tabular-section methods can also be multi-overload
    // (e.g. `ТаблицаЗначений.Скопировать` has 4 variants). Same
    // conservative lowering — no row-generic rebinding for params.
    let overloads: Vec<Vec<Ty>> = method
        .variants
        .iter()
        .map(|v| {
            v.parameters
                .iter()
                .map(|p| {
                    p.param_type
                        .as_ref()
                        .map(|t| lower_param_type(t.as_str()))
                        .unwrap_or(Ty::Unknown)
                })
                .collect()
        })
        .collect();

    MethodInfo { return_ty, params, overloads }
}

fn rewrite_row_generic(ty: Ty, parent: MdoType, section_name: &Name) -> Ty {
    match ty {
        Ty::PlatformObject(ref n) if is_tabular_row_type_name(n.as_str()) => Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent },
            name: section_name.clone(),
        },
        Ty::Union(members) => Ty::union(
            members.iter().map(|m| rewrite_row_generic(m.clone(), parent, section_name)).collect(),
        ),
        other => other,
    }
}

/// Match the platform return / param string for a TS row, accepting both
/// canonical Russian and English spellings. Uses lowercase comparison so
/// future scraper-emitted casing variants (`строка табличной части`)
/// also match — `eq_ignore_ascii_case` would not fold Cyrillic.
fn is_tabular_row_type_name(name: &str) -> bool {
    let lc = name.to_lowercase();
    lc == "строка табличной части" || lc == "line of a tabular section"
}

/// Split a comma-joined platform type string into a `Ty::union(...)`.
///
/// Only comma-joined strings whose every segment resolves to a known structured
/// type or sentinel are treated as a union. Free-prose commas fall back to a
/// single raw platform object type.
///
/// If any segment is the BSL "any value" placeholder
/// (`Произвольный` / `Arbitrary`), the whole result collapses to
/// [`Ty::Unknown`]: the union "any value ∪ X" is degenerate, and routing
/// it through `Ty::union([Unknown, X])` would not help — `is_assignable`
/// distributes over a left-side union (see [`crate::subtype`]), so a
/// concrete sibling like `Ty::Undefined` would still false-fire
/// `TypeMismatch` against typed sinks. Mirrors the single-token early
/// exit in [`resolve_platform_type_name`].
fn resolve_platform_type_union(raw: &str) -> Ty {
    let segments: Vec<&str> = raw.split(',').map(str::trim).collect();
    if segments.iter().any(|segment| is_arbitrary_type_name(segment)) {
        return Ty::Unknown;
    }
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
///
/// `"Произвольный"` / `"Arbitrary"` is the BSL spec's "any value"
/// placeholder (`Константы.X.Получить()` returns it; `Найти(...)`
/// accepts it) and must lower to [`Ty::Unknown`] so the gradual-typing
/// rule in [`crate::subtype::is_assignable`] accepts the value in any
/// typed slot. Without this early exit the catch-all below would lift
/// it to `Ty::PlatformObject("Произвольный")` and structural equality
/// against `String` / `Number` / etc. would false-fire `TypeMismatch`.
pub(crate) fn resolve_platform_type_name(name: &str) -> Ty {
    if is_arbitrary_type_name(name) {
        return Ty::Unknown;
    }
    let ty = Ty::from_type_name(name);
    if ty.is_unknown() {
        Ty::PlatformObject(Name::new(name))
    } else {
        ty
    }
}

/// Whether `name` is the BSL "any value" placeholder. Lowercase
/// comparison so future scraper-emitted casing variants
/// (`произвольный`) also match — `eq_ignore_ascii_case` would not
/// fold Cyrillic.
fn is_arbitrary_type_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.eq_ignore_ascii_case("Arbitrary") || trimmed.to_lowercase() == "произвольный"
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
            variants: Vec::new(),
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
    fn resolve_platform_type_union_collapses_arbitrary_segment_to_unknown() {
        // BSL's "any value" placeholder anywhere in a comma-joined return
        // string makes the whole union degenerate. Collapsing to
        // `Ty::Unknown` keeps the gradual rule firing on typed sinks; a
        // `Ty::union([Unknown, Undefined])` would *not* — `is_assignable`
        // distributes on the left and `Undefined ≤ String` is false, so
        // `ПустаяСтрока(...)` against a `"Произвольный, Неопределено"`
        // return would still false-fire `TypeMismatch`. Pinned platform
        // entries: `LoadDefaultSetting` and the second `Произвольный,
        // Неопределено` site in platform_data.json.
        assert_eq!(resolve_platform_type_union("Произвольный, Неопределено"), Ty::Unknown);
        assert_eq!(resolve_platform_type_union("Неопределено, Произвольный"), Ty::Unknown);
        assert_eq!(resolve_platform_type_union("Arbitrary, Undefined"), Ty::Unknown);
        assert_eq!(resolve_platform_type_union("Undefined, Arbitrary"), Ty::Unknown);
        // Whitespace folding around the placeholder.
        assert_eq!(resolve_platform_type_union("  Произвольный  ,  Неопределено"), Ty::Unknown);
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
    fn to_method_info_lowers_param_type_asymmetrically() {
        // Returns and params lower differently on purpose — see
        // [`lower_param_type`]. For garbage / scraper-prose with a
        // stray comma (`"Метаданные,"`), the return path lifts to
        // `Ty::PlatformObject("Метаданные,")` (any future receiver
        // chain is best-effort), but the param path stays
        // `Ty::Unknown` so the call-site `is_assignable` check accepts
        // any actual via gradual typing rather than false-firing
        // structural-equality `TypeMismatch`.
        let info = to_method_info(&test_method(Some("Число, Неопределено"), Some("Метаданные,")));
        assert_eq!(info.return_ty, Ty::union(vec![Ty::Number, Ty::Undefined]));
        assert_eq!(
            info.params,
            vec![Ty::Unknown],
            "garbage param strings stay Unknown for gradual typing",
        );
    }

    #[test]
    fn to_method_info_prose_comma_param_stays_unknown() {
        // Real-world `param_type` strings emitted by the HBK scraper
        // include human-prose descriptions with commas
        // (e.g. `"Ссылка на объект, либо Уникальный идентификатор"`).
        // These must NOT be misread as a type union — at least one
        // segment fails type validation, so we collapse to
        // `Ty::Unknown` rather than a strict
        // `Ty::PlatformObject("Ссылка на объект, либо …")`.
        let info = to_method_info(&test_method(None, Some("Ссылка на объект, либо")));
        assert_eq!(info.params, vec![Ty::Unknown]);
    }

    #[test]
    fn to_method_info_single_unknown_param_stays_unknown() {
        // The asymmetry in [`lower_param_type`] preserves gradual
        // typing for SINGLE-name params whose name `Ty::from_type_name`
        // doesn't recognise — `Ty::Unknown` accepts any actual at the
        // call-site argument check. Routing this through
        // `resolve_platform_type_union` would lift to a structural
        // `PlatformObject`, falsely rejecting valid args.
        let info = to_method_info(&test_method(None, Some("Строка табличной части")));
        assert_eq!(info.params, vec![Ty::Unknown]);
    }

    #[test]
    fn to_method_info_multi_type_param_lowers_to_union() {
        // Headline regression: `ТаблицаЗначений.ВыгрузитьКолонку.Колонка`
        // is `"Число, Строка, КолонкаТаблицыЗначений"`. After the
        // multi-type lowering fix, it surfaces as a `Ty::Union` so the
        // `is_assignable` check at the call site accepts a `Ty::String`
        // arg without false-firing `TypeMismatch`.
        let info =
            to_method_info(&test_method(None, Some("Число, Строка, КолонкаТаблицыЗначений")));
        match &info.params[..] {
            [Ty::Union(members)] => {
                assert!(members.contains(&Ty::Number));
                assert!(members.contains(&Ty::String));
                assert!(members.iter().any(
                    |m| matches!(m, Ty::PlatformObject(n) if n.as_str() == "КолонкаТаблицыЗначений")
                ));
            }
            other => panic!("expected single Ty::Union param, got {other:?}"),
        }
    }

    #[test]
    fn resolve_platform_type_name_arbitrary_collapses_to_unknown() {
        // BSL's "any value" placeholder must lower to `Ty::Unknown` so the
        // gradual-typing rule (`Unknown ≤ A`) accepts the value in any
        // typed slot. Without this, `Константы.X.Получить()` (declared
        // `return_type: "Произвольный"` in platform_data.json) would lift
        // to `Ty::PlatformObject("Произвольный")` and false-fire
        // `TypeMismatch` on every `ПустаяСтрока(...)` / `СтрДлина(...)` /
        // assignment-to-typed-var sink.
        assert_eq!(resolve_platform_type_name("Произвольный"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("произвольный"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("ПРОИЗВОЛЬНЫЙ"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("Arbitrary"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("arbitrary"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("ARBITRARY"), Ty::Unknown);
        assert_eq!(resolve_platform_type_name("  Произвольный  "), Ty::Unknown);
    }

    #[test]
    fn to_method_info_arbitrary_return_lowers_to_unknown() {
        // End-to-end pin: a platform method declared with
        // `return_type: "Произвольный"` (the shape used by
        // `ConstantManager.<Name>.Get` in platform_data.json) must
        // surface as `Ty::Unknown` through `to_method_info`, so that
        // call-site argument checks against it stay quiet under the
        // gradual rule.
        let info = to_method_info(&test_method(Some("Произвольный"), None));
        assert_eq!(info.return_ty, Ty::Unknown);
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

    fn ts_receiver(parent: MdoType, name: &str) -> Ty {
        Ty::MetadataRef { kind: MetadataKind::TabularSection { parent }, name: Name::new(name) }
    }

    #[test]
    fn method_lookup_tabular_section_add_returns_row_metadata_ref() {
        // `Добавить()` rebinds the generic `Строка табличной части`
        // return to a `TabularSectionRow` receiver pinned to the
        // section's parent MDO and qualified name.
        let r = ts_receiver(MdoType::Catalog, "Номенклатура.Услуги");
        let info = lookup_method(&r, &Name::new("Добавить")).expect(
            "TabularSection.Добавить must resolve through PlatformData[\"Tabular section\"]",
        );
        assert_eq!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                name: Name::new("Номенклатура.Услуги"),
            }
        );
    }

    #[test]
    fn method_lookup_tabular_section_count_returns_number() {
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup_method(&r, &Name::new("Количество"))
            .expect("TabularSection.Количество must resolve");
        assert_eq!(info.return_ty, Ty::Number);
    }

    #[test]
    fn method_lookup_tabular_section_unload_returns_value_table() {
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup_method(&r, &Name::new("Выгрузить"))
            .expect("TabularSection.Выгрузить must resolve");
        assert_eq!(info.return_ty, Ty::ValueTable);
    }

    #[test]
    fn method_lookup_tabular_section_find_returns_union_with_row() {
        // Платформенный return `"Строка табличной части, Неопределено"`
        // → `Ty::Union([TabularSectionRow, Undefined])`. Pin the
        // member ordering / membership rather than equality so
        // future Ty::union flattening tweaks don't break the test.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info =
            lookup_method(&r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
        let members = match info.return_ty {
            Ty::Union(ref m) => m.clone(),
            other => panic!("expected Ty::Union, got {other:?}"),
        };
        assert!(
            members.iter().any(|m| matches!(
                m,
                Ty::MetadataRef {
                    kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                    name,
                } if name.as_str() == "X.Y"
            )),
            "Найти union must include TabularSectionRow {{ parent: Catalog, name: \"X.Y\" }}, got {members:?}",
        );
        assert!(
            members.iter().any(|m| matches!(m, Ty::Undefined)),
            "Найти union must include Ty::Undefined, got {members:?}",
        );
    }

    #[test]
    fn method_lookup_tabular_section_findrows_returns_array() {
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup_method(&r, &Name::new("НайтиСтроки"))
            .expect("TabularSection.НайтиСтроки must resolve");
        assert_eq!(info.return_ty, Ty::Array);
    }

    #[test]
    fn method_lookup_tabular_section_english_name_resolves() {
        // The bilingual platform index keys `Tabular section` ↔
        // `Табличная часть` and the method `Добавить` ↔ `Add`. Asking
        // by the English method name on the Russian-conventional
        // English type key still resolves through the same row rebind.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup_method(&r, &Name::new("Add"))
            .expect("TabularSection.Add must resolve via bilingual platform index");
        assert!(matches!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                ..
            },
        ));
    }

    #[test]
    fn method_lookup_tabular_section_unknown_method_returns_none() {
        // A miss must return `None` so `UnresolvedMethodCall` can
        // surface — ТЧ no longer silently swallows typos.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        assert!(lookup_method(&r, &Name::new("НетТакогоМетодаНаТЧ")).is_none());
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_document() {
        let r = ts_receiver(MdoType::Document, "ПКО.Товары");
        let info = lookup_method(&r, &Name::new("Добавить"))
            .expect("Document TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
                name: Name::new("ПКО.Товары"),
            }
        );
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_exchange_plan() {
        let r = ts_receiver(MdoType::ExchangePlan, "ПО.Состав");
        let info = lookup_method(&r, &Name::new("Добавить"))
            .expect("ExchangePlan TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::ExchangePlan },
                name: Name::new("ПО.Состав"),
            }
        );
    }

    #[test]
    fn method_lookup_tabular_section_find_params_preserve_arbitrary_as_unknown() {
        // `Найти(Произвольный, Строка)`: the first parameter is
        // declared `"Произвольный"` (BSL's "any value" placeholder).
        // It must stay `Ty::Unknown` so subtype checks accept any
        // argument — narrowing it to `Ty::PlatformObject("Произвольный")`
        // would reject every real call site.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info =
            lookup_method(&r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
        assert_eq!(
            info.params,
            vec![Ty::Unknown, Ty::String],
            "Произвольный must stay Ty::Unknown; only the row generic is rebound",
        );
    }

    #[test]
    fn method_lookup_tabular_section_index_param_stays_unknown() {
        // `Индекс(СтрокаТЧ: Строка табличной части)` — parameter
        // intentionally lowers to `Ty::Unknown` (mirrors how every
        // other unrecognised platform-name param behaves through
        // `Ty::from_type_name`). Rebinding the row generic here
        // would narrow it to a section-specific row receiver, but
        // `is_assignable` uses structural equality on `MetadataRef`,
        // so legitimate cross-section transfers
        // (`ТЧ1.Индекс(ТЧ2.Получить(0))`) and possibly-Undefined
        // `Ty::Union` results (`ТЧ.Индекс(ТЧ.Найти(…))`) would be
        // falsely rejected. Gradual typing (`Unknown ≤ A`) keeps the
        // diagnostic quiet.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info =
            lookup_method(&r, &Name::new("Индекс")).expect("TabularSection.Индекс must resolve");
        assert_eq!(
            info.params,
            vec![Ty::Unknown],
            "Индекс param must stay Ty::Unknown — rebinding would false-reject valid args",
        );
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_chart_of_accounts() {
        let r = ts_receiver(MdoType::ChartOfAccounts, "Основной.ВидыСубконто");
        let info = lookup_method(&r, &Name::new("Добавить"))
            .expect("ChartOfAccounts TabularSection.Добавить must resolve");
        assert_eq!(
            info.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::ChartOfAccounts },
                name: Name::new("Основной.ВидыСубконто"),
            }
        );
    }
}
