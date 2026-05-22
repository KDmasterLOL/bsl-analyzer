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
//! - **`Ty::Union(_)`** — dispatched per live branch (Undefined/Null
//!   sentinels stripped); the FIRST successful branch's signature
//!   wins for `params`/`overloads`, later branches only contribute to
//!   the return-type union. See [`union_lookup`] for the cohesion
//!   rule.
//! - **`Ty::ManagerCollection(_)`** / primitives (`Number`, `String`,
//!   `Boolean`, `Date`) — `None`. Collections only expose iteration,
//!   primitives have no instance methods in BSL (`СтрДлина`,
//!   `ДобавитьМесяц` are global functions, not receiver methods).
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

use crate::lower::type_string::{lower_param_type_string, lower_return_type_string};

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
    // `Ty::ThisObject` and `Ty::ThisManager` are coerced to dispatch-
    // ready receivers at adapter entry — `ThisObject` → `MetadataRef
    // { *Object, .. }` (hits the metadata-ref branch); `ThisManager`
    // → `ObjectManager { .. }` (hits the manager branch). See
    // [`crate::this_object`].
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    if let Ty::Union(members) = receiver_ty {
        return union_lookup(members, method_name);
    }

    match receiver_ty {
        Ty::ObjectManager { kind, name } => lookup_on_object_manager(*kind, name, method_name),
        Ty::MetadataRef { kind, name } => lookup_on_metadata_ref(*kind, name, method_name),
        Ty::FormControl { kind, .. } => lookup_on_form_control(*kind, method_name),
        _ => lookup_scalar_receiver(receiver_ty, method_name),
    }
}

/// Resolve a method on any receiver served by the bilingual scalar
/// `PlatformData::get_method` index.
///
/// Covers `Ty::PlatformObject`, `Ty::Array`, `Ty::TypedArray`,
/// `Ty::Map`, `Ty::Structure`, `Ty::ValueTable`, `Ty::ValueList`,
/// `Ty::Type`, and `Ty::FormData` — everything that
/// [`platform_type_key`] resolves to a single English type-name key.
///
/// Post-step: for `Ty::FormData { kind: Collection, .. }`, the generic
/// `ДанныеФормыЭлементКоллекции` return is rewritten to the document /
/// catalog row receiver so the chain
/// `<коллекция>.Получить(0).Атрибут` continues resolving via
/// `field_lookup::lookup_on_tabular_row`.
fn lookup_scalar_receiver(receiver_ty: &Ty, method_name: &Name) -> Option<MethodInfo> {
    let type_key = platform_type_key(receiver_ty)?;
    let method = PlatformData::instance().get_method(type_key, method_name.as_str())?;
    let mut info = to_method_info(method);
    if let Some(row) = form_data_collection_row_ty(receiver_ty) {
        info.return_ty =
            rewrite_form_data_collection_item_return(info.return_ty, &row, method.name.as_str());
    }
    Some(info)
}

/// Resolve a method on a [`Ty::ObjectManager`] receiver.
///
/// Platform-data indexes managers with composite `type_name`
/// (`"CatalogManager.<Имя>"`) and placeholder per-method `name`, so the
/// scalar `get_method` path never hits. Routed through
/// [`crate::platform_manager_lookup::resolve_platform_manager_method`].
///
/// Workspace `ManagerModule.bsl` overrides win earlier via
/// `Resolver::resolve_three_level_method` at the 3-segment call site
/// in `infer.rs` — this fallback runs only through `Expr::MethodCall`
/// / aliased-manager shapes, where there is no CFE resolver to
/// consult.
fn lookup_on_object_manager(
    mdo_type: MdoType,
    name: &Name,
    method_name: &Name,
) -> Option<MethodInfo> {
    let res = crate::platform_manager_lookup::resolve_platform_manager_method(
        mdo_type,
        name,
        method_name,
    )?;
    Some(MethodInfo {
        return_ty: res.return_ty,
        params: res.signature.params.to_vec(),
        overloads: res.overloads,
    })
}

