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

use std::sync::Arc;

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformMethod};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::ty::{MetadataKind, SdblProjection, Ty};
use hir_def::{DefWithBodyId, ExprId, Name};
use vfs::FileId;

use crate::db::HirDatabase;
use crate::lower::type_string::{lower_param_type_string, lower_return_type_string};
use crate::ty_bridge::ty_to_typeid;

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
/// Result of a successful method lookup — kernel-native surface.
///
/// Phase 3 §4.E.2a: the public `MethodInfo` now carries interned
/// [`TypeId`]s. Internal resolution still runs on legacy [`Ty`] via the
/// private [`MethodInfoTy`] (see below); the public entry points bridge
/// at the boundary. §4.E.2b flips the internal dispatch + SDBL machinery
/// to kernel-native and removes the entry/exit bridges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodInfo {
    /// Return type id. `Undefined` for procedures (platform methods with
    /// no declared return type).
    pub return_ty: TypeId,
    /// Parameter type ids, in declaration order — flat union across
    /// overloads. Used by hover / completion / single-signature
    /// fallbacks.
    pub params: Vec<TypeId>,
    /// Per-overload parameter id lists. Empty for single-overload methods.
    pub overloads: Vec<Vec<TypeId>>,
}

/// Internal legacy-`Ty` resolution result.
///
/// Phase 3 §4.E.2a: the resolution pipeline (`lookup_*`, `union_lookup`,
/// `apply_sdbl_chain_rewrite`, …) continues to operate on `Ty`; this is
/// the struct it threads. The public [`MethodInfo`] is produced from it
/// by [`method_info_ty_to_kernel`] at the boundary. §4.E.2b will delete
/// this and run the pipeline directly on `TypeId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MethodInfoTy {
    pub(crate) return_ty: Ty,
    pub(crate) params: Vec<Ty>,
    pub(crate) overloads: Vec<Vec<Ty>>,
}

/// Bridge an internal [`MethodInfoTy`] to the public kernel-native
/// [`MethodInfo`].
fn method_info_ty_to_kernel(db: &dyn TypeKernelDb, info: MethodInfoTy) -> MethodInfo {
    MethodInfo {
        return_ty: ty_to_typeid(db, &info.return_ty),
        params: info.params.iter().map(|t| ty_to_typeid(db, t)).collect(),
        overloads: info
            .overloads
            .iter()
            .map(|row| row.iter().map(|t| ty_to_typeid(db, t)).collect())
            .collect(),
    }
}

/// Resolve a method call on a typed receiver (refinement-free entry).
///
/// Returns `None` when:
/// - the receiver type carries no platform method table (e.g.
///   `Ty::Unknown`, `Ty::Number`-style primitives that are not platform
///   objects, unions, manager collectives);
/// - the method name does not exist in the resolved table.
///
/// Equivalent to [`lookup_method_with_refinement`] with `refine_ctx =
/// None`: callers without inference context (tests, hir-level facade
/// queries, hover synthesis) reach the same platform tables but skip
/// the variable-state refinement Phase D adds for SDBL chains.
pub fn lookup_method(
    db: &dyn TypeKernelDb,
    receiver_ty: &Ty,
    method_name: &Name,
) -> Option<MethodInfo> {
    lookup_method_with_refinement(db, receiver_ty, method_name, None)
}