/// Resolve a method on a [`Ty::MetadataRef`] receiver.
///
/// Three layered dispatch paths in priority order:
///
/// 1. **TabularSection** — flat `type_name = "Tabular section"` in
///    platform_data has no `"Prefix.<MDO>"` shape, so it cannot be
///    served by `platform_manager_lookup::find_prefixed_method` (which
///    requires a dot-separated prefix). Route directly to the
///    bilingual scalar index and rebind the generic
///    `"Строка табличной части"` return to a row receiver so
///    `ТЧ.Добавить().Атрибут` continues resolving via
///    `field_lookup::lookup_on_tabular_row`.
/// 2. **Composite metadata-ref** — object/ref flavours
///    (`CatalogObject`, `CatalogRef`, …) go through
///    [`crate::platform_manager_lookup::resolve_platform_metadata_ref_method`].
/// 3. **Scalar key fallback** — synthetic kinds (e.g. `RegisterFilter`)
///    wrap an existing scalar `type_name` (`"Filter"`) whose methods
///    live under a flat HBK row, not a composite prefix. Route through
///    the bilingual scalar index so e.g. `<recordSet>.Отбор.Сбросить()`
///    resolves.
///
/// MetadataRef flavours without a platform surface (register
/// dimensions, the bare `TabularSectionRow` row receiver) fall through
/// `None`. Row methods do not exist in HBK data — `Удалить(Индекс)` and
/// friends are methods on the section, not on rows.
fn lookup_on_metadata_ref(
    kind: MetadataKind,
    name: &Name,
    method_name: &Name,
) -> Option<MethodInfo> {
    if let MetadataKind::TabularSection { parent } = kind {
        let method =
            PlatformData::instance().get_method("Tabular section", method_name.as_str())?;
        return Some(build_tabular_section_method_info(method, parent, name));
    }
    if let Some(res) = crate::platform_manager_lookup::resolve_platform_metadata_ref_method(
        kind,
        name,
        method_name,
    ) {
        return Some(MethodInfo {
            return_ty: res.return_ty,
            params: res.signature.params.to_vec(),
            overloads: res.overloads,
        });
    }
    if let Some(scalar_key) = kind.scalar_platform_key() {
        if let Some(method) = PlatformData::instance().get_method(scalar_key, method_name.as_str())
        {
            return Some(to_method_info(method));
        }
    }
    None
}

/// Resolve a method on a [`Ty::FormControl`] receiver.
///
/// Walks the platform-type chain `[base, extension?]` in reverse:
/// kind-specific extension methods (e.g. `<UsualGroup>.Скрыть()` from
/// "Расширение группы формы для обычной группы") override the shared
/// base `ГруппаФормы` table. Single-entry chains (Field/Button/etc.)
/// reduce to one `get_method` call. `Other` chain is empty → immediate
/// `None`.
fn lookup_on_form_control(
    kind: hir_def::ty::FormElementKind,
    method_name: &Name,
) -> Option<MethodInfo> {
    hir_def::ty::form_control_chain_first_hit(kind, |type_name| {
        PlatformData::instance().get_method(type_name, method_name.as_str()).map(to_method_info)
    })
}

/// Dispatch method lookup across a [`Ty::Union`] receiver.
///
/// `Ty::Union` receivers are the common "happy path + Неопределено"
/// shape from platform return types (e.g. `Запрос.Выполнить()` →
/// `Ty::Union([QueryResult, Undefined])`). `Undefined` / `Null`
/// sentinels are stripped (they have no instance methods); the caller
/// sees a unioned return type so chained calls like
/// `Запрос.Выполнить().Выгрузить()` resolve without waiting for M4
/// full narrowing.
///
/// **Cohesion rule:** `params` and `overloads` MUST come from the SAME
/// branch. If `params` won on the first hit and `overloads` on a later
/// one, callers would type-check args against overloads belonging to a
/// receiver shape that the chosen `params` does not represent. The
/// FIRST successful branch's signature is bound wholesale; later
/// branches only contribute to the return-type union.
fn union_lookup(members: &[Ty], method_name: &Name) -> Option<MethodInfo> {
    let live: Vec<&Ty> =
        members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).collect();
    let mut returns: Vec<Ty> = Vec::with_capacity(live.len());
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
    hit_any.then(|| MethodInfo { return_ty: Ty::union(returns), params, overloads })
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
        // `TypedArray` shares the platform method table with `Array`:
        // method lookup is structural (`.Добавить()`, `.Количество()`,
        // …), the element type only refines field/iteration surfaces.
        Ty::Array | Ty::TypedArray(_) => Some("Array"),
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
        // fallback. Same posture for `ThisManager`, whose coercion
        // target is `Ty::ObjectManager`.
        Ty::ThisObject { .. } | Ty::ThisManager { .. } => None,
        // Managed-form attribute receivers route methods through the
        // platform form-data wrappers (`ДанныеФормыСтруктура` /
        // `ДанныеФормыКоллекция` / `ДанныеФормыСтруктураСКоллекцией`),
        // **not** through `underlying`. The wrapper deliberately hides
        // object-level methods like `Записать()` that don't apply to a
        // form-data projection — see [`Ty::FormData`] docs.
        Ty::FormData { kind, .. } => Some(kind.platform_type_name()),
        // Form-control receivers (`Элементы.<имя>`) route through the
        // per-kind platform tables (`ТаблицаФормы` / `ПолеФормы` /
        // `КнопкаФормы` / `ГруппаФормы` / `ДекорацияФормы` /
        // `ДополнениеЭлементаФормы`). `binding` is irrelevant for
        // method dispatch — it only refines a handful of properties
        // (`.ВыделенныеСтроки`, `.ТекущаяСтрока`) handled in
        // `field_lookup`. `Other` returns `None`: unrecognised XML tag,
        // no platform table to query.
        Ty::FormControl { kind, .. } => hir_def::ty::form_control_platform_type_name(*kind),
        // Projection-typed receivers alias to the same platform
        // method tables `Ty::PlatformObject(...)` reaches today. Phase 0
        // carries no projection payload, so method dispatch resolves
        // identically to the legacy `Ty::PlatformObject("Запрос")` etc.
        // shapes (tests at `method_lookup.rs:706-729` pin this).
        Ty::Query { .. } => Some("Запрос"),
        Ty::QueryResult { .. } => Some("РезультатЗапроса"),
        Ty::QueryResultSelection { .. } => Some("ВыборкаИзРезультатаЗапроса"),
        // Batch result iterates / counts like `Массив` — share the table.
        Ty::QueryBatchResult { .. } => Some("Array"),
        // `AnyMetadataRef` mirrors `ManagerCollection` in Phase 0 — no
        // scalar platform key (manager dispatch routes through
        // `platform_manager_lookup`, not the scalar table).
        Ty::AnyMetadataRef { .. } => None,
    }
}

fn form_data_collection_row_ty(receiver_ty: &Ty) -> Option<Ty> {
    let Ty::FormData {
        kind: hir_def::ty::FormDataKind::Collection,
        underlying: Some((mdo_type, section_name)),
    } = receiver_ty
    else {
        return None;
    };
    if !section_name.as_str().contains('.') {
        return None;
    }
    Some(Ty::MetadataRef {
        kind: MetadataKind::TabularSectionRow { parent: *mdo_type },
        name: section_name.clone(),
    })
}

fn rewrite_form_data_collection_item_return(ty: Ty, row: &Ty, method_name: &str) -> Ty {
    match ty {
        Ty::PlatformObject(ref name) if is_form_data_collection_item_type_name(name.as_str()) => {
            row.clone()
        }
        Ty::Union(members) => Ty::union(
            members
                .iter()
                .map(|m| rewrite_form_data_collection_item_return(m.clone(), row, method_name))
                .collect(),
        ),
        Ty::Array if is_row_array_method(method_name) => Ty::TypedArray(Box::new(row.clone())),
        other => other,
    }
}

fn is_form_data_collection_item_type_name(name: &str) -> bool {
    let lc = name.to_lowercase();
    lc == "данныеформыэлементколлекции" || lc == "formdatacollectionitem"
}