/// Resolve a method call on a typed receiver, optionally upgrading
/// SDBL-chain receivers via reaching-defs refinement (Phase D).
///
/// `refine_ctx` carries the inference-side handles
/// ([`HirDatabase`], file/owner, dispatch + receiver `ExprId`s, body)
/// needed by [`crate::query_text_dataflow::refine_query_at_dispatch`]
/// to walk reaching `<var>.Текст = "..."` writes and recover a
/// projection that earlier lowering steps could not see (assignment
/// after `Новый Запрос`, or a constructor with a non-literal arg).
///
/// When `refine_ctx` is `None` (the case for the facade entry
/// [`lookup_method`]), refinement is skipped — the receiver Ty is
/// taken as-is, exactly matching pre-Phase-D behaviour.
pub fn lookup_method_with_refinement(
    db: &dyn TypeKernelDb,
    receiver_ty: &Ty,
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfo> {
    // Phase 3 §4.E.2a: run the (unchanged) `Ty`-native resolution
    // pipeline, then bridge ONLY the result to the kernel-native
    // `MethodInfo`. The receiver stays `&Ty` until §4.E.2b flips the
    // whole dispatch (and the SDBL chain machinery) to `TypeId`. The
    // kernel manager-representation gap that previously blocked the
    // receiver flip is closed (§4.E.2b-i: `TypeKind::ObjectManager`
    // now carries `ManagerFacet { mdo: MdoType, .. }`, lossless).
    let info = lookup_method_with_refinement_ty(receiver_ty, method_name, refine_ctx)?;
    Some(method_info_ty_to_kernel(db, info))
}

/// Internal `Ty`-native resolver — the pre-§4.E pipeline body.
fn lookup_method_with_refinement_ty(
    receiver_ty: &Ty,
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfoTy> {
    // `Ty::ThisObject` and `Ty::ThisManager` are coerced to dispatch-
    // ready receivers at adapter entry — `ThisObject` → `MetadataRef
    // { *Object, .. }` (hits the metadata-ref branch); `ThisManager`
    // → `ObjectManager { .. }` (hits the manager branch). See
    // [`crate::this_object`].
    let coerced = crate::this_object::coerce_to_metadata_ref(receiver_ty);
    let receiver_ty = coerced.as_ref().unwrap_or(receiver_ty);

    if let Ty::Union(members) = receiver_ty {
        return union_lookup(members, method_name, refine_ctx);
    }

    let info = match receiver_ty {
        Ty::ObjectManager { kind, name } => lookup_on_object_manager(*kind, name, method_name),
        Ty::MetadataRef { kind, name } => lookup_on_metadata_ref(*kind, name, method_name),
        Ty::FormControl { kind, .. } => lookup_on_form_control(*kind, method_name),
        _ => lookup_scalar_receiver(receiver_ty, method_name),
    }?;

    // SDBL chain rewrite — `Запрос.Выполнить()`, `.Выбрать()`,
    // `.ВыполнитьПакет()` lift the platform return into the
    // projection-typed `Ty::Query*` variants seeded in Phase 0. The
    // hook runs AFTER scalar lookup (so it never rewrites a method
    // that doesn't exist on the receiver) and is gated by
    // [`is_sdbl_chain_method`] so unrelated method calls pay only a
    // hashset-membership check.
    Some(apply_sdbl_chain_rewrite(receiver_ty, method_name, info, refine_ctx))
}

/// Per-dispatch context for [`lookup_method_with_refinement`].
///
/// Held by value at the call site; the borrow lifetime `'a` is the
/// same that callers already use to thread `&self.body` /
/// `&dyn HirDatabase`. The struct is `pub` so consumers in `hir-ty`
/// and `hir` can build it from their own inference / facade state,
/// but its only consumer is the SDBL chain rewrite — non-SDBL
/// receivers never inspect it.
#[derive(Clone, Copy)]
pub struct RefineCtx<'a> {
    /// Salsa database handle — used to pull
    /// `module_reaching_definitions` and `sdbl_hir_for_file_query`.
    pub db: &'a dyn HirDatabase,
    /// File the dispatch lives in.
    pub file_id: FileId,
    /// Owning body of the dispatch (`Method(local_id)` or `ModuleCode`).
    pub owner: DefWithBodyId,
    /// HIR body containing both the dispatch expression and any
    /// assignment statements that may reach it.
    pub body: &'a Body,
    /// ExprId of the dispatch site — the `Expr::Field { base, .. }`
    /// or outer `Expr::Call` that lowers to the SDBL chain method.
    /// Either works for [`Body::enclosing_stmt`] lookup since both
    /// share the same enclosing statement.
    pub dispatch_expr_id: ExprId,
    /// ExprId of the receiver — `base` of the `Expr::Field` node.
    /// Phase D only refines when this is `Expr::Path(name)`.
    pub receiver_expr_id: ExprId,
    /// Argument ExprIds of the dispatch site. Phase H's
    /// `.Выгрузить(arg)` narrowing inspects the first element to
    /// decide tree-vs-table; Phase D's text-write refinement
    /// ignores this field. Empty slice is the no-arg case.
    pub call_args: &'a [ExprId],
}

/// Method-name filter for [`apply_sdbl_chain_rewrite`].
///
/// Returns `true` for the bilingual chain entry points (`Выполнить`,
/// `Execute`, `Выбрать`, `Choose`, `ВыполнитьПакет`, `ExecuteBatch`).
/// Comparison is case-insensitive against Russian and English forms;
/// the bilingual platform index treats them as the same method, so the
/// rewrite must too.
fn is_sdbl_chain_method(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "выполнить" | "execute" | "выбрать" | "choose" | "выполнитьпакет" | "executebatch",
    )
}

/// Method-name filter for the Phase H `.Выгрузить()` narrowing.
///
/// `РезультатЗапроса.Выгрузить(ТипОбхода)` returns a static union of
/// `[ТаблицаЗначений, ДеревоЗначений]`; the runtime shape depends on
/// the iteration argument. The narrower only fires when the receiver
/// is a `Ty::QueryResult` (or its legacy `Ty::PlatformObject
/// ("РезультатЗапроса")` shape) — see [`is_query_result_receiver`].
fn is_unload_method(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "выгрузить" | "unload")
}