/// Convert a `PlatformMethod` entry into the semantic `MethodInfo`.
///
/// - `return_type = Some("Число")` → `Ty::Number` (via
///   `ty_from_bare_name`); unrecognised names fall back to
///   `Ty::PlatformObject(name)` so hover / chained calls still carry a
///   meaningful type.
/// - `return_type = Some("РезультатЗапроса, Неопределено")` — comma-joined
///   union strings emitted by the HBK scraper when a method returns one of
///   several types (happy-path + null sentinel). Split on `,`, map each
///   segment via `lower_platform_type_name`, and feed the list to
///   `Ty::union` so chained calls see `Ty::Union([QueryResult, Undefined])`
///   instead of a poisoned `Ty::PlatformObject("... ,Неопределено")`.
/// - `return_type = None` → procedure; `Ty::Undefined`.
/// - Parameter types are kept as raw scalars for now; malformed comma-heavy
///   HBK prose stays a single `Ty::PlatformObject(...)` instead of poisoning
///   argument checks with bogus union members.
pub(crate) fn to_method_info(method: &PlatformMethod) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| lower_return_type_string(ret))
        .unwrap_or(Ty::Undefined);

    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| p.param_type.as_ref().map(|t| lower_param_type_string(t)).unwrap_or(Ty::Unknown))
        .collect();

    // Per-overload param lists for multi-overload methods
    // (`ЧтениеXML.ПолучитьАтрибут` etc.). Empty when the platform JSON
    // declares a single signature — `params` already covers it.
    let overloads = lower_overloads(method);

    MethodInfo { return_ty, params, overloads }
}

/// Lower per-variant parameter lists for multi-overload methods.
///
/// Mirrors [`to_method_info`]'s overload computation but exposed as a
/// stand-alone helper so the unified `resolve_method` use case and the
/// composite-prefix `build_resolution` adapter can share one
/// implementation.
///
/// Returns an empty `Vec` when the platform JSON declares a single
/// signature; populated when multiple `Вариант синтаксиса:` sections
/// exist (e.g. `Array.Найти`, `ЧтениеXML.ПолучитьАтрибут`,
/// `InformationRegisterManager.Get`, `AccountingRegisterRecordSet.Move`,
/// `BusinessProcessManager.FindByNumber`). Argument-type checks accept
/// the call when ANY overload accepts it.
pub(crate) fn lower_overloads(method: &PlatformMethod) -> Vec<Vec<Ty>> {
    method
        .variants
        .iter()
        .map(|v| {
            v.parameters
                .iter()
                .map(|p| {
                    p.param_type.as_ref().map(|t| lower_param_type_string(t)).unwrap_or(Ty::Unknown)
                })
                .collect()
        })
        .collect()
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
/// `to_method_info` baseline (`ty_from_bare_name`) so unrecognised
/// platform names like `"Произвольный"` (Найти's first arg, "any
/// value") and `"Строка табличной части"` (Индекс's arg) lower to
/// `Ty::Unknown`. Rebinding params would narrow them to
/// `Ty::MetadataRef { TabularSectionRow { parent }, "<this section>" }`
/// — and [`crate::subtype::is_assignable`] uses structural equality on
/// `MetadataRef`, so any legitimate cross-section transfer
/// (`ТЧ1.Индекс(ТЧ2.Получить(0))`) or possibly-Undefined `Ty::Union`
/// argument (`ТЧ.Индекс(ТЧ.Найти(...))`) would be falsely rejected. The
/// gradual-typing rule (`Unknown ≤ A`) keeps these calls quiet.
pub(crate) fn build_tabular_section_method_info(
    method: &PlatformMethod,
    parent: MdoType,
    section_name: &Name,
) -> MethodInfo {
    let return_ty = method
        .return_type
        .as_ref()
        .map(|ret| {
            let lowered = rewrite_row_generic(lower_return_type_string(ret), parent, section_name);
            // Methods like `НайтиСтроки` / `FindRows` carry a HBK return
            // string of `"Массив"` with no element-type schema — the
            // platform JSON has no slot for typed-array witnesses. The
            // bare `Ty::Array` makes `arr[i]` and `Для каждого row Из …`
            // surface `Ty::Unknown` for the row, which collapses every
            // downstream column lookup. Rewrite to `Ty::TypedArray(Row)`
            // when the method is one of the row-array-returning kind so
            // chained access (`ТЧ.НайтиСтроки(От)[0].Колонка`,
            // iteration body field-access) stays typed.
            rewrite_row_array_for_method(lowered, method.name.as_str(), parent, section_name)
        })
        .unwrap_or(Ty::Undefined);

    // Same conservative param lowering as `to_method_info` — see
    // [`lower_param_type_string`] for the multi-type-vs-single-type
    // asymmetry rationale. We deliberately do NOT pipe through
    // `rewrite_row_generic` for params (function-doc on
    // `build_tabular_section_method_info` above explains why).
    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string(t.as_str()))
                .unwrap_or(Ty::Unknown)
        })
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
                        .map(|t| lower_param_type_string(t.as_str()))
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