/// Phase H — drop the wrong arm of the
/// `[ТаблицаЗначений, ДеревоЗначений]` return union when
/// `.Выгрузить(arg)`'s argument is a statically recognisable
/// `ОбходРезультатаЗапроса` member.
///
/// Gated on [`is_query_result_receiver`] so the rewrite never collides
/// with `ТабличнаяЧасть.Выгрузить` / `FormDataCollection.Выгрузить`
/// (those declare a single-typed `ТаблицаЗначений` return — narrowing
/// is a no-op there but would still cost the union walk).
fn narrow_unload_return(
    receiver_ty: &Ty,
    info: MethodInfoTy,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> MethodInfoTy {
    if !is_query_result_receiver(receiver_ty) {
        return info;
    }
    // Slice 1b — extract the receiver's projection so the kept
    // `Ty::ValueTable` arm can carry it through `.Выгрузить()`. None
    // is the legacy `Ty::PlatformObject("РезультатЗапроса")` shape;
    // chain still narrows but the table arm stays projection-less.
    let projection = projection_of_query_result_receiver(receiver_ty).flatten();
    let return_ty = if let Some(ctx) = refine_ctx {
        use crate::query_unload_refinement::{classify_unload_arg, UnloadIteration};
        let decision = classify_unload_arg(ctx.body, ctx.call_args);
        let narrowed = match decision {
            UnloadIteration::Dynamic => info.return_ty,
            UnloadIteration::Linear => drop_union_arm(info.return_ty, is_value_tree_arm),
            UnloadIteration::Hierarchical => drop_union_arm(info.return_ty, is_value_table_arm),
        };
        attach_projection_to_value_table(narrowed, projection)
    } else {
        attach_projection_to_value_table(info.return_ty, projection)
    };
    MethodInfoTy { return_ty, params: info.params, overloads: info.overloads }
}

/// Walk `ty` and replace every `Ty::ValueTable { projection: None }`
/// arm with `Ty::ValueTable { projection }`. Other arms pass through.
///
/// Operates on the bare type and inside `Ty::Union` arms. Receivers
/// that already carry a non-None projection are intentionally left
/// alone — projection refinement is only ever an *upgrade*. None
/// receiver projection short-circuits to identity so the rewrite
/// stays free for legacy code paths.
fn attach_projection_to_value_table(ty: Ty, projection: Option<Arc<SdblProjection>>) -> Ty {
    let Some(projection) = projection else { return ty };
    let upgrade = |arm: &Ty| -> Option<Ty> {
        match arm {
            Ty::ValueTable { projection: None } => {
                Some(Ty::ValueTable { projection: Some(projection.clone()) })
            }
            _ => None,
        }
    };
    if let Some(upgraded) = upgrade(&ty) {
        return upgraded;
    }
    let Ty::Union(members) = ty else { return ty };
    let rebuilt: Vec<Ty> =
        members.iter().map(|arm| upgrade(arm).unwrap_or_else(|| arm.clone())).collect();
    Ty::Union(rebuilt.into())
}

/// `ТаблицаЗначений` shows up as the dedicated [`Ty::ValueTable`]
/// variant (or `{ projection: .. }` once Slice 2 lands); the legacy
/// `Ty::PlatformObject("ТаблицаЗначений")` is also accepted in case
/// platform-string lowering ever produces it.
fn is_value_table_arm(ty: &Ty) -> bool {
    matches!(ty, Ty::ValueTable { .. })
        || matches!(ty, Ty::PlatformObject(n) if is_platform_name(n, "ТаблицаЗначений", "ValueTable"))
}

/// `ДеревоЗначений` has no dedicated `Ty` variant today, so we match
/// the bilingual `Ty::PlatformObject` shape only.
fn is_value_tree_arm(ty: &Ty) -> bool {
    matches!(ty, Ty::PlatformObject(n) if is_platform_name(n, "ДеревоЗначений", "ValueTree"))
}

/// Receiver gate for [`narrow_unload_return`].
fn is_query_result_receiver(ty: &Ty) -> bool {
    match ty {
        Ty::QueryResult { .. } => true,
        Ty::PlatformObject(n) => is_platform_name(n, "РезультатЗапроса", "QueryResult"),
        _ => false,
    }
}

/// Walk a `Ty::Union` and drop arms matching `unwanted`. Non-Union
/// inputs pass through.
///
/// If dropping leaves a single arm, collapse to that arm directly. If
/// dropping would empty the union, return the original (defensive —
/// the platform signature should always have at least the kept arm).
fn drop_union_arm(ty: Ty, unwanted: impl Fn(&Ty) -> bool) -> Ty {
    let Ty::Union(members) = ty else { return ty };
    let kept: Vec<Ty> = members.iter().filter(|t| !unwanted(t)).cloned().collect();
    if kept.is_empty() {
        return Ty::Union(members);
    }
    if kept.len() == 1 {
        return kept.into_iter().next().unwrap();
    }
    Ty::Union(kept.into())
}

/// Rewrite the return type of an SDBL chain method to the matching
/// projection-typed `Ty::Query*` variant.
///
/// Operates on the already-resolved [`MethodInfoTy`] so the receiver-side
/// signature (`params`, `overloads`) is untouched — only the return
/// type changes. The bilingual method-name filter
/// [`is_sdbl_chain_method`] short-circuits unrelated calls before any
/// real work.
///
/// Receiver-shape guards live in [`pick_chain_rewrite`]; nullability is
/// preserved by [`rewrite_platform_arm_in_return`] which walks `Union`
/// arms and replaces only the target `Ty::PlatformObject(name)` arm
/// (leaving `Ty::Undefined` / other arms intact). Example:
///
/// ```text
/// receiver:  Ty::PlatformObject("Запрос")
/// method:    Выполнить
/// platform:  return = Union([PlatformObject("РезультатЗапроса"), Undefined])
/// rewritten: return = Union([Ty::QueryResult{None}, Undefined])
/// ```
///
/// Phase 1.3 Slice 1: projection payload is always `None`. Phase 1.3b
/// adds constructor-arg projection synthesis; Phase D adds variable
/// refinement so the projection survives a `.Текст = "..."` assignment.
fn apply_sdbl_chain_rewrite(
    receiver_ty: &Ty,
    method_name: &Name,
    info: MethodInfoTy,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> MethodInfoTy {
    // Phase H — `QueryResult.Выгрузить(ТипОбхода)` argument-driven
    // narrowing. The platform signature declares the return as
    // `Union([ТаблицаЗначений, ДеревоЗначений])`; runtime shape is
    // single-typed and chosen by the `ОбходРезультатаЗапроса` arg
    // (default = `Прямой` → `ТаблицаЗначений`). The narrower drops
    // the wrong arm whenever the arg shape is statically recognisable.
    if is_unload_method(method_name.as_str()) {
        return narrow_unload_return(receiver_ty, info, refine_ctx);
    }

    if !is_sdbl_chain_method(method_name.as_str()) {
        return info;
    }
    let refined_ty = refine_ctx
        .and_then(|ctx| try_refine_receiver(ctx, receiver_ty))
        .map(|projections| Ty::Query { projections });
    let effective_ty = refined_ty.as_ref().unwrap_or(receiver_ty);

    let Some((target_platform_name, replacement)) =
        pick_chain_rewrite(effective_ty, method_name.as_str())
    else {
        return info;
    };
    MethodInfoTy {
        return_ty: rewrite_chain_arm_in_return(info.return_ty, target_platform_name, &replacement),
        params: info.params,
        overloads: info.overloads,
    }
}

/// Gate + dispatch for the Phase D refinement helper.
///
/// Returns `Some(projections)` only when the receiver shape is one of
/// the projection-less Query receivers (`Ty::PlatformObject("Запрос")`
/// or `Ty::Query{projections:[None]}` / empty) and the helper finds a
/// well-formed reaching `<var>.Текст = "..."` writer. Any other shape
/// — receivers that already carry a projection, non-Query types,
/// unions — returns `None` so the chain rewrite falls back to the
/// no-projection path unchanged.
///
/// The returned vector mirrors the SDBL package's per-sub-query
/// projection shape (same convention as Phase B's constructor synth):
/// `.Выполнить()` reads the last entry, `.ВыполнитьПакет()[i]` indexes
/// by position.
fn try_refine_receiver(
    ctx: &RefineCtx<'_>,
    receiver_ty: &Ty,
) -> Option<Arc<[Option<Arc<SdblProjection>>]>> {
    if !receiver_needs_refinement(receiver_ty) {
        return None;
    }
    // Receiver must be `Expr::Path(name)` for the dataflow walk to
    // ground itself on a binding key. Anything else (chained call,
    // field of a struct, etc.) is intentionally skipped — those
    // paths either already carry a projection from upstream lowering
    // or aren't covered by the module-local reaching-defs analysis.
    let Expr::Path(receiver_name) = ctx.body.expr(ctx.receiver_expr_id) else {
        return None;
    };
    crate::query_text_dataflow::refine_query_at_use_site(
        ctx.db,
        ctx.file_id,
        ctx.owner,
        ctx.dispatch_expr_id,
        receiver_name,
        ctx.body,
    )
}

/// Whether `receiver_ty` is a Query-shape that still has no
/// projection — the precondition for Phase D refinement.
///
/// Mirrors the union of the two arms accepted by
/// [`projection_of_query_receiver`] that produce `Some(None)`:
/// legacy `Ty::PlatformObject("Запрос")` and the empty / last-arm-None
/// flavours of `Ty::Query`. Receivers that already carry a non-None
/// last projection (the entry `.Выполнить()` will read) are
/// intentionally left alone — refinement is only ever an *upgrade*.
///
/// `pub(crate)` so [`crate::infer`] can share the gate at
/// `infer_path_name` for Phase F (binding-type refinement).
pub(crate) fn receiver_needs_refinement(ty: &Ty) -> bool {
    match ty {
        Ty::PlatformObject(n) => is_platform_name(n, "Запрос", "Query"),
        Ty::Query { projections } => match projections.last() {
            None | Some(None) => true,
            Some(Some(_)) => false,
        },
        _ => false,
    }
}

/// Shape of the platform return arm the chain rewrite is looking for.
///
/// `Запрос.Выполнить()` and `РезультатЗапроса.Выбрать()` return a
/// named [`Ty::PlatformObject`] in the platform data; the rewrite
/// matches on the bilingual name pair. `Запрос.ВыполнитьПакет()`
/// returns `Массив` which lowers to the structural [`Ty::Array`]
/// variant, not `PlatformObject("Массив")` — so the matcher needs
/// both shapes.
enum ChainTarget {
    /// Match `Ty::PlatformObject(name)` where `name` is bilingually
    /// equal to either the Russian or English canonical spelling.
    /// Both are stored so the matcher reaches the same platform-data
    /// row regardless of whether the lowerer produced the RU or EN
    /// form for the cell.
    PlatformObjectNamed { ru: &'static str, en: &'static str },
    /// Match `Ty::Array` (or `Ty::TypedArray(_)` — same platform
    /// method table) for `ВыполнитьПакет → Массив`-style returns.
    AnyArray,
}

/// Pick the (target-arm, replacement-Ty) pair for a given receiver +
/// method name, or `None` when the call is not a recognised SDBL chain
/// entry.
///
/// Receiver-shape guards prevent collisions with other types that share
/// a method name — `.Выбрать()` exists on dialogs, file pickers, mail,
/// and standard-settings-storage managers, so the rewrite must only
/// fire when the receiver is a `Ty::QueryResult` / `Ty::PlatformObject
/// ("РезультатЗапроса")`. Same posture for `.Выполнить()` /
/// `.ВыполнитьПакет()`.
fn pick_chain_rewrite(receiver_ty: &Ty, method_name: &str) -> Option<(ChainTarget, Ty)> {
    let lower = method_name.to_lowercase();
    match lower.as_str() {
        "выполнить" | "execute" => {
            let projection = projection_of_query_receiver(receiver_ty)?;
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "РезультатЗапроса", en: "QueryResult"
                },
                Ty::QueryResult { projection },
            ))
        }
        "выбрать" | "choose" => {
            let projection = projection_of_query_result_receiver(receiver_ty)?;
            Some((
                ChainTarget::PlatformObjectNamed {
                    ru: "ВыборкаИзРезультатаЗапроса",
                    en: "QueryResultSelection",
                },
                Ty::QueryResultSelection { projection },
            ))
        }
        "выполнитьпакет" | "executebatch" => {
            // Receiver must be Query-shape; the slice carried by
            // `Ty::Query.projections` is exactly what
            // `Ty::QueryBatchResult.per_query` consumes — copy by Arc
            // clone (cheap).
            let projections = projections_of_query_receiver(receiver_ty)?;
            Some((ChainTarget::AnyArray, Ty::QueryBatchResult { per_query: projections }))
        }
        _ => None,
    }
}