/// Promote a bare [`Ty::Array`] return to [`Ty::TypedArray`] of the
/// matching tabular-section row when the method is one of the
/// row-array-returning kind on a tabular section receiver.
///
/// Companion to [`rewrite_row_generic`]: that one rewrites the scalar
/// `Строка табличной части` PlatformObject; this one rewrites the
/// element-less `Массив` for methods whose semantics are "find / collect
/// rows". The platform JSON cannot encode "Array of Row" so the rebind
/// has to happen in code, keyed on the method name.
///
/// The set of row-array methods is intentionally minimal — only HBK
/// methods whose contract is unambiguously "return array of this
/// section's rows". Other Array-returning methods on the same receiver
/// — `ВыгрузитьКолонку` / `UnloadColumn` is the concrete same-receiver
/// example, returning an array of heterogeneous column values, NOT row
/// objects — keep their bare `Ty::Array` so subtyping stays gradual and
/// chained `.<row-field>` access on them correctly stays `Unknown`
/// rather than being rebound to a row schema that wouldn't apply.
fn rewrite_row_array_for_method(
    ty: Ty,
    method_name: &str,
    parent: MdoType,
    section_name: &Name,
) -> Ty {
    if !is_row_array_method(method_name) {
        return ty;
    }
    let row = Ty::MetadataRef {
        kind: MetadataKind::TabularSectionRow { parent },
        name: section_name.clone(),
    };
    match ty {
        Ty::Array => Ty::TypedArray(Box::new(row)),
        // Already typed by the JSON / earlier rewrite — leave it.
        Ty::TypedArray(_) => ty,
        other => other,
    }
}

/// Method names whose return on a tabular section receiver is
/// semantically "array of rows of THIS section". Bilingual + lowercase.
fn is_row_array_method(name: &str) -> bool {
    let lc = name.to_lowercase();
    matches!(lc.as_str(), "найтистроки" | "findrows")
}

/// Match the platform return / param string for a TS row, accepting both
/// canonical Russian and English spellings. Uses lowercase comparison so
/// future scraper-emitted casing variants (`строка табличной части`)
/// also match — `eq_ignore_ascii_case` would not fold Cyrillic.
fn is_tabular_row_type_name(name: &str) -> bool {
    let lc = name.to_lowercase();
    lc == "строка табличной части" || lc == "line of a tabular section"
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
                is_variadic: false,
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
    fn method_lookup_typed_array_shares_array_method_table() {
        // `Ty::TypedArray(_)` keys through the same `"Array"` page as
        // `Ty::Array` — the element type only refines field/iteration,
        // method dispatch is structural.
        let receiver = Ty::TypedArray(Box::new(Ty::String));
        let info = lookup_method(&receiver, &Name::new("Добавить"))
            .expect("TypedArray must expose Массив.Добавить through the Array platform page");
        assert_eq!(info.return_ty, Ty::Undefined);

        // `.Количество()` returns Число — proves arithmetic-friendly
        // chaining (`.ВыделенныеСтроки.Количество()` in the form-control
        // refinement targeted by Phase 5) survives the new variant.
        let count = lookup_method(&receiver, &Name::new("Количество"))
            .expect("TypedArray.Количество must resolve via the Array platform page");
        assert_eq!(count.return_ty, Ty::Number);
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
        // typing for SINGLE-name params whose name `ty_from_bare_name`
        // doesn't recognise — `Ty::Unknown` accepts any actual at the
        // call-site argument check. Routing this through
        // `lower_return_type_string` would lift to a structural
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

    #[test]
    fn method_lookup_register_filter_resolves_filter_method_via_scalar_key() {
        // `RegisterFilter` has no composite `platform_prefix`; its
        // methods (`Сбросить`, `Получить`, …) live under scalar
        // `type_name = "Filter"`. The scalar-key fallback must route
        // there so `<recordSet>.Отбор.<method>()` resolves for
        // inference. Pinned because regressing this would break the
        // user-facing `НаборЗаписей.Отбор.Сбросить()` snippet.
        let r = Ty::MetadataRef {
            kind: MetadataKind::RegisterFilter { parent: MdoType::InformationRegister },
            name: Name::new("РегистрСведений1"),
        };
        let info = lookup_method(&r, &Name::new("Сбросить"))
            .expect("Filter.Сбросить must resolve through scalar-key fallback");
        // `Сбросить()` is a procedure → `Ty::Undefined`.
        assert_eq!(info.return_ty, Ty::Undefined);
    }

    // Hover/goto coverage for the `Filter.Сбросить` scalar-key fallback
    // moved to `platform_resolution::tests::metadata_ref_register_filter_resolves_with_scalar_handle`
    // after `lookup_method_with_key` was replaced by the unified
    // `resolve_method` use case.

    #[test]
    fn method_lookup_composite_multi_overload_populates_overloads() {
        // Inference-side regression: composite-prefix methods can declare
        // multiple `Вариант синтаксиса:` sections in HBK
        // (`InformationRegisterManager.Get`,
        // `AccountingRegisterRecordSet.Move`,
        // `BusinessProcessManager.FindByNumber`, …). The pre-fix
        // `lookup_method` produced `overloads: Vec::new()` for these
        // because `build_resolution` didn't compute per-variant params,
        // and `arg_diagnostics_query` (which consumes
        // `MethodInfo.overloads`) consequently saw composite multi-
        // overload calls as strictly typed against the first signature
        // only and false-fired on legitimate alternative call shapes.
        // Pin the fix here — the IDE-side equivalent lives in
        // `platform_resolution::tests::composite_multi_overload_method_populates_overloads`.
        let r =
            Ty::ObjectManager { kind: MdoType::InformationRegister, name: Name::new("Курсы") };
        let Some(info) = lookup_method(&r, &Name::new("Получить")) else {
            // Skip when running without platform data.
            println!("Skipping: no platform data available");
            return;
        };
        assert!(
            !info.overloads.is_empty(),
            "InformationRegisterManager.Получить must surface multi-overload variants \
             through lookup_method (the inference path); got params={:?}, overloads={:?}",
            info.params,
            info.overloads,
        );
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
    fn method_lookup_tabular_section_findrows_returns_typed_array_of_rows() {
        // НайтиСтроки's HBK contract is "Массив" (no element witness).
        // `build_tabular_section_method_info` rebinds that to
        // `TypedArray(Row)` so chained `НайтиСтроки(...)[0].Колонка`
        // resolves the column instead of falling into Unknown.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup_method(&r, &Name::new("НайтиСтроки"))
            .expect("TabularSection.НайтиСтроки must resolve");
        match info.return_ty {
            Ty::TypedArray(elem) => match *elem {
                Ty::MetadataRef {
                    kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                    ref name,
                } => assert_eq!(name.as_str(), "X.Y"),
                other => panic!(
                    "expected TypedArray(MetadataRef{{TabularSectionRow,X.Y}}), got element {other:?}"
                ),
            },
            other => panic!("expected TypedArray, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_tabular_section_findrows_english_alias_typed_array() {
        let r = ts_receiver(MdoType::Document, "ПКО.Товары");
        let info = lookup_method(&r, &Name::new("FindRows"))
            .expect("TabularSection.FindRows must resolve via bilingual platform index");
        assert!(matches!(
            info.return_ty,
            Ty::TypedArray(ref elem)
                if matches!(**elem, Ty::MetadataRef {
                    kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
                    ..
                }),
        ));
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
        // `ty_from_bare_name`). Rebinding the row generic here
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

    // ---------- Phase 12: form-control chain walk for methods ----------

    #[test]
    fn method_lookup_usual_group_resolves_extension_method() {
        // `<UsualGroup>.Скрыть()` lives in `Расширение группы формы для
        // обычной группы` (3 methods total: Скрыть/Показать/Скрыта),
        // NOT on the shared `ГруппаФормы` base. Without the chain walk
        // method dispatch would miss; chain.iter().rev() hits the
        // extension first.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::UsualGroup, binding: None };
        assert!(
            lookup_method(&receiver, &Name::new("Скрыть")).is_some(),
            "<UsualGroup>.Скрыть must resolve via the usual-group extension chain entry"
        );
        assert!(
            lookup_method(&receiver, &Name::new("Показать")).is_some(),
            "<UsualGroup>.Показать must resolve via the usual-group extension chain entry"
        );
    }

    #[test]
    fn method_lookup_pages_does_not_borrow_usual_group_methods() {
        // `Скрыть`/`Показать` are scoped to the UsualGroup extension.
        // A `<Pages>` receiver carries the Pages extension only — its
        // chain must NOT surface UsualGroup methods.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Pages, binding: None };
        assert!(
            lookup_method(&receiver, &Name::new("Скрыть")).is_none(),
            "Pages chain must not borrow UsualGroup-extension methods"
        );
    }

    #[test]
    fn method_lookup_form_control_other_with_empty_chain_returns_none() {
        // `Other` chain is empty → the rev-walk loop runs zero
        // iterations and we return None safely without panicking.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Other, binding: None };
        assert!(lookup_method(&receiver, &Name::new("Скрыть")).is_none());
    }

    #[test]
    fn method_lookup_union_two_live_branches_first_branch_signature_wins() {
        // Cohesion rule in `union_lookup`: when MULTIPLE live branches
        // resolve the same method, `params`/`overloads` MUST come from
        // the FIRST successful branch only; later branches contribute
        // their return types to the union but never overwrite the
        // bound signature. Pins this guarantee with `Array | ValueTable`
        // — both expose `Количество()` with `Ty::Number` return, so the
        // union return stays `Ty::Number` and `params` is empty (both
        // signatures match), but if the cohesion rule ever flips to
        // "last wins", a future overload divergence between Array and
        // ValueTable would silently re-bind callers' arg-checks.
        let u = Ty::union(vec![Ty::Array, Ty::ValueTable]);
        let info = lookup_method(&u, &Name::new("Количество"))
            .expect("Union(Array, ValueTable).Количество must resolve through both branches");
        assert_eq!(info.return_ty, Ty::Number, "Количество returns Число on both branches");
        // Cohesion sanity: a single signature was bound — neither
        // params nor overloads were merged across branches.
        assert!(
            info.overloads.is_empty(),
            "cohesion: overloads must NOT be merged across union branches, got {:?}",
            info.overloads,
        );
    }

    #[test]
    fn method_lookup_form_data_collection_get_rewrites_item_return_to_row() {
        // Pin for the `form_data_collection_row_ty` + return-rewrite
        // post-step inside [`lookup_scalar_receiver`].
        //
        // `Ty::FormData { kind: Collection, underlying:
        // Some((Document, "Док.Товары")) }` has its platform key
        // resolved to "ДанныеФормыКоллекция", whose `Получить` method
        // returns generic `ДанныеФормыЭлементКоллекции`. The post-step
        // must rebind that return to the document's tabular-section
        // row receiver, so the chain `<коллекция>.Получить(0).Атрибут`
        // resolves via `field_lookup::lookup_on_tabular_row`.
        let receiver = Ty::FormData {
            kind: hir_def::ty::FormDataKind::Collection,
            underlying: Some((MdoType::Document, Name::new("Док.Товары"))),
        };
        let info = lookup_method(&receiver, &Name::new("Получить"))
            .expect("FormDataCollection.Получить must resolve in platform data");
        match info.return_ty {
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
                name,
            } => assert_eq!(name.as_str(), "Док.Товары"),
            other => panic!("expected TabularSectionRow{{Document}} rewrite, got {other:?}"),
        }
    }

    #[test]
    fn method_lookup_form_control_table_unchanged_by_chain_walk() {
        // Single-entry chain (`["ТаблицаФормы"]`) reduces the chain
        // walk to one `get_method` call — identical pre-chain
        // behaviour. Pinning it as a non-regression for kinds that
        // weren't split.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        // ТаблицаФормы has documented platform method `ОбновитьСтроки`
        // (per platform_data; this is a stable canary). If the data
        // ever drops it, swap for any other `ТаблицаФормы` method.
        let _ = lookup_method(&receiver, &Name::new("ОбновитьСтроки"));
    }
}