/// Read the last sub-query's projection off a Query-shape receiver.
///
/// `.Выполнить()` is single-result and 1С returns the **last** query
/// in a batched package (`ВЫБРАТЬ … ПОМЕСТИТЬ Т; ВЫБРАТЬ … ИЗ Т`
/// gives the second SELECT's rows — the `ПОМЕСТИТЬ` stages produce
/// no result). Empty slice and legacy `Ty::PlatformObject("Запрос")`
/// both yield `Some(None)` — the receiver is a query but no projection
/// is available; chain rewrite still fires but produces
/// `Ty::QueryResult{None}`.
///
/// Returns `None` when the receiver is not a query at all, gating
/// accidental rewrites of unrelated `.Выполнить()` calls on dialogs,
/// file pickers, etc.
fn projection_of_query_receiver(ty: &Ty) -> Option<Option<Arc<SdblProjection>>> {
    match ty {
        Ty::Query { projections } => Some(projections.last().cloned().flatten()),
        Ty::PlatformObject(n) if is_platform_name(n, "Запрос", "Query") => Some(None),
        _ => None,
    }
}

/// Read the per-sub-query projection slice off a Query-shape receiver.
///
/// `.ВыполнитьПакет()` returns `Ty::QueryBatchResult` whose `per_query`
/// field has the same shape, so this just hands the slice through.
/// Empty slice and legacy `Ty::PlatformObject("Запрос")` both yield
/// `Some(Arc::from([]))` so the chain rewrite still fires (gating
/// remains intact) but the resulting batch carries no projection.
fn projections_of_query_receiver(ty: &Ty) -> Option<Arc<[Option<Arc<SdblProjection>>]>> {
    match ty {
        Ty::Query { projections } => Some(projections.clone()),
        Ty::PlatformObject(n) if is_platform_name(n, "Запрос", "Query") => {
            Some(Arc::from([]))
        }
        _ => None,
    }
}

/// Sibling of [`projection_of_query_receiver`] for `.Выбрать()`.
///
/// Accepts both the new `Ty::QueryResult{projection}` and the legacy
/// `Ty::PlatformObject("РезультатЗапроса")` / `Ty::PlatformObject
/// ("QueryResult")`. The `Ty::QueryResultSelection` it returns inherits
/// the projection unchanged — `.Выбрать()` is a cursor over the same
/// result schema.
fn projection_of_query_result_receiver(ty: &Ty) -> Option<Option<Arc<SdblProjection>>> {
    match ty {
        Ty::QueryResult { projection } => Some(projection.clone()),
        Ty::PlatformObject(n) if is_platform_name(n, "РезультатЗапроса", "QueryResult") => {
            Some(None)
        }
        _ => None,
    }
}

/// Case-insensitive bilingual name match against the platform's
/// canonical Russian + English spellings.
///
/// Uses [`str::to_lowercase`] so Cyrillic case folds correctly; the
/// platform_data index normalises both forms the same way, so anything
/// the user can write into a `Новый <Name>` lift through to the same
/// methods will also match here.
pub(crate) fn is_platform_name(name: &Name, ru: &str, en: &str) -> bool {
    let lower = name.as_str().to_lowercase();
    lower == ru.to_lowercase() || lower == en.to_lowercase()
}

/// Replace every arm matching `target` in `return_ty` with
/// `replacement`, walking through `Ty::Union` arms.
///
/// Preserves nullability: the platform table declares
/// `Query.Execute → "РезультатЗапроса, Неопределено"` which lowers to
/// `Ty::Union([Ty::PlatformObject("РезультатЗапроса"), Ty::Undefined])`
/// — replacing only the matching arm keeps the `Undefined` companion
/// intact so callers that pattern-match nullability still see it.
fn rewrite_chain_arm_in_return(return_ty: Ty, target: ChainTarget, replacement: &Ty) -> Ty {
    let matches_target = |arm: &Ty| -> bool {
        match (&target, arm) {
            (ChainTarget::PlatformObjectNamed { ru, en }, Ty::PlatformObject(n)) => {
                is_platform_name(n, ru, en)
            }
            (ChainTarget::AnyArray, Ty::Array | Ty::TypedArray(_)) => true,
            _ => false,
        }
    };

    if matches_target(&return_ty) {
        return replacement.clone();
    }
    match return_ty {
        Ty::Union(arms) => {
            let new_arms: Vec<Ty> = arms
                .iter()
                .map(|arm| if matches_target(arm) { replacement.clone() } else { arm.clone() })
                .collect();
            // `Ty::union` re-canonicalises (flatten + sort + dedup) so
            // a one-element result collapses correctly when the
            // original union had only the rewritten arm.
            Ty::union(new_arms)
        }
        other => other,
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
fn lookup_scalar_receiver(receiver_ty: &Ty, method_name: &Name) -> Option<MethodInfoTy> {
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
) -> Option<MethodInfoTy> {
    let res = crate::platform_manager_lookup::resolve_platform_manager_method(
        mdo_type,
        name,
        method_name,
    )?;
    Some(MethodInfoTy {
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
) -> Option<MethodInfoTy> {
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
        return Some(MethodInfoTy {
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
) -> Option<MethodInfoTy> {
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
fn union_lookup(
    members: &[Ty],
    method_name: &Name,
    refine_ctx: Option<&RefineCtx<'_>>,
) -> Option<MethodInfoTy> {
    let live: Vec<&Ty> =
        members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)).collect();
    let mut returns: Vec<Ty> = Vec::with_capacity(live.len());
    let mut chosen_signature: Option<(Vec<Ty>, Vec<Vec<Ty>>)> = None;
    let mut hit_any = false;
    for m in live {
        // Per-arm refinement: `Ty::Union([Запрос, Undefined])` reaches
        // refinement through the live arm so a nullability-typed
        // query receiver still picks up its projection. Phase D's
        // gate (`receiver_needs_refinement`) silently skips arms that
        // are already-projected or non-Query, so non-SDBL unions pay
        // only the cheap discriminant check.
        if let Some(info) = lookup_method_with_refinement_ty(m, method_name, refine_ctx) {
            hit_any = true;
            returns.push(info.return_ty);
            if chosen_signature.is_none() {
                chosen_signature = Some((info.params, info.overloads));
            }
        }
    }
    let (params, overloads) = chosen_signature.unwrap_or_default();
    hit_any.then(|| MethodInfoTy { return_ty: Ty::union(returns), params, overloads })
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
        Ty::ValueTable { .. } => Some("ValueTable"),
        Ty::ValueTableRow { .. } => Some("ValueTableRow"),
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

/// Convert a `PlatformMethod` entry into the semantic `MethodInfoTy`.
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
pub(crate) fn to_method_info(method: &PlatformMethod) -> MethodInfoTy {
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

    MethodInfoTy { return_ty, params, overloads }
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

/// Build a `MethodInfoTy` from a `PlatformData["Tabular section"]` entry,
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
) -> MethodInfoTy {
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

    MethodInfoTy { return_ty, params, overloads }
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
    use crate::ty_bridge::typeid_to_ty;
    use bsl_metadata::MdoType;
    use bsl_platform::MethodParam;
    use bsl_types::testing::InMemoryDb;
    use hir_def::ty::MetadataKind;

    /// Phase 3 §4.E.2a test shim: the public `lookup_method` now takes
    /// `db` and returns a kernel-native `MethodInfo`. These tests build
    /// readable `Ty` fixtures, so this helper passes the `&Ty` receiver
    /// straight through and bridges only the kernel *result* back to the
    /// `Ty`-typed [`MethodInfoTy`] the assertions inspect. A fresh
    /// sandbox [`InMemoryDb`] per call is fine — platform lookup is stateless
    /// w.r.t. the intern table.
    fn lookup(recv: &Ty, method: &Name) -> Option<MethodInfoTy> {
        let db = InMemoryDb::new();
        let info = super::lookup_method(&db, recv, method)?;
        Some(MethodInfoTy {
            return_ty: typeid_to_ty(&db, info.return_ty),
            params: info.params.iter().map(|id| typeid_to_ty(&db, *id)).collect(),
            overloads: info
                .overloads
                .iter()
                .map(|row| row.iter().map(|id| typeid_to_ty(&db, *id)).collect())
                .collect(),
        })
    }

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
        let info = lookup(&Ty::Array, &Name::new("Добавить"))
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
        let info = lookup(&receiver, &Name::new("Добавить"))
            .expect("TypedArray must expose Массив.Добавить through the Array platform page");
        assert_eq!(info.return_ty, Ty::Undefined);

        // `.Количество()` returns Число — proves arithmetic-friendly
        // chaining (`.ВыделенныеСтроки.Количество()` in the form-control
        // refinement targeted by Phase 5) survives the new variant.
        let count = lookup(&receiver, &Name::new("Количество"))
            .expect("TypedArray.Количество must resolve via the Array platform page");
        assert_eq!(count.return_ty, Ty::Number);
    }

    #[test]
    fn method_lookup_unknown_method_returns_none() {
        // A nonsensical method name never hits — the lookup is None, not a
        // poisoned `Ty::Unknown` signature.
        assert!(lookup(&Ty::Array, &Name::new("НеСуществуетТакогоМетода")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_unknown_receiver() {
        // Unknown receiver has no method table.
        assert!(lookup(&Ty::Unknown, &Name::new("Любой")).is_none());
        assert!(lookup(&Ty::Undefined, &Name::new("Любой")).is_none());
        assert!(lookup(&Ty::Null, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_returns_none_for_union_without_live_method() {
        // Primitives expose no instance methods → union of primitives still
        // returns `None` for any method name (every branch misses).
        let u = Ty::union(vec![Ty::Number, Ty::String]);
        assert!(lookup(&u, &Name::new("Любой")).is_none());
    }

    #[test]
    fn method_lookup_union_narrows_past_undefined_sentinel() {
        // `Запрос.Выполнить()` returns `"РезультатЗапроса, Неопределено"`
        // → `Ty::Union([QueryResult, Undefined])`. Chained
        // `.Выгрузить()` must resolve against the live branch (QueryResult
        // has a `Выгрузить` method in platform data) — the `Undefined`
        // sentinel is stripped before dispatch so the chain survives.
        let u = Ty::union(vec![Ty::PlatformObject(Name::new("РезультатЗапроса")), Ty::Undefined]);
        let info = lookup(&u, &Name::new("Выгрузить")).expect(
            "Union([QueryResult, Undefined]).Выгрузить must resolve through the live branch",
        );
        // HBK declares `QueryResult.Выгрузить` as `"ТаблицаЗначений,
        // ДеревоЗначений"` — the narrowing must at least preserve
        // `Ty::ValueTable` in the result so chained `.Добавить()` works.
        let contains_value_table = match &info.return_ty {
            Ty::ValueTable { .. } => true,
            Ty::Union(members) => members.iter().any(|m| matches!(m, Ty::ValueTable { .. })),
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
        let info = lookup(&Ty::PlatformObject(Name::new("Запрос")), &Name::new("Выполнить"));
        assert!(info.is_some(), "PlatformObject(Запрос).Выполнить must resolve");
    }

    #[test]
    fn method_lookup_query_execute_returns_union_with_undefined() {
        // Pins the HBK shape for `Запрос.Выполнить` — return_type is the
        // comma-joined string `"РезультатЗапроса, Неопределено"`. After
        // `to_method_info` splits it AND the SDBL chain rewrite
        // (Phase 1.3) replaces the `РезультатЗапроса` arm with
        // `Ty::QueryResult{None}`, both branches must still be present
        // for any downstream chain to nullability-check.
        let info = lookup(&Ty::PlatformObject(Name::new("Запрос")), &Name::new("Выполнить"))
            .expect("Запрос.Выполнить must resolve in platform data");
        match info.return_ty {
            Ty::Union(members) => {
                assert!(
                    members.iter().any(|m| matches!(m, Ty::QueryResult { projection: None })),
                    "union must include Ty::QueryResult{{None}} (the rewritten РезультатЗапроса arm), got {members:?}",
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
        assert!(lookup(&doc, &Name::new("Любой")).is_none());
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
        let info = lookup(&Ty::ValueTable { projection: None }, &Name::new("Добавить"))
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
        let info = lookup(&om, &Name::new("СоздатьЭлемент"))
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
        assert!(lookup(&om, &Name::new("НетТакогоМетода")).is_none());
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
        let info = lookup(&r, &Name::new("Записать"))
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
        let info = lookup(&r, &Name::new("Сбросить"))
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
        let Some(info) = lookup(&r, &Name::new("Получить")) else {
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
        let info = lookup(&r, &Name::new("Добавить")).expect(
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
        let info =
            lookup(&r, &Name::new("Количество")).expect("TabularSection.Количество must resolve");
        assert_eq!(info.return_ty, Ty::Number);
    }

    #[test]
    fn method_lookup_tabular_section_unload_returns_value_table() {
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info =
            lookup(&r, &Name::new("Выгрузить")).expect("TabularSection.Выгрузить must resolve");
        assert_eq!(info.return_ty, Ty::ValueTable { projection: None });
    }

    #[test]
    fn method_lookup_tabular_section_find_returns_union_with_row() {
        // Платформенный return `"Строка табличной части, Неопределено"`
        // → `Ty::Union([TabularSectionRow, Undefined])`. Pin the
        // member ordering / membership rather than equality so
        // future Ty::union flattening tweaks don't break the test.
        let r = ts_receiver(MdoType::Catalog, "X.Y");
        let info = lookup(&r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
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
        let info =
            lookup(&r, &Name::new("НайтиСтроки")).expect("TabularSection.НайтиСтроки must resolve");
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
        let info = lookup(&r, &Name::new("FindRows"))
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
        let info = lookup(&r, &Name::new("Add"))
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
        assert!(lookup(&r, &Name::new("НетТакогоМетодаНаТЧ")).is_none());
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_document() {
        let r = ts_receiver(MdoType::Document, "ПКО.Товары");
        let info = lookup(&r, &Name::new("Добавить"))
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
        let info = lookup(&r, &Name::new("Добавить"))
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
        let info = lookup(&r, &Name::new("Найти")).expect("TabularSection.Найти must resolve");
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
        let info = lookup(&r, &Name::new("Индекс")).expect("TabularSection.Индекс must resolve");
        assert_eq!(
            info.params,
            vec![Ty::Unknown],
            "Индекс param must stay Ty::Unknown — rebinding would false-reject valid args",
        );
    }

    #[test]
    fn method_lookup_tabular_section_parent_propagates_chart_of_accounts() {
        let r = ts_receiver(MdoType::ChartOfAccounts, "Основной.ВидыСубконто");
        let info = lookup(&r, &Name::new("Добавить"))
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
            lookup(&receiver, &Name::new("Скрыть")).is_some(),
            "<UsualGroup>.Скрыть must resolve via the usual-group extension chain entry"
        );
        assert!(
            lookup(&receiver, &Name::new("Показать")).is_some(),
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
            lookup(&receiver, &Name::new("Скрыть")).is_none(),
            "Pages chain must not borrow UsualGroup-extension methods"
        );
    }

    #[test]
    fn method_lookup_form_control_other_with_empty_chain_returns_none() {
        // `Other` chain is empty → the rev-walk loop runs zero
        // iterations and we return None safely without panicking.
        use hir_def::ty::FormElementKind;
        let receiver = Ty::FormControl { kind: FormElementKind::Other, binding: None };
        assert!(lookup(&receiver, &Name::new("Скрыть")).is_none());
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
        let u = Ty::union(vec![Ty::Array, Ty::ValueTable { projection: None }]);
        let info = lookup(&u, &Name::new("Количество"))
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
        let info = lookup(&receiver, &Name::new("Получить"))
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
        let _ = lookup(&receiver, &Name::new("ОбновитьСтроки"));
    }

    // ============================================================
    // SDBL chain rewrite (Phase 1.3 Slice 1)
    // ============================================================

    fn assert_query_result_in_return(return_ty: &Ty) {
        // `Query.Execute` in platform_data returns
        // `Union([РезультатЗапроса, Неопределено])`. After rewrite we
        // expect the `РезультатЗапроса` arm to become
        // `Ty::QueryResult { projection: None }`; the `Undefined` arm
        // stays unchanged. Either single-arm form (legacy
        // platform data without the Union) or two-arm union are
        // acceptable — the rewrite is shape-preserving.
        let has_query_result = match return_ty {
            Ty::QueryResult { projection: None } => true,
            Ty::Union(arms) => {
                arms.iter().any(|a| matches!(a, Ty::QueryResult { projection: None }))
            }
            _ => false,
        };
        assert!(has_query_result, "expected Ty::QueryResult{{None}} in return, got {return_ty:?}",);
    }

    #[test]
    fn sdbl_chain_rewrite_executes_query() {
        // `Запрос.Выполнить()` — receiver `Ty::PlatformObject("Запрос")`
        // (legacy first-step lift, since Phase 1.3b hasn't yet
        // promoted `Новый Запрос` to `Ty::Query`).
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let info = lookup(&receiver, &Name::new("Выполнить"))
            .expect("Запрос.Выполнить must resolve in platform data");
        assert_query_result_in_return(&info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_executes_query_english_alias() {
        // English `Execute` on the same receiver — bilingual platform
        // index resolves the method, our rewrite must match the
        // English form too.
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let info = lookup(&receiver, &Name::new("Execute"))
            .expect("Запрос.Execute must resolve in platform data");
        assert_query_result_in_return(&info.return_ty);
    }

    #[test]
    fn sdbl_chain_rewrite_choose_on_result() {
        // `РезультатЗапроса.Выбрать()` — receiver currently surfaces
        // as `Ty::PlatformObject("РезультатЗапроса")` until Phase 1.3
        // wires the `.Выполнить()` lift to produce `Ty::QueryResult`.
        let receiver = Ty::PlatformObject(Name::new("РезультатЗапроса"));
        let info = lookup(&receiver, &Name::new("Выбрать"))
            .expect("РезультатЗапроса.Выбрать must resolve in platform data");
        match info.return_ty {
            Ty::QueryResultSelection { projection: None } => {}
            other => panic!("expected QueryResultSelection{{None}}, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_choose_on_typed_result() {
        // Direct invocation on `Ty::QueryResult { None }` — what the
        // hook will receive once Phase 1.3 wires the `.Выполнить()`
        // lift. The lookup path goes through `platform_type_key`
        // (which aliases `Ty::QueryResult` to "РезультатЗапроса" per
        // Phase 0's match-fanout) so the rewrite should fire the same
        // way.
        let receiver = Ty::QueryResult { projection: None };
        let info = lookup(&receiver, &Name::new("Выбрать"))
            .expect("Ty::QueryResult.Выбрать must resolve via platform alias");
        match info.return_ty {
            Ty::QueryResultSelection { projection: None } => {}
            other => panic!("expected QueryResultSelection{{None}}, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_skips_unrelated_choose() {
        // `.Выбрать()` exists on multiple receivers (file pickers,
        // mail, dialogs). The receiver guard must prevent the rewrite
        // from firing on those — they keep their platform return type.
        // `СтандартноеПериод.Выбрать()` is one example: the platform
        // returns `Булево`, not `ВыборкаИзРезультатаЗапроса`.
        let receiver = Ty::PlatformObject(Name::new("СтандартныйПериод"));
        if let Some(info) = lookup(&receiver, &Name::new("Выбрать")) {
            assert!(
                !matches!(info.return_ty, Ty::QueryResultSelection { .. }),
                "rewrite must not fire on non-query receivers — got {:?}",
                info.return_ty,
            );
        }
    }

    #[test]
    fn sdbl_chain_rewrite_execute_batch() {
        let receiver = Ty::PlatformObject(Name::new("Запрос"));
        let info = lookup(&receiver, &Name::new("ВыполнитьПакет"))
            .expect("Запрос.ВыполнитьПакет must resolve in platform data");
        match info.return_ty {
            Ty::QueryBatchResult { ref per_query } => {
                assert!(per_query.is_empty(), "Slice 1 leaves per_query empty; Phase 3 fills it",);
            }
            other => panic!("expected QueryBatchResult, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_preserves_nullability_in_union() {
        // Construct the exact union shape `Query.Execute` returns in
        // platform_data: `Union([РезультатЗапроса, Неопределено])`.
        // After our rewrite walks the union, the `РезультатЗапроса`
        // arm should become `Ty::QueryResult{None}`, the `Undefined`
        // arm must remain untouched.
        let input =
            Ty::union(vec![Ty::PlatformObject(Name::new("РезультатЗапроса")), Ty::Undefined]);
        let rewritten = rewrite_chain_arm_in_return(
            input,
            ChainTarget::PlatformObjectNamed {
                ru: "РезультатЗапроса", en: "QueryResult"
            },
            &Ty::QueryResult { projection: None },
        );
        match rewritten {
            Ty::Union(arms) => {
                assert_eq!(arms.len(), 2);
                assert!(arms.contains(&Ty::QueryResult { projection: None }));
                assert!(arms.contains(&Ty::Undefined));
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn sdbl_chain_rewrite_skips_non_chain_methods() {
        // `.Колонки` is a property, not a chain method. A hypothetical
        // `Запрос.Колонки` (if the method existed) must not be
        // rewritten. Tests the `is_sdbl_chain_method` gate.
        assert!(!is_sdbl_chain_method("Колонки"));
        assert!(!is_sdbl_chain_method("Columns"));
        assert!(!is_sdbl_chain_method("УстановитьПараметр"));
        // Chain methods, both forms.
        assert!(is_sdbl_chain_method("Выполнить"));
        assert!(is_sdbl_chain_method("execute"));
        assert!(is_sdbl_chain_method("ВЫБРАТЬ"));
        assert!(is_sdbl_chain_method("ExecuteBatch"));
    }
}
