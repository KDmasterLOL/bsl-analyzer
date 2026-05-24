//! Type inference for BSL using HIR.
//!
//! This module implements type inference over HIR (Body/Expr/Stmt) instead of AST.
//! This allows:
//! - Diagnostics collection during inference
//! - Efficient caching via Salsa
//! - Simpler code (HIR is already normalized)
//!
//! ## Architecture
//!
//! ```text
//! DefDatabase query: module_bodies(file_id) → Body
//!        ↓
//! HirDatabase query: infer(file_id) → InferenceResult
//!        ↓
//! InferenceContext:
//!   - infer_expr(expr_id) → Ty
//!   - infer_stmt(stmt_id)
//!   - collect diagnostics in result
//! ```
//!
//! ## Phase 1 Scope (MVP)
//!
//! - Basic type inference for literals, binary ops, calls
//! - Method resolution for CommonModule.Method()
//! - Shadowing detection
//! - Diagnostic collection (UnresolvedMethodCall, MismatchedArgCount)

use base_db::FileIdInput;
use bsl_types::builders::Builders;
use bsl_types::facet::{ArgArity, DateComponent, FormDataFacet, MdoRefFacet, ProjectionSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{
    ConfigId, Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId,
    TypeKind,
};
use bsl_types::testing::RootConfigCtx;
use cfg_types::{BindingId, IdConversion};
use hir_def::body::Body;
use hir_def::hir::{BinaryOp, Expr, ExprIdx, Literal, Stmt, StmtIdx, UnaryOp};
use hir_def::resolver::Resolver;
use hir_def::ty::FunctionSignature;
use hir_def::ty::SdblProjection;
use hir_def::ty::Ty;
use hir_def::{sdbl_hir_for_file_query, DefWithBodyId, ExprId, MethodIdInput, Name, SdblExprId};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
use crate::method_resolution;
use crate::platform_manager_lookup::{resolve_platform_manager_method, PlatformMethodResolution};
use crate::ty_bridge::{ty_to_typeid, typeid_to_ty};

/// Result of type inference for a file/module.
///
/// Contains inferred types for all expressions and collected diagnostics.
/// This structure is cached by Salsa.
///
/// `ExprId`s are only unique **within a single `Body`**, so the merged
/// `expr_types_by_body` keys the per-body maps by [`DefWithBodyId`]
/// (method local-id, or `ModuleCode` for module-level code). This is the
/// M3 Task 9 bridge that lets `Semantics::type_of_expr` go from a
/// `SyntaxNode` — resolved through `BodySourceMap::expr_at_range` — to
/// the inferred `Ty`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferenceResult {
    /// Per-body inferred expression types.
    ///
    /// Outer key = body owner (`Method(local_id)` or `ModuleCode`), inner
    /// map = that body's `ExprId -> TypeId`.
    ///
    /// Phase 3 §4.D: inner value storage migrated from `Ty` to `TypeId`.
    /// Callers needing the `Ty` view bridge via [`Self::type_of_expr_in`]
    /// (which takes `db` and returns owned `Ty`), or convert raw `TypeId`
    /// values through [`crate::ty_bridge::typeid_to_ty`].
    ///
    /// Shape note: M3 ships this as a nested map, not a flat
    /// `FxHashMap<(DefWithBodyId, ExprId), TypeId>`. No current caller uses
    /// the "grab the whole body's map in one lookup" shortcut the nesting
    /// would enable (`Semantics::type_of_expr` does a single point
    /// lookup), but the nested shape stays open for Tasks 10-12 hooks
    /// (body-scoped completion, narrowing) that need per-body iteration.
    /// If those never materialise, a later cleanup can flatten.
    pub expr_types_by_body: FxHashMap<DefWithBodyId, FxHashMap<ExprId, TypeId>>,

    /// Variable types inferred from assignments.
    ///
    /// Maps lowercase variable name to its last inferred type.
    /// Populated by tracking `Stmt::Assign { target: Path(name), value }` during inference.
    /// Used by completion to resolve receiver types for method lookup.
    ///
    /// Phase 3 §4.D: stores `TypeId` (interned via the type kernel) instead
    /// of `Ty`. Bridge to `Ty` via [`crate::ty_bridge::typeid_to_ty`].
    ///
    /// File-global merge across all bodies (last-write-wins on lowercase
    /// name). Useful for name-based completion which has no body context.
    /// For binding-anchored lookups (hover on `Для Каждого X Из …`
    /// declaration site, classic-for counter, parameter), prefer
    /// [`InferenceResult::binding_type_in`]: this map collides on
    /// shadowing (same name across procedures or repeated within one
    /// body), `binding_types_by_body` does not.
    pub var_types: FxHashMap<String, TypeId>,

    /// Per-body, per-binding inferred types.
    ///
    /// Keyed by [`BindingId`] (allocated by `Body`'s arena) rather than
    /// lowercase name, so two bindings with the same name in the same
    /// body or in sibling bodies stay isolated. Required for hover at
    /// **declaration-site** identifiers — `Для Каждого X Из …`, classic
    /// `Для X = … По …`, procedure parameters — where there is no
    /// `Expr::Path` for the wrapper-walk to land on. Populated alongside
    /// [`InferenceResult::var_types`] from `Stmt::ForEach` and
    /// `Stmt::For`. Mirrors the per-body shape of
    /// [`InferenceResult::expr_types_by_body`].
    ///
    /// Phase 3 §4.D: inner storage migrated from `Ty` to `TypeId`.
    pub binding_types_by_body: FxHashMap<DefWithBodyId, FxHashMap<BindingId, TypeId>>,

    /// Per-body implicit locals introduced by simple assignments.
    ///
    /// BSL allows `X = ...` without a preceding `Перем X`. Lowering keeps that
    /// distinction out of `ExprScopes`, while inference is the layer that knows
    /// whether the assignment target really behaves as a local variable rather
    /// than a managed-form self property. IDE features use this map to give
    /// implicit locals the same symbol identity as declared locals.
    pub implicit_locals_by_body: FxHashMap<DefWithBodyId, FxHashMap<String, ImplicitLocalInfo>>,

    /// Diagnostics collected during type inference, paired with the body that
    /// produced them.
    ///
    /// Each `InferenceDiagnostic` variant carries an `ExprId`, which is only
    /// unique **within a single `Body`**. The `DefWithBodyId` owner is the key
    /// that lets downstream consumers (ide-diagnostics) recover the right
    /// `BodySourceMap` to resolve `ExprId → TextRange`. Shape mirrors
    /// [`hir_def::ModuleBodies::all_diagnostics`] for consistency with the rest
    /// of the diagnostics pipeline.
    pub diagnostics: Vec<(DefWithBodyId, InferenceDiagnostic)>,

    /// Per-call-site argument bindings recorded during inference,
    /// consumed downstream by the narrowing-aware
    /// [`crate::arg_diagnostics::arg_diagnostics_query`].
    ///
    /// Inference itself does **not** emit `TypeMismatch` for arguments
    /// — it only records the `(args, params)` pair. The downstream
    /// query merges the recorded base types with the
    /// [`crate::narrow`] overlay before deciding whether to emit a
    /// diagnostic, so guards like `If X <> Undefined Then …` correctly
    /// suppress false positives without forcing inference to depend on
    /// narrowing (which would create a Salsa cycle).
    ///
    /// Recorded for every call shape that accepts a typed signature
    /// — workspace `CommonModule.Method`, three-segment manager calls,
    /// receiver method calls (single + multi-overload), platform global
    /// builtins, and `Ty::Function` callees.
    pub call_arg_bindings: Vec<CallArgBinding>,
}

/// Symbol data for an implicit local introduced by assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplicitLocalInfo {
    /// Original spelling from the first assignment target.
    pub name: Name,
    /// HIR expression id of the first assignment target.
    pub first_assignment: ExprId,
    /// Best inferred type observed for the local in this body.
    pub ty: TypeId,
    /// All simple-name assignment sites for this implicit local.
    ///
    /// A single BSL body may reuse the same implicit variable name for
    /// unrelated runtime values in disjoint branches. IDE symbol identity needs
    /// the assignment sites, not only the lowercase name, to split references by
    /// the inferred value type at each occurrence.
    pub assignments: Vec<ImplicitLocalAssignment>,
}

/// One simple-name assignment that contributes to an implicit local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplicitLocalAssignment {
    /// HIR expression id of the assignment target.
    pub target: ExprId,
    /// Inferred type of the assignment value.
    pub ty: TypeId,
}

impl InferenceResult {
    /// Get the type of an expression in a specific body.
    ///
    /// `owner` identifies the body — `DefWithBodyId::Method(local_id)`
    /// for a procedure / function, `DefWithBodyId::ModuleCode` for
    /// module-level code. Returns `None` if inference produced no entry
    /// for that `(owner, expr)` pair.
    ///
    /// Phase 3 §4.D: bridges the stored `TypeId` to owned `Ty` via the
    /// kernel; signature now takes `db` and returns `Option<Ty>` (owned)
    /// instead of `Option<&Ty>`. Internal callers that already hold the
    /// `TypeId` should read the raw map (`expr_types_by_body`) directly.
    pub fn type_of_expr_in(
        &self,
        db: &dyn TypeKernelDb,
        owner: DefWithBodyId,
        expr: ExprId,
    ) -> Option<Ty> {
        let id = *self.expr_types_by_body.get(&owner)?.get(&expr)?;
        Some(typeid_to_ty(db, id))
    }

    /// Raw `TypeId` view of the per-(owner, expr) entry. Cheaper than
    /// [`Self::type_of_expr_in`] for callers that can stay in the
    /// kernel space (no bridge round-trip). Returns `None` when no
    /// inference entry exists.
    pub fn type_id_of_expr_in(&self, owner: DefWithBodyId, expr: ExprId) -> Option<TypeId> {
        self.expr_types_by_body.get(&owner)?.get(&expr).copied()
    }

    /// Get the inferred type of a specific binding in `owner`'s body.
    ///
    /// Used by hover on declaration-site identifiers (loop variables,
    /// classic-for counters, parameters) to avoid name shadowing across
    /// or within bodies — two bindings with the same lowercase name
    /// resolve to distinct entries here.
    ///
    /// Phase 3 §4.D: see [`Self::type_of_expr_in`] for the signature
    /// rationale.
    pub fn binding_type_in(
        &self,
        db: &dyn TypeKernelDb,
        owner: DefWithBodyId,
        id: BindingId,
    ) -> Option<Ty> {
        let typeid = *self.binding_types_by_body.get(&owner)?.get(&id)?;
        Some(typeid_to_ty(db, typeid))
    }

    /// Raw `TypeId` view of the per-(owner, binding) entry. Kernel-native
    /// counterpart to [`Self::binding_type_in`].
    pub fn type_id_of_binding_in(&self, owner: DefWithBodyId, id: BindingId) -> Option<TypeId> {
        self.binding_types_by_body.get(&owner)?.get(&id).copied()
    }

    /// Check if there are any diagnostics.
    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

/// Diagnostics collected during type inference.
///
/// These are lower-level diagnostics that will be converted to user-facing
/// diagnostics in ide-diagnostics layer.
///
/// Uses ExprId instead of TextRange - positions are resolved via BodySourceMap
/// in ide-diagnostics layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceDiagnostic {
    /// Unresolved method call.
    ///
    /// Emitted when:
    /// - 2-segment `Module.Method()` — CommonModule doesn't exist in
    ///   workspace, the method doesn't exist in it, or exists but is
    ///   not exported.
    /// - 3-segment `Plural.MDO.Method()` — workspace `ManagerModule`
    ///   miss with no platform fallback, or non-exported workspace
    ///   method.
    /// - 2-shape `receiver.method()` on a typed receiver
    ///   (`MetadataRef` / `ThisObject` / `ObjectManager`) — both the
    ///   workspace resolver and `lookup_method` returned `None`.
    ///   `Ty::ObjectManager` is authoritative here because the call
    ///   site consults the workspace `ManagerModule.bsl` resolver
    ///   first (Phase A); reaching the platform `lookup_method` miss
    ///   means workspace and platform have both been exhausted.
    ///   Register-kind `MetadataRef` receivers stay silent until
    ///   Phase C wires up `RecordSetModule.bsl`.
    /// - CommonModule source file is missing.
    UnresolvedMethodCall {
        expr: ExprId,
        receiver_name: Name,
        method_name: Name,
        kind: UnresolvedMethodKind,
    },

    /// Mismatched argument count in function call.
    ///
    /// Emitted when `found` falls outside the inclusive range
    /// `[required_count, total_count]` derived from the resolved signature.
    /// `required_count` is the number of leading-required parameters
    /// (computed via [`hir_def::ty::FunctionSignature::required_count`]
    /// — the index of the last param without a default + 1, so non-standard
    /// `(А, Б = ..., В)` orders yield `3`, not `1`). `total_count` is the
    /// full parameter list length. Equal `required` and `total` mean the
    /// signature has no optional parameters; the diagnostic message
    /// renders that as a single number, otherwise as a range.
    ///
    /// `found` is `args.len()` from HIR. BSL allows skipped positional
    /// arguments (`Foo(1,,3)`) which the parser drops; HIR lowering
    /// inserts `Expr::Missing` placeholders so the arg count still
    /// matches the call's syntactic shape. As a consequence, this arity
    /// check does NOT fire when an `Expr::Missing` slot lands on a
    /// required parameter (e.g. `Foo(,2,3)` with `А` required). That
    /// per-slot validation is the job of `MissedRequiredParameter`,
    /// which is already wired with index-aware checks; keeping the two
    /// concerns separate avoids double-reporting.
    MismatchedArgCount {
        call_expr: ExprId,
        required_count: usize,
        total_count: usize,
        found: usize,
    },

    /// Type mismatch between expected and actual type.
    ///
    /// Emitted when expression type doesn't match expected type
    /// (e.g., assigning String to Number variable).
    TypeMismatch { expr: ExprId, expected: TypeId, actual: TypeId },

    /// Field access on a typed receiver did not resolve.
    ///
    /// Emitted from `Expr::Field` when [`crate::field_lookup::lookup_field`]
    /// returns `None` **and** the receiver carries enough type information
    /// for the gap to be user-actionable — i.e. the receiver is not
    /// [`Ty::Unknown`] (no type info to disagree with) and not a
    /// [`Ty::Union`] (field lookup on unions defers to M4 narrowing, so a
    /// `None` there is "can't decide yet", not "field does not exist").
    ///
    /// `receiver_ty` captures the type as seen at the access site so the
    /// IDE layer can render `<CatalogRef.Номенклатура>.НеСуществует` in
    /// the diagnostic message without re-running inference.
    UnresolvedField { expr: ExprId, receiver_ty: TypeId, field_name: Name },

    /// Assignment to a field whose platform-property entry carries
    /// `is_readonly = true` in HBK (`Использование:` chapter reads
    /// `"Только чтение"`).
    ///
    /// Emitted from `Stmt::Assign` when the LHS is `Expr::Field` and
    /// [`crate::field_lookup::lookup_field`] returns a [`crate::FieldInfo`]
    /// flagged read-only. Propagated to the IDE layer as the
    /// `ReadOnlyPropertyAssignment` diagnostic.
    ///
    /// `lhs` anchors the diagnostic to the field-access expression so the
    /// editor underlines `.Параметры` rather than the whole statement;
    /// `receiver_ty` and `field_name` feed the message body.
    ReadOnlyPropertyAssignment { lhs: ExprId, receiver_ty: TypeId, field_name: Name },

    /// Redundant `<CommonModule>.<Method>()` self-quoted call inside the
    /// owning CommonModule body — counterpart to `BodyDiagnostic::
    /// RedundantAccessToObject { kind: TwoLevel }`, but emitted from
    /// inference instead of body lowering.
    ///
    /// Lifted into the inference layer so the lowering decision
    /// "this is a CommonModule call" no longer happens without the
    /// receiver's resolved type — the lowering layer cannot tell a
    /// CommonModule receiver apart from a form attribute, an implicit
    /// form global, or a module-level `Перем` declaration. Inference
    /// has the resolver and the type, so this variant fires only when
    /// the cascade gate confirms `user_common_module_exists`.
    ///
    /// Adapter in `ide-diagnostics` reuses the existing
    /// `handlers::redundant_access_to_object::from_hir` by reconstructing
    /// `RedundantAccessKind::TwoLevel { module: <Name as String> }`.
    RedundantAccessToObjectTwoLevel { expr: ExprId, module: Name },

    /// `MissedRequiredParameter` for a `<CommonModule>.<Method>(...)` call
    /// — counterpart to `BodyDiagnostic::MissedRequiredParameter` with
    /// `module: Some(...)` (two-level shape), but emitted from inference.
    ///
    /// Same lift rationale as `RedundantAccessToObjectTwoLevel`: only
    /// inference can confirm the receiver actually resolves to a
    /// CommonModule before validating the parameter set against its
    /// signature. Three-level (`Документы.ПКО.Method`) calls keep using
    /// the `BodyDiagnostic` path since the lowering positive
    /// `MdoType::from_plural` gate is sufficient there.
    ///
    /// `args` reconstruction mirrors lowering's `extract_arg_presence`
    /// but operates on `&[ExprId]` already available to `infer_call`:
    /// `args.iter().map(|id| !matches!(body.expr(*id), Expr::Missing))`.
    MissedRequiredParameterCommonModule {
        expr: ExprId,
        callee: Name,
        module: Name,
        args: Vec<bool>,
    },
}

/// Kind of unresolved method call error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedMethodKind {
    /// Method not found in the module.
    MethodNotFound,
    /// Method exists but is not exported.
    MethodNotExport,
    /// CommonModule source file is missing.
    CommonModuleNoSource,
    /// Receiver type could not be resolved.
    ReceiverNotResolved,
}

/// Per-call-site record of `(args, params)` shape captured during
/// inference for downstream narrowing-aware validation.
///
/// `ExprId`s are body-local, so [`Self::owner`] disambiguates the body
/// they belong to. The narrowing-aware
/// [`crate::arg_diagnostics::arg_diagnostics_query`] reads each binding,
/// re-computes the per-arg type with the narrowing overlay, and emits
/// the [`InferenceDiagnostic::TypeMismatch`] entries inference no longer
/// produces directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgBinding {
    /// Body that owns `call_expr` and the entries of `args` —
    /// `DefWithBodyId::Method(local_id)` for a procedure / function
    /// body, `DefWithBodyId::ModuleCode` for module-level code.
    pub owner: DefWithBodyId,

    /// `ExprId` of the call expression itself. Used as the diagnostic
    /// anchor (and to look up the call's source range when emitting
    /// `TypeMismatch` for a particular `args[i]` slot).
    pub call_expr: ExprId,

    /// HIR `ExprId`s of the supplied arguments, in source order. Empty
    /// when the call has no arguments. The `arg_diagnostics_query`
    /// zips this against [`Self::params`] to compare per-slot types.
    pub args: Vec<ExprId>,

    /// Parameter signature shape — single or multi-overload. Used by
    /// `arg_diagnostics_query` to apply the right validation rule
    /// (per-arg `is_assignable` for `Single`; `any_accepts` +
    /// closest-by-arity fallback for `Overloaded`).
    pub params: ParamsShape,
}

/// Parameter-list shape captured per call site.
///
/// `Arc<[TypeId]>` is used (instead of `Vec<TypeId>`) so that the same signature
/// can be shared across many call sites of the same method without
/// re-allocating the parameter list per record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamsShape {
    /// Single parameter list — covers plain functions, workspace
    /// `CommonModule.Method`, three-segment manager calls,
    /// non-overloaded receiver methods, and `Ty::Function` callees.
    Single(Arc<[TypeId]>),

    /// Multi-overload signature.
    ///
    /// `flat` is the legacy "flattened" parameter list used as a
    /// fallback when the overload set is empty (mirrors the old
    /// `emit_arg_type_mismatches_overloaded` contract). `overloads`
    /// carries one entry per declared overload — validation accepts
    /// the call when any entry's per-arg `is_assignable` check passes,
    /// and falls back to the closest-by-arity overload for the
    /// diagnostic message otherwise.
    Overloaded { flat: Arc<[TypeId]>, overloads: Arc<[Arc<[TypeId]>]> },
}

/// Context for type inference.
///
/// Performs type inference for a **single body** (one method, or the
/// module-level code block), building up expr-type and diagnostic
/// sub-results that `infer_query` merges into the file-level
/// [`InferenceResult`].
pub struct InferenceContext<'db> {
    /// Database for queries.
    ///
    /// Used for method resolution, metadata queries, workspace symbols.
    db: &'db dyn HirDatabase,

    /// File being inferred.
    ///
    /// Used for diagnostics reporting and workspace file collection.
    file_id: FileId,

    /// Body owner. Preserved so the per-body `expr_types` map can be
    /// keyed by `DefWithBodyId` when `finish()` folds into
    /// [`InferenceResult::expr_types_by_body`].
    owner: DefWithBodyId,

    /// HIR body for the file.
    body: Arc<Body>,

    /// Phase O.10 — `ExprId` of every `Stmt::Return { value: Some(_) }`
    /// statement reached during inference. Populated by
    /// [`Self::infer_stmt`]'s `Stmt::Return` arm after `infer_expr`
    /// runs; consumed by `method_return_type_query` via
    /// [`BodyInferenceResult::return_expr_ids`] to compute the per-method
    /// return type without re-walking the body. `infer_query` ignores
    /// this field — it only matters for the per-method cascade query.
    return_expr_ids: Vec<ExprId>,

    /// Variable types tracked from assignments (lowercase name → TypeId).
    ///
    /// **Conservative**: an `X = …` whose RHS infers to `Ty::Unknown`
    /// is NOT inserted here — keeping the entry from a previous,
    /// more informative assignment (`X = "string"; X = НеизвестнаяФункция()`
    /// keeps `X` typed as `String`). The companion
    /// [`Self::assigned_var_names`] set is the cheap probe for
    /// "is this name an implicit local at all?", regardless of
    /// whether we have a useful type for it.
    var_types: FxHashMap<String, TypeId>,

    /// Implicit locals introduced by simple assignments in this body.
    implicit_locals: FxHashMap<String, ImplicitLocalInfo>,

    /// Lowercase names of every implicit local seen on the LHS of a
    /// `Stmt::Assign`, regardless of the RHS's inferred type.
    ///
    /// `var_types` is intentionally type-aware (only inserts when
    /// the RHS yields a non-`Unknown` `Ty`), but the cascade-gate in
    /// `dispatch_bare_ident_field_call` needs to know whether a
    /// name is a body-local — even an "untyped" one whose RHS came
    /// back `Ty::Unknown` — so it can short-circuit silent in
    /// gate 2 instead of walking all the way to gate 5 and
    /// emitting a misleading `ReceiverNotResolved` for an entirely
    /// normal local-variable call (e.g.
    /// `Х = НеизвестнаяФункция(); Х.Метод()`).
    ///
    /// This set is write-only inside `infer_stmt`'s `Stmt::Assign`
    /// arm and read-only in the cascade gate; nothing else needs to
    /// observe it, so we don't fold it into [`BodyInferenceResult`].
    assigned_var_names: rustc_hash::FxHashSet<String>,

    /// Per-binding types written by declaration-site arms
    /// (`Stmt::ForEach`, `Stmt::For`). Keyed by `BindingId` so name
    /// shadowing within the body does not collide. Surfaced through
    /// [`InferenceResult::binding_type_in`] for hover.
    binding_types: FxHashMap<BindingId, TypeId>,

    /// Per-body `ExprId -> Ty` cache. Doubles as the memoisation table
    /// (`infer_expr` short-circuits when the entry already exists) and
    /// as the payload we hand to the merged result keyed by `owner`.
    expr_types: FxHashMap<ExprId, TypeId>,

    /// Diagnostics collected while inferring this body.
    diagnostics: Vec<InferenceDiagnostic>,

    /// Call-site arg/param bindings recorded during inference for the
    /// downstream narrowing-aware
    /// [`crate::arg_diagnostics::arg_diagnostics_query`].
    call_arg_bindings: Vec<CallArgBinding>,
}

/// Per-method (or module-code) inference output.
///
/// Phase O.13 (Lni.1) promotes this from an `infer_query`-internal
/// intermediate to the public per-method Salsa surface that
/// `infer_method_query` (O.15) returns. `infer_query` keeps using
/// `InferenceContext::finish` to fold per-body results into the
/// file-level [`InferenceResult`], so this struct does double duty:
/// the public per-method payload and the fold input.
///
/// All collection fields are `pub` so narrow callers (per-method hover,
/// completion, narrow_query) can read them through
/// [`InferOwnerResult::Method`]; `return_expr_ids` stays `pub(crate)`
/// because its sole consumer is `method_return_type_query` inside this
/// crate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyInferenceResult {
    /// Owner of the body that produced this output.
    pub owner: DefWithBodyId,
    /// Variable types discovered during inference (lowercase name → `TypeId`).
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`. Bridge via
    /// [`crate::ty_bridge::typeid_to_ty`].
    pub var_types: FxHashMap<String, TypeId>,
    /// Implicit locals introduced by simple assignments in this body.
    pub implicit_locals: FxHashMap<String, ImplicitLocalInfo>,
    /// Per-binding inferred types (declaration-site arms). Folded into
    /// [`InferenceResult::binding_types_by_body`] keyed by `owner`.
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`.
    pub binding_types: FxHashMap<BindingId, TypeId>,
    /// Expression types keyed by body-local `ExprId`.
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`.
    pub expr_types: FxHashMap<ExprId, TypeId>,
    /// Diagnostics collected during inference.
    pub diagnostics: Vec<InferenceDiagnostic>,
    /// Call-site arg/param bindings collected during inference, to be
    /// folded into [`InferenceResult::call_arg_bindings`].
    pub call_arg_bindings: Vec<CallArgBinding>,

    /// Phase O.10 — `ExprId`s of every `Stmt::Return { value: Some(_) }`
    /// statement reached during inference. Crate-private because the
    /// only consumer is `method_return_type_query` (same `hir-ty`
    /// crate). `infer_query` does not surface this — it is purely the
    /// per-method cascade query's view of the body.
    pub(crate) return_expr_ids: Vec<ExprId>,
}

impl BodyInferenceResult {
    /// Phase O.13 — empty payload for an owner whose inference produced
    /// no entries. Used by `infer_method_query` (O.15) to return a
    /// well-typed default when the requested method does not exist in
    /// the resolved module — cheaper than threading `Option` through
    /// every narrow caller. All collection fields are default-empty;
    /// the only meaningful slot is `owner`.
    pub fn empty_for(owner: DefWithBodyId) -> Self {
        Self {
            owner,
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
            return_expr_ids: Vec::new(),
        }
    }
}

/// Module-code inference output.
///
/// Phase O.13 (Lni.2) introduces this struct as the public Salsa
/// surface that `infer_module_code_query` (O.14) returns. Mirrors
/// [`BodyInferenceResult`] structurally — the two diverge only in that
/// `ModuleCodeInferenceResult::owner` is always
/// [`DefWithBodyId::ModuleCode`], and there is no `return_expr_ids`
/// (module code has no return statements to track for the cascade
/// query). `from_body` is the canonical conversion path: the underlying
/// `InferenceContext::finish` always produces a `BodyInferenceResult`
/// regardless of owner kind, so module-code inference reuses the same
/// machinery and lifts the result into this dedicated type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCodeInferenceResult {
    /// Always [`DefWithBodyId::ModuleCode`]. Kept explicit so the
    /// struct can be inspected uniformly with `BodyInferenceResult`.
    pub owner: DefWithBodyId,
    /// Variable types discovered during inference (lowercase name → `TypeId`).
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`.
    pub var_types: FxHashMap<String, TypeId>,
    /// Implicit locals introduced by simple assignments in module code.
    pub implicit_locals: FxHashMap<String, ImplicitLocalInfo>,
    /// Per-binding inferred types (declaration-site arms).
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`.
    pub binding_types: FxHashMap<BindingId, TypeId>,
    /// Expression types keyed by body-local `ExprId`.
    ///
    /// Phase 3 §4.D: storage migrated from `Ty` to `TypeId`.
    pub expr_types: FxHashMap<ExprId, TypeId>,
    /// Diagnostics collected during inference.
    pub diagnostics: Vec<InferenceDiagnostic>,
    /// Call-site arg/param bindings collected during inference.
    pub call_arg_bindings: Vec<CallArgBinding>,
}

impl Default for ModuleCodeInferenceResult {
    fn default() -> Self {
        Self {
            owner: DefWithBodyId::ModuleCode,
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
        }
    }
}

impl ModuleCodeInferenceResult {
    /// Lift a `BodyInferenceResult` produced from a module-code body
    /// (`DefWithBodyId::ModuleCode`) into the dedicated module-code
    /// type. Drops `return_expr_ids` (always empty for module code by
    /// the parser: module-level statements never reach `Stmt::Return`).
    ///
    /// Debug-asserts the owner is `ModuleCode` to catch mis-routed
    /// inference results during development; release builds trust the
    /// caller.
    pub fn from_body(body: BodyInferenceResult) -> Self {
        debug_assert!(
            matches!(body.owner, DefWithBodyId::ModuleCode),
            "ModuleCodeInferenceResult::from_body requires DefWithBodyId::ModuleCode owner"
        );
        Self {
            owner: body.owner,
            var_types: body.var_types,
            implicit_locals: body.implicit_locals,
            binding_types: body.binding_types,
            expr_types: body.expr_types,
            diagnostics: body.diagnostics,
            call_arg_bindings: body.call_arg_bindings,
        }
    }
}

/// Routing enum returned by `infer_owner` to unify per-method
/// and module-code Salsa outputs behind a single accessor surface.
///
/// Phase O.13 (Lni.3) introduces this enum as the migration target for
/// every narrow caller currently doing `db.infer(file_id)` followed by
/// a body-keyed lookup. Once `infer_method_query` (O.15) and
/// `infer_module_code_query` (O.14) land, narrow callers route
/// `db.infer_method(...)` / `db.infer_module_code(...)` through this
/// enum and read per-owner state without touching the file-wide
/// aggregate.
///
/// Accessors mirror the per-owner slices of [`InferenceResult`]
/// (`expr_types_by_body[owner]`, `implicit_locals_by_body[owner]`,
/// `var_types`) so the call-site shape stays uniform between Method
/// and ModuleCode variants.
#[derive(Debug, Clone)]
pub enum InferOwnerResult {
    /// Result of `infer_method_query` for a specific method body.
    Method(Arc<BodyInferenceResult>),
    /// Result of `infer_module_code_query` for the file's module code.
    ModuleCode(Arc<ModuleCodeInferenceResult>),
}

impl InferOwnerResult {
    /// Owner of the body whose inference produced this result.
    ///
    /// `DefWithBodyId::Method(local_id)` for the `Method` variant,
    /// always `DefWithBodyId::ModuleCode` for the `ModuleCode` variant.
    pub fn owner(&self) -> DefWithBodyId {
        match self {
            InferOwnerResult::Method(r) => r.owner,
            InferOwnerResult::ModuleCode(r) => r.owner,
        }
    }

    /// Type of expression `expr` within this owner's body, if inference
    /// recorded one. Bridges the stored `TypeId` to owned `Ty` via the
    /// type kernel.
    ///
    /// Phase 3 §4.D: signature takes `db` and returns `Option<Ty>`
    /// (owned) instead of `Option<&Ty>`. Callers that can stay in
    /// kernel space should use [`Self::type_id_of_expr`].
    pub fn type_of_expr(&self, db: &dyn TypeKernelDb, expr: ExprId) -> Option<Ty> {
        let id = self.type_id_of_expr(expr)?;
        Some(typeid_to_ty(db, id))
    }

    /// Raw `TypeId` view of `expr` within this owner's body — kernel-native
    /// counterpart to [`Self::type_of_expr`].
    pub fn type_id_of_expr(&self, expr: ExprId) -> Option<TypeId> {
        match self {
            InferOwnerResult::Method(r) => r.expr_types.get(&expr).copied(),
            InferOwnerResult::ModuleCode(r) => r.expr_types.get(&expr).copied(),
        }
    }

    /// Full `ExprId -> TypeId` map for this owner's body. Callers that
    /// need bulk access (e.g. `narrow_query` building a `base_types`
    /// overlay across all expressions in the body) use this in
    /// preference to repeated [`Self::type_id_of_expr`] calls.
    ///
    /// Phase 3 §4.D: map values migrated from `Ty` to `TypeId`. Callers
    /// needing `Ty` views must bridge per-entry via
    /// [`crate::ty_bridge::typeid_to_ty`].
    pub fn expr_types(&self) -> &FxHashMap<ExprId, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.expr_types,
            InferOwnerResult::ModuleCode(r) => &r.expr_types,
        }
    }

    /// Type of `binding` within this owner's body, if inference
    /// recorded one. Equivalent to
    /// `InferenceResult::binding_type_in(owner, binding)` but reads
    /// directly from the per-owner payload.
    ///
    /// Phase 3 §4.D: see [`Self::type_of_expr`] for the signature
    /// rationale.
    pub fn type_of_binding(&self, db: &dyn TypeKernelDb, binding: BindingId) -> Option<Ty> {
        let id = self.type_id_of_binding(binding)?;
        Some(typeid_to_ty(db, id))
    }

    /// Raw `TypeId` view of `binding` within this owner's body.
    pub fn type_id_of_binding(&self, binding: BindingId) -> Option<TypeId> {
        match self {
            InferOwnerResult::Method(r) => r.binding_types.get(&binding).copied(),
            InferOwnerResult::ModuleCode(r) => r.binding_types.get(&binding).copied(),
        }
    }

    /// Variable types map (lowercase name → `TypeId`) for this owner.
    ///
    /// Phase 3 §4.D: map values migrated to `TypeId`.
    pub fn var_types(&self) -> &FxHashMap<String, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.var_types,
            InferOwnerResult::ModuleCode(r) => &r.var_types,
        }
    }

    /// Implicit-locals map for this owner — completion uses this to
    /// surface assignment-introduced locals at the cursor.
    pub fn implicit_locals(&self) -> &FxHashMap<String, ImplicitLocalInfo> {
        match self {
            InferOwnerResult::Method(r) => &r.implicit_locals,
            InferOwnerResult::ModuleCode(r) => &r.implicit_locals,
        }
    }

    /// Per-binding inferred types for declaration-site identifiers
    /// (loop variables, classic-for counters, parameters).
    ///
    /// Phase 3 §4.D: map values migrated to `TypeId`.
    pub fn binding_types(&self) -> &FxHashMap<BindingId, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.binding_types,
            InferOwnerResult::ModuleCode(r) => &r.binding_types,
        }
    }
}

/// Phase O.17 — narrow-caller routing entry point.
///
/// Routes a `(file_id, owner)` pair to the appropriate per-owner
/// Salsa query and wraps the result in [`InferOwnerResult`]. This is
/// the migration target for every narrow caller previously doing
/// `db.infer(file_id).<body-keyed-slice>` — after O.17 each call
/// site reaches per-owner state directly, bypassing the file-wide
/// `infer_query` aggregate. Warm narrow paths now skip the wrapper
/// entirely (Arc::clone on the per-method cell), and cold cross-file
/// hits invalidate only the touched method instead of every method
/// in the file.
pub fn infer_owner(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
) -> InferOwnerResult {
    match owner {
        DefWithBodyId::Method(local_id) => {
            let module_id = hir_def::ModuleId { file_id };
            let method_id = hir_def::MethodId { module: module_id, local_id };
            let method_input = MethodIdInput::new(db, method_id);
            InferOwnerResult::Method(db.infer_method(method_input))
        }
        DefWithBodyId::ModuleCode => InferOwnerResult::ModuleCode(db.infer_module_code(file_id)),
    }
}

impl<'db> InferenceContext<'db> {
    /// Create a new inference context for a single body.
    ///
    /// `owner` records whether the body is a method (and which
    /// `local_id`) or module-level code. `infer_query` supplies the
    /// right value per call.
    pub fn new(
        db: &'db dyn HirDatabase,
        file_id: FileId,
        owner: DefWithBodyId,
        body: &Arc<Body>,
    ) -> Self {
        Self {
            db,
            file_id,
            owner,
            body: Arc::clone(body),
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            assigned_var_names: rustc_hash::FxHashSet::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
            return_expr_ids: Vec::new(),
        }
    }

    /// Phase O.7 — body-scoped factory anchored on a single method.
    ///
    /// Thin wrapper over [`Self::new`] that derives `file_id` and
    /// `owner` from the salsa-interned [`MethodIdInput`]. The body is
    /// supplied by the caller so the context can be built without
    /// going through `infer_query` / `module_bodies_query`. This is
    /// the per-method inference primitive that the upcoming method-graph
    /// queries (O.8+) will drive.
    ///
    /// O.7 ships the constructor alone; cascade typing and production
    /// callers land in later commits (O.8–O.11). Coverage here is
    /// unit-test only.
    ///
    /// # Body type
    ///
    /// Takes `&Arc<Body>` to match [`Self::new`] and to avoid an extra
    /// clone when the caller already holds the `Arc`. Callers that
    /// only have a `&Body` can pass `&Arc::new(body.clone())`.
    pub fn new_for_method(
        db: &'db dyn HirDatabase,
        method: MethodIdInput<'db>,
        body: &Arc<Body>,
    ) -> Self {
        let mid = method.method_id(db);
        Self::new(db, mid.module.file_id, DefWithBodyId::Method(mid.local_id), body)
    }

    /// Suppress inference diagnostics whose key expression was lowered
    /// from a parser ERROR node.
    ///
    /// Rust-analyzer-style recovery (`hir-def/src/body/lower/stmt.rs`) lets
    /// us type-check expressions the user is still typing (`Сп.В`, bare
    /// field access, etc.). Those expressions intentionally lack full
    /// syntactic context, so firing `UnresolvedField` /
    /// `UnresolvedMethodCall` / `MismatchedArgCount` / `TypeMismatch` on
    /// them would flicker in the editor as the user types. The recovery
    /// marker (`Body::is_recovered`) is the single source of truth for
    /// "this expression came from an ERROR node, don't complain".
    ///
    /// Call this instead of `self.diagnostics.push(...)` whenever the
    /// diagnostic is anchored to an expression the user could still be
    /// editing.
    fn push_inference_diagnostic(&mut self, diag: InferenceDiagnostic) {
        let key = match &diag {
            InferenceDiagnostic::UnresolvedMethodCall { expr, .. } => *expr,
            InferenceDiagnostic::MismatchedArgCount { call_expr, .. } => *call_expr,
            InferenceDiagnostic::TypeMismatch { expr, .. } => *expr,
            InferenceDiagnostic::UnresolvedField { expr, .. } => *expr,
            InferenceDiagnostic::ReadOnlyPropertyAssignment { lhs, .. } => *lhs,
            InferenceDiagnostic::RedundantAccessToObjectTwoLevel { expr, .. } => *expr,
            InferenceDiagnostic::MissedRequiredParameterCommonModule { expr, .. } => *expr,
        };
        if self.body.is_recovered(key) {
            return;
        }
        self.diagnostics.push(diag);
    }

    fn is_unknown(&self, id: TypeId) -> bool {
        id == self.db.unknown()
    }

    /// Finish inference and return the per-body output.
    pub fn finish(self) -> BodyInferenceResult {
        BodyInferenceResult {
            owner: self.owner,
            var_types: self.var_types,
            implicit_locals: self.implicit_locals,
            binding_types: self.binding_types,
            expr_types: self.expr_types,
            diagnostics: self.diagnostics,
            call_arg_bindings: self.call_arg_bindings,
            return_expr_ids: self.return_expr_ids,
        }
    }

    /// Record a call-site `(args, params)` binding for downstream
    /// narrowing-aware validation by
    /// [`crate::arg_diagnostics::arg_diagnostics_query`].
    ///
    /// Replaces the legacy `emit_arg_type_mismatches[_overloaded]`
    /// inline emit: inference no longer decides whether arguments
    /// type-match (that decision needs the narrowing overlay, which
    /// runs after inference); it only saves the raw shape so the
    /// downstream query can replay the check against narrowed types.
    ///
    /// The recovery-marker filter from `push_inference_diagnostic`
    /// applies here too: bindings whose `call_expr` was lowered from a
    /// parser ERROR node are dropped so the editor doesn't flicker
    /// `TypeMismatch` while the user is still typing.
    fn record_call_arg_binding(&mut self, call_expr: ExprId, args: &[ExprId], params: ParamsShape) {
        if self.body.is_recovered(call_expr) {
            return;
        }
        self.call_arg_bindings.push(CallArgBinding {
            owner: self.owner,
            call_expr,
            args: args.to_vec(),
            params,
        });
    }

    /// Get resolver for the current module.
    ///
    /// Includes `Scope::Builtins` so that platform globals (`Сообщить`,
    /// `ТекущаяДата`, ...) are recognised by `Resolver::resolve_name`; this
    /// lets inference share the same lookup cascade as hover / goto-def.
    fn get_resolver(&self) -> Resolver {
        let module_id = hir_def::ModuleId { file_id: self.file_id };
        Resolver::with_builtins_and_workspace(module_id)
    }

    /// Case-insensitive probe: does the current body declare a parameter or
    /// local variable named `name`?
    ///
    /// `get_resolver` does not push `Scope::ExprScope`, so
    /// `Resolver::resolve_name` never returns `Resolution::Local` for these.
    /// Implicit locals derived from `Stmt::Assign` are tracked via
    /// `var_types`, but **declared** parameters and bare `Перем` locals
    /// without a prior assignment never reach `var_types` either — they
    /// only live as `Body::Binding`s. Form-self resolution must consult
    /// this list to honour BSL's "explicit user declaration shadows
    /// implicit Self" rule (e.g. `Процедура Тест(Заголовок)` must read the
    /// parameter, not the form's `Заголовок` property).
    fn body_declares_binding(&self, name: &hir_def::Name) -> bool {
        let target = name.as_str().to_lowercase();
        self.body.bindings_iter().any(|(_, b)| b.name.as_str().to_lowercase() == target)
    }

    /// Infer types for all expressions in the body.
    ///
    /// Walks statements top-down to track variable types from assignments,
    /// then infers remaining expressions. This ensures `Expr::Path` lookups
    /// see variable types from prior assignments.
    pub fn infer_all(&mut self) {
        let _p = tracing::debug_span!("infer_all").entered();

        // Walk statements to track variable types from assignments
        let stmts: Vec<StmtIdx> = self.body.body_stmts_typed().to_vec();
        self.infer_stmts(&stmts);

        // Infer remaining expressions not reached via statements
        let expr_ids: Vec<ExprId> = self.body.exprs_iter().map(|(id, _)| id).collect();
        for expr_id in expr_ids {
            self.infer_expr(expr_id);
        }

        debug!(
            "inferred {} expression types, {} var types, {} diagnostics",
            self.expr_types.len(),
            self.var_types.len(),
            self.diagnostics.len()
        );
    }

    /// Walk a list of statements, inferring types and tracking variable assignments.
    fn infer_stmts(&mut self, stmts: &[StmtIdx]) {
        for &stmt_idx in stmts {
            self.infer_stmt(stmt_idx);
        }
    }

    /// Infer types for a single statement.
    fn infer_stmt(&mut self, stmt_idx: StmtIdx) {
        let stmt = self.body.stmt_idx(stmt_idx).clone();
        match &stmt {
            Stmt::Assign { target, value } => {
                let value_ty = self.infer_expr(ExprId::from_idx(*value));

                // Track variable type if target is a simple name
                let target_expr = self.body.expr_idx(*target).clone();
                match &target_expr {
                    Expr::Path(name) => {
                        // Managed-form Self assignment gate: if `name` is a
                        // property of `ФормаКлиентскогоПриложения` in a
                        // managed-form module, the LHS is the form property
                        // itself, not a fresh implicit local. We must NOT
                        // pollute `var_types` — otherwise a later
                        // `X = Заголовок;` would read the RHS-derived local
                        // (e.g. `Ty::String` from a literal) instead of the
                        // platform-typed property and the form-self resolver
                        // in `infer_path_name` would never get a chance.
                        // Read-only properties additionally emit
                        // `ReadOnlyPropertyAssignment`, mirroring the
                        // `Expr::Field` arm below.
                        //
                        // Shadowing rule mirrors `infer_path_name`'s step
                        // 4: a module-level `Метод()` or `Перем`, AND any
                        // parameter or local `Перем` declared in this
                        // body, take priority over the form-self property
                        // (BSL semantics — explicit user declaration wins
                        // over implicit self). `body_declares_binding`
                        // covers the parameter / declared-local gap left
                        // by `Resolver::resolve_name` (no ExprScope) and
                        // `var_types` (no entry until first assign).
                        let resolver = self.get_resolver();
                        let resolved_name = resolver.resolve_name(self.db, name);
                        let existing_module_variable = matches!(
                            resolved_name,
                            Some(hir_def::resolver::Resolution::Variable(_))
                        );
                        let user_shadows = matches!(
                            resolved_name,
                            Some(hir_def::resolver::Resolution::Method(_))
                                | Some(hir_def::resolver::Resolution::Variable(_))
                        ) || self.body_declares_binding(name);
                        let form_self_resolution = if user_shadows {
                            None
                        } else {
                            crate::form_self::resolve_form_self_property(self.db, &resolver, name)
                        };
                        match form_self_resolution {
                            Some(prop) => {
                                if prop.is_readonly {
                                    let form_ty = self.db.platform_object(
                                        crate::form_self::FORM_TYPE_NAME.to_string(),
                                    );
                                    self.push_inference_diagnostic(
                                        InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                            lhs: ExprId::from_idx(*target),
                                            receiver_ty: form_ty,
                                            field_name: name.clone(),
                                        },
                                    );
                                }
                            }
                            None if existing_module_variable => {}
                            None => {
                                let key = name.as_str().to_lowercase();
                                // Always remember the name as a body-local
                                // (cascade-gate gate 2 needs this even when
                                // the RHS yields no useful type info).
                                self.assigned_var_names.insert(key.clone());
                                let target_id = ExprId::from_idx(*target);
                                let unknown = self.db.unknown();
                                self.implicit_locals
                                    .entry(key.clone())
                                    .and_modify(|info| {
                                        info.assignments.push(ImplicitLocalAssignment {
                                            target: target_id,
                                            ty: value_ty,
                                        });
                                        if info.ty == unknown && value_ty != unknown {
                                            info.ty = value_ty;
                                        }
                                    })
                                    .or_insert_with(|| ImplicitLocalInfo {
                                        name: name.clone(),
                                        first_assignment: target_id,
                                        ty: value_ty,
                                        assignments: vec![ImplicitLocalAssignment {
                                            target: target_id,
                                            ty: value_ty,
                                        }],
                                    });
                                if !self.is_unknown(value_ty) {
                                    self.var_types.insert(key, value_ty);
                                }
                            }
                        }
                    }
                    Expr::Field { base, field } => {
                        // Read-only-property gate. We resolve the receiver
                        // type (already cached by the `infer_expr(target)`
                        // call below, but we need it *before* the assignment
                        // has a chance to misresolve anything) through
                        // `field_lookup`, which is the same adapter hover
                        // and completion consult. A hit with
                        // `is_readonly = true` fires
                        // `ReadOnlyPropertyAssignment`; a miss is simply
                        // "we don't know what this is" and stays silent
                        // (the companion `UnresolvedField` diagnostic
                        // covers the Authoritative-receiver case).
                        let base_ty = self.infer_expr(ExprId::from_idx(*base));
                        let configs = self.db.configurations(self.file_id);
                        let resolver = self.get_resolver();
                        let base_ty_for_form = typeid_to_ty(self.db, base_ty);
                        let info = crate::form_items::lookup_form_item_field(
                            self.db,
                            &resolver,
                            &base_ty_for_form,
                            field,
                        )
                        .or_else(|| {
                            crate::field_lookup::lookup_field(self.db, &configs, base_ty, field)
                        });
                        if let Some(info) = info {
                            if info.is_readonly {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                        lhs: ExprId::from_idx(*target),
                                        receiver_ty: base_ty,
                                        field_name: field.clone(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }

                self.infer_expr(ExprId::from_idx(*target));
            }

            Stmt::Expr(expr_idx) => {
                self.infer_expr(ExprId::from_idx(*expr_idx));
            }

            Stmt::If(if_stmt) => {
                self.infer_expr(ExprId::from_idx(if_stmt.condition));
                self.infer_stmts(&if_stmt.then_branch);
                for (cond, branch) in if_stmt.elsif_branches.iter() {
                    self.infer_expr(ExprId::from_idx(*cond));
                    self.infer_stmts(branch);
                }
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.infer_stmts(else_branch);
                }
            }

            Stmt::PreprocIf(preproc) => {
                self.infer_stmts(&preproc.then_branch);
                for (_, _, branch) in preproc.elsif_branches.iter() {
                    self.infer_stmts(branch);
                }
                if let Some(else_branch) = &preproc.else_branch {
                    self.infer_stmts(else_branch);
                }
            }

            Stmt::While { condition, body } => {
                self.infer_expr(ExprId::from_idx(*condition));
                self.infer_stmts(body);
            }

            Stmt::For { var, from, to, body } => {
                self.infer_expr(ExprId::from_idx(*from));
                self.infer_expr(ExprId::from_idx(*to));
                // BSL semantics: `Для I = … По … Цикл` always binds `I`
                // to `Number`. The loop boundaries themselves are not
                // narrowed — runtime errors on non-Number `from`/`to` —
                // so we mirror the constant-binding shape and pin
                // `Ty::Number` on the loop variable for hover/goto and
                // body inference. ALWAYS overwrite, mirroring the
                // ForEach arm: locals are procedure-scoped and the
                // counter must shadow any prior binding for the
                // duration of the loop and the trailing tail.
                let var_name = self.body.binding_idx(*var).name.as_str().to_lowercase();
                let number = self.db.number(None, None);
                self.var_types.insert(var_name, number);
                // Per-binding pin for declaration-site hover. The
                // `BindingId` is fresh for each `Для I = … По …`, so
                // hover on a specific `I` declaration cannot pick up
                // another loop's stored type even when the lowercase
                // name collides.
                self.binding_types.insert(BindingId::from_idx(*var), number);
                self.infer_stmts(body);
            }

            Stmt::ForEach { var, collection, body } => {
                let coll_ty = self.infer_expr(ExprId::from_idx(*collection));
                if let Some(elem_ty) =
                    crate::iteration_lookup::resolve_iter_element_ty(self.db, coll_ty)
                {
                    // BSL semantics: `Для каждого X Из Y Цикл` rebinds
                    // X to elements of Y for the duration of the body
                    // and leaves it as the last yielded element (or
                    // `Undefined` for an empty collection) afterwards.
                    // Locals are procedure-scoped, so any prior
                    // assignment to the same name is shadowed by the
                    // loop. We honour that by ALWAYS overwriting
                    // `var_types` — including the `Ty::Unknown` case
                    // for `Произвольный` element types — so a stale
                    // prior binding cannot leak into the loop body or
                    // the trailing tail.
                    let var_name = self.body.binding_idx(*var).name.as_str().to_lowercase();
                    self.var_types.insert(var_name, elem_ty);
                    // Per-binding pin: each `Для каждого X` allocates a
                    // fresh `BindingId`, so two loops with the same
                    // lowercase name (or one shadowing a prior `Перем
                    // X`) keep distinct entries here. Hover at the
                    // declaration site routes through this map and
                    // therefore returns the type of the *specific*
                    // declaration, never another binding's type.
                    self.binding_types.insert(BindingId::from_idx(*var), elem_ty);
                }
                self.infer_stmts(body);
            }

            Stmt::Try { body, except } => {
                self.infer_stmts(body);
                self.infer_stmts(except);
            }

            Stmt::Return { value } => {
                if let Some(expr_idx) = value {
                    let expr_id = ExprId::from_idx(*expr_idx);
                    self.infer_expr(expr_id);
                    // Phase O.10: record return-expr id for the
                    // per-method cascade query. `infer_query` walks
                    // every body unconditionally and discards this
                    // field; `method_return_type_query` consumes it
                    // from `BodyInferenceResult.return_expr_ids` to
                    // compute the unioned return type without
                    // re-walking the body.
                    self.return_expr_ids.push(expr_id);
                }
            }

            Stmt::Raise { value } => {
                if let Some(expr_idx) = value {
                    self.infer_expr(ExprId::from_idx(*expr_idx));
                }
            }

            Stmt::Execute { expr } => {
                self.infer_expr(ExprId::from_idx(*expr));
            }

            Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
                self.infer_expr(ExprId::from_idx(*event));
                self.infer_expr(ExprId::from_idx(*handler));
            }

            Stmt::VarDecl { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::Goto(_)
            | Stmt::Label(_) => {}
        }
    }

    /// Project every sub-query of `Новый Запрос("<literal>")`'s
    /// string-literal arg through the SDBL ↔ Ty bridge.
    ///
    /// Returns a non-empty index-aligned slice when `args[0]` is a
    /// string literal whose `SdblExprId` resolves to an `SdblPackage`:
    /// `result[i]` is the projection of the i-th sub-query, `Some`
    /// when the bridge resolved it, `None` for unresolvable ones
    /// (asterisk against an unresolved table, parse errors, etc.).
    /// Empty slice for no args / dynamic text / unrecognised literals
    /// — the caller then types as `Ty::Query{projections: empty}`,
    /// which downstream dispatch treats the same as the legacy
    /// `Ty::PlatformObject("Запрос")` shape.
    ///
    /// Carrying the full per-sub-query slice (not just `queries[0]`)
    /// is what lets `.ВыполнитьПакет()[i]` recover the i-th
    /// sub-query's projection without re-fetching the package at
    /// chain-rewrite time — the rewrite hook stays pure.
    ///
    /// The `matches!` guard is belt-and-suspenders — by construction
    /// every `ExprId` in `body.sdbl_exprs` is a string literal today,
    /// but the explicit check prevents a future lowerer change from
    /// quietly producing projections for non-literal expressions.
    fn try_synthesise_query_projections(
        &self,
        args: &[ExprIdx],
    ) -> Arc<[Option<Arc<SdblProjection>>]> {
        let Some(arg_idx) = args.first().copied() else {
            return Arc::from([]);
        };
        let arg_id = ExprId::from_idx(arg_idx);
        if !matches!(self.body.expr(arg_id), Expr::Literal(Literal::String(_))) {
            return Arc::from([]);
        }
        let sdbl_expr_id = SdblExprId { owner: self.owner, expr_id: arg_id };
        let file_id_input = FileIdInput::new(self.db, self.file_id);
        let entries = sdbl_hir_for_file_query(self.db, file_id_input);
        let Some((_, pkg)) = entries.iter().find(|(id, _)| *id == sdbl_expr_id) else {
            return Arc::from([]);
        };
        crate::sdbl_bridge::package_to_projections(self.db, pkg).into()
    }

    /// Infer the type of an expression.
    ///
    /// This is the core type inference function. It pattern-matches on the expression
    /// kind and dispatches to specialized inference functions.
    fn infer_expr(&mut self, expr_id: ExprId) -> TypeId {
        // Check if already inferred (avoid re-inference)
        if let Some(ty) = self.expr_types.get(&expr_id) {
            return *ty;
        }

        // Clone the expression to avoid borrow checker issues
        // (we need &mut self for recursive infer_expr calls)
        let expr = self.body.expr(expr_id).clone();
        trace!("inferring expr {:?}: {:?}", expr_id, expr);

        let ty = match &expr {
            Expr::Missing => self.db.unknown(),

            Expr::Literal(lit) => self.infer_literal(lit),

            Expr::Path(name) => self.infer_path_name(name, expr_id),

            Expr::QualifiedPath(_path) => {
                // Standalone qualified paths never reach this branch in
                // practice: HIR lowering (`body::lower::expr`) only produces
                // `Expr::QualifiedPath` when rewriting call syntax
                // `a.b()` / `a.b.c()` — the callee ends up in
                // `Expr::Call { callee: QualifiedPath, .. }` and the match
                // in `infer_call` takes over before this arm fires. For
                // non-call access (`Х = Документы.ПКО`) HIR emits
                // `Expr::Field { base, field }` instead.
                //
                // Leaving the arm as `Unknown` documents the contract: if
                // a future HIR pass ever lifts standalone 2-segment paths
                // into `QualifiedPath`, Ty resolution lives in the call
                // site already (`infer_two_segment_qualified_path`
                // analogue must be added here, gated on arity == 2).
                self.db.unknown()
            }

            Expr::BinaryOp { lhs, rhs, op } => {
                self.infer_binary_op(ExprId::from_idx(*lhs), ExprId::from_idx(*rhs), *op)
            }

            Expr::UnaryOp { expr, op } => self.infer_unary_op(ExprId::from_idx(*expr), *op),

            Expr::Ternary { condition, then_expr, else_expr } => {
                // Infer all branches
                self.infer_expr(ExprId::from_idx(*condition));
                let then_ty = self.infer_expr(ExprId::from_idx(*then_expr));
                let else_ty = self.infer_expr(ExprId::from_idx(*else_expr));

                // Unify types
                if then_ty == else_ty {
                    then_ty
                } else {
                    self.db.unknown()
                }
            }

            Expr::Call { callee, args } => {
                let converted_args: Vec<ExprId> =
                    args.iter().map(|&arg| ExprId::from_idx(arg)).collect();
                self.infer_call(ExprId::from_idx(*callee), &converted_args)
            }

            Expr::MethodCall { receiver, method, args } => {
                let receiver_ty = self.infer_expr(ExprId::from_idx(*receiver));
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                // `MethodLookup` is the single adapter that turns
                // `(receiver_ty, method_name)` into a return type. Covers
                // platform-value types, object managers, and metadata refs;
                // returns `None` for unions / collectives / unknown
                // receivers. When lookup fails, inference keeps the
                // previous "best effort" semantics by emitting
                // `Ty::Unknown` — chain continuation still typechecks
                // structurally, it just doesn't carry a concrete type.
                // Phase 3 §4.E.2b-ii: `lookup_method` is kernel-native
                // (`receiver: TypeId`, returns `MethodInfo`). Inference
                // still works in `Ty`, so bridge the receiver in and the
                // return id back out (transitional until infer flips).
                crate::method_lookup::lookup_method(self.db, receiver_ty, method)
                    .map(|info| info.return_ty)
                    .unwrap_or_else(|| self.db.unknown())
            }

            Expr::Index { base, index } => {
                let base_ty = self.infer_expr(ExprId::from_idx(*base));
                self.infer_expr(ExprId::from_idx(*index));

                // Parameterised arrays carry their element schema in the
                // type itself, so `arr[i]` resolves to the element Ty
                // directly. The motivating chain is the form-control row
                // path: `Элементы.Переприемка.ВыделенныеСтроки[i]` —
                // the receiver is `TypedArray(row)` (Phase 5 refined
                // property + Phase 0 parameterised-array variant), so
                // indexing surfaces the row's tabular-section schema and
                // downstream `.ШтрихКод` access typechecks the same way
                // it does for the loop-variable spelling.
                //
                // All other receivers — including the legacy
                // `Ty::Array` (no element schema) and platform value
                // collections (`СписокЗначений`, `ТаблицаЗначений`) —
                // keep returning `Unknown`. Those carry their element
                // type only via the `Элементы коллекции:` chapter
                // surfaced by `iteration_lookup`, which is a different
                // axis from indexing and is not parameterised at the
                // `Ty` level today.
                let base_kind = self.db.lookup_type(base_ty);
                match base_kind {
                    TypeKind::Array(facet) => facet.element.unwrap_or_else(|| self.db.unknown()),
                    // `Зап.ВыполнитьПакет()[i]` — when the index is a
                    // bare numeric literal, recover the i-th sub-query's
                    // projection from the batch result's `per_query`
                    // slice. Dynamic / arithmetic / out-of-range indices
                    // degrade to `Ty::QueryResult{None}`, matching the
                    // platform behaviour of "an in-bounds query result,
                    // schema unknown".
                    TypeKind::QueryBatchResult { per_query } => {
                        let index_expr = self.body.expr(ExprId::from_idx(*index));
                        let projection = const_eval_literal_index(index_expr)
                            .and_then(|i| per_query.get(i).cloned())
                            .flatten();
                        self.db.query_result(projection, ProjectionSource::Unknown)
                    }
                    _ => self.db.unknown(),
                }
            }

            Expr::Field { base, field } => {
                let base_ty = self.infer_expr(ExprId::from_idx(*base));

                // Two adapters participate. `FieldLookup` resolves MDO
                // attributes / tabular sections / register parts on
                // `Ty::MetadataRef` receivers. `ManagerLookup` resolves
                // manager-global members — promoting
                // `ManagerCollection(kind).<MdoName>` to
                // `ObjectManager { kind, MdoName }` when the MDO exists
                // in `Configuration`, then resolving predefined items /
                // enum values on the `ObjectManager` side in the next
                // `Expr::Field` hop. The two adapters cover disjoint
                // receiver shapes, so the order in which they are tried
                // only matters for readability — try the field-table
                // first (the common path) and fall through to the
                // manager surface.
                //
                // Pulling `configurations(file_id)` inside this branch
                // keeps the Salsa dependency fine-grained — invalidating
                // one configuration XML re-runs inference exactly for
                // the bodies that observed it.
                let configs = self.db.configurations(self.file_id);
                let resolver = self.get_resolver();
                let base_ty_for_form = typeid_to_ty(self.db, base_ty);
                if let Some(info) = crate::form_items::lookup_form_item_field(
                    self.db,
                    &resolver,
                    &base_ty_for_form,
                    field,
                ) {
                    info.ty
                } else if let Some(info) =
                    crate::field_lookup::lookup_field(self.db, &configs, base_ty, field)
                {
                    info.ty
                } else if let Some(info) =
                    crate::manager_lookup::lookup_manager_field(self.db, &configs, base_ty, field)
                {
                    info.ty
                } else {
                    // Only emit on receivers where `lookup_field` is
                    // actually authoritative — today that is
                    // [`Ty::MetadataRef`] and [`Ty::ThisObject`].
                    // `ThisObject` is coerced to the matching
                    // `*Object` `MetadataRef` at `lookup_field`'s
                    // entry (see `crate::this_object`), so a miss on
                    // it is as conclusive as a miss on the explicit
                    // object reference — the catalog's attribute
                    // list was checked and the field genuinely isn't
                    // there. Primitives, `Function`,
                    // `ManagerCollection`, and `ObjectManager` all
                    // fall through the adapter to `None` because
                    // their field tables live elsewhere (predefined
                    // items via the M4 Task 3 adapter, registers via
                    // M4 Task 2) or simply do not exist. Treating
                    // those misses as "field does not exist on the
                    // receiver" would flood the IDE with false
                    // positives on perfectly legal code like
                    // `Число.ToString()`. `Unknown` and `Union` are
                    // excluded by the match — `MetadataRef` is never
                    // a union member by construction. `ManagerLookup`
                    // misses (typo'd MDO names, unknown predefined
                    // items) stay silent here; a follow-up can make
                    // them authoritative once the method surface on
                    // `ObjectManager` lands too.
                    let base_kind = self.db.lookup_type(base_ty);
                    if matches!(base_kind, TypeKind::MetadataRef(_) | TypeKind::ThisObject { .. }) {
                        // `Ty::ThisManager` is intentionally **not**
                        // authoritative here: it coerces to
                        // `Ty::ObjectManager`, and `lookup_field` /
                        // `enumerate_fields` do not yet resolve manager
                        // predefined-item / enum-value access through
                        // that channel. Until predefined-item resolution
                        // for `Ty::ObjectManager` lands (separate slice
                        // — see `field_lookup.rs` doc-block above the
                        // coercion call), promoting `ThisManager` to
                        // authoritative would emit spurious
                        // `UnresolvedField` for valid
                        // `ЭтотОбъект.<PredefinedName>` /
                        // `ЭтотОбъект.<EnumValue>` access inside a
                        // ManagerModule. This mirrors the equally
                        // non-authoritative posture for
                        // `Справочники.<MDO>.<NotAField>` — the same
                        // shape after qualified-manager indexing.
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedField {
                            expr: expr_id,
                            receiver_ty: base_ty,
                            field_name: field.clone(),
                        });
                    }
                    self.db.unknown()
                }
            }

            Expr::New { type_name, args } => {
                // Infer arguments
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                // Constructor arity check — multi-overload "accept-if-any".
                //
                // Resolves the platform constructors for the bare type name
                // (case-insensitive, bilingual via `constructors_by_type`).
                // Each overload becomes a `BuiltinSignature` descriptor
                // through the same adapter that handles global functions
                // (arity only — `Новый` never type-checks args), so the
                // PR1/PR2/PR3 `is_variadic` precedence applies uniformly:
                // explicit JSON flag → `,...,<X>N` name idiom → `<X>N-<X>M`
                // capped name → fixed arity. Diagnostic fires only when
                // NO overload accepts the call's arity.
                //
                // Skipped paths (intentional):
                // - `args = []` (`Новый Массив` w/ or w/o parens) when any
                //   overload has `required = 0`. Covers the no-parens
                //   form for types that ship a zero-arity ctor (Array,
                //   Structure, Query, …).
                // - Empty constructor list (unresolved type, primitive
                //   `Новый Строка`, user CFE — none of which surface in
                //   `PLATFORM_CONSTRUCTORS`). Avoids double-firing on top
                //   of upstream "unresolved type" diagnostics.
                if let Some(name) = type_name {
                    let ctors =
                        bsl_platform::PlatformDataInner::instance().get_constructors(name.as_str());
                    if !ctors.is_empty() {
                        let arg_count = args.len();
                        // Constructor arity uses the db-free descriptor
                        // view only — `Новый` never type-checks args, so no
                        // kernel lowering is needed here.
                        let sigs: Vec<builtin::BuiltinSignature> = ctors
                            .iter()
                            .map(|ctor| {
                                builtin::descriptor_from_params(
                                    &ctor.parameters,
                                    builtin::ReturnTypeSpec::Unknown,
                                )
                            })
                            .collect();

                        let mut arity_match: Option<usize> = None;
                        let mut best_idx = 0usize;
                        let mut best_distance = usize::MAX;
                        for (idx, sig) in sigs.iter().enumerate() {
                            let required = sig
                                .defaults()
                                .iter()
                                .rposition(|has_default| !*has_default)
                                .map_or(0, |i| i + 1);
                            let too_few = arg_count < required;
                            let too_many = sig.max_args().is_some_and(|m| arg_count > m as usize);
                            if !too_few && !too_many {
                                arity_match = Some(idx);
                                break;
                            }
                            let upper = sig.max_args().map_or(arg_count, |m| m as usize);
                            let distance = if too_few {
                                required - arg_count
                            } else {
                                arg_count.saturating_sub(upper)
                            };
                            if distance < best_distance {
                                best_distance = distance;
                                best_idx = idx;
                            }
                        }

                        if arity_match.is_none() {
                            let sig = &sigs[best_idx];
                            let required = sig
                                .defaults()
                                .iter()
                                .rposition(|has_default| !*has_default)
                                .map_or(0, |i| i + 1);
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: expr_id,
                                    required_count: required,
                                    total_count: sig.param_count(),
                                    found: arg_count,
                                },
                            );
                        }
                    }
                }

                // `Новый Запрос(<text>)` always produces `Ty::Query{..}`
                // (bilingual `Новый Query` too) — never the legacy
                // `Ty::PlatformObject("Запрос")` shape. That stable
                // receiver lets chain rewrites (`.Выполнить()`,
                // `.ВыполнитьПакет()`) carry the projection forward
                // without enumerating two query-shapes. Legacy property
                // lookups (`Зап.Параметры`, `Зап.Текст`) still resolve
                // because `platform_type_key` keys `Ty::Query → "Запрос"`.
                let is_query_ctor = type_name.as_ref().is_some_and(|name| {
                    crate::method_lookup::is_platform_name(name, "Запрос", "Query")
                });
                if is_query_ctor {
                    let projections = self.try_synthesise_query_projections(args);
                    self.db.query(
                        projections
                            .iter()
                            .map(|p| p.as_ref().map(|p| sdbl_projection_to_projection(self.db, p)))
                            .collect(),
                    )
                } else {
                    // Lower the constructor name through the shared TypeRef →
                    // Ty adapter. The cascade (builtin → MDO plural → platform
                    // object fallback) moved into `lower_bare_name`, so every
                    // syntactic source of type info (`Новый X`, `Тип("…")`,
                    // JSDoc) now takes the same path — editing the fallback
                    // rules in one place is enough.
                    match type_name {
                        Some(name) => {
                            ty_to_typeid(self.db, &TyLoweringContext::new().lower_bare_name(name))
                        }
                        None => self.db.unknown(),
                    }
                }
            }

            Expr::Array(elements) => {
                // Infer element types
                for &elem in elements.iter() {
                    self.infer_expr(ExprId::from_idx(elem));
                }

                self.db.array(None)
            }

            Expr::Await { expr } => {
                // BSL Await returns the same type as the awaited expression
                self.infer_expr(ExprId::from_idx(*expr))
            }
        };

        // Store the inferred type
        self.expr_types.insert(expr_id, ty);
        ty
    }

    /// Resolve a bare `Expr::Path` identifier to a [`Ty`].
    ///
    /// Lookup order mirrors BSL visibility:
    ///
    /// 1. **Implicit locals** — BSL has no explicit `Var` declarations;
    ///    a name springs into existence at its first assignment. The
    ///    inference context captures those types in [`Self::var_types`]
    ///    as [`Stmt::Assign`] is walked in [`Self::infer_stmts`].
    ///    In value / receiver position, implicit locals shadow module-level,
    ///    manager, form, platform, and builtin names. Phase F upgrades a
    ///    projection-less `Ty::Query{[None]}` / `Ty::PlatformObject("Запрос")`
    ///    here via reaching-defs on `<name>.Текст` writes so bare-name
    ///    references (e.g. `Возврат Зап;` from a helper) inherit the
    ///    projection that Phase D used to compute only at chain dispatch.
    /// 2. **Declared locals / parameters / module symbols** — returned as
    ///    `Unknown` today when no type was inferred yet, but they still
    ///    shadow platform builtins in value position. Builtin functions are
    ///    resolved from `infer_call` only when the syntax is actually a call
    ///    (`Name(...)`), not when `Name` is a bare value.
    /// 3. **Module-level methods / variables** — returned as `Unknown`
    ///    today (no signature carrier yet); Task 2.x will synthesise
    ///    `Ty::Function` from `MethodId`.
    ///
    /// `expr_id` is the use-site ExprId. Phase F's dataflow refinement
    /// asks "what reaching definitions of `<name>.Текст` reach the
    /// enclosing statement of this expression?" — the answer depends on
    /// where the path is referenced (a `Возврат Зап;` mid-procedure
    /// sees different reaching defs than a `Зап.Выполнить()` later).
    fn infer_path_name(&mut self, name: &hir_def::Name, expr_id: ExprId) -> TypeId {
        use hir_def::resolver::Resolution;

        let resolver = self.get_resolver();

        // 0. `ЭтотОбъект` / `ThisObject` — intercepted ahead of every
        //    other scope because BSL treats the identifier like a
        //    platform global: not shadowable, resolved through module
        //    metadata (the enclosing MDO) rather than the scope chain.
        //    Module kind dictates the receiver shape:
        //      - ObjectModule with an `*Object` companion → `Ty::ThisObject`
        //      - ManagerModule with a manager prefix → `Ty::ThisManager`
        //      - RecordSetModule for a register flavour → `Ty::MetadataRef{*RecordSet}`
        //      - Managed form → `Ty::PlatformObject(ФормаКлиентскогоПриложения)`
        //    Common / command modules and other unsupported kinds fall
        //    through to the normal cascade and become `Ty::Unknown`.
        let name_lower = name.as_str().to_lowercase();
        if name_lower == "этотобъект" || name_lower == "thisobject" {
            if let Some(owner) = crate::this_object::resolve_this_object_owner(self.db, &resolver) {
                trace!("resolved {} as ThisObject {{ owner: {:?} }}", name, owner);
                return self.db.mk_this_object(
                    ConfigId::Root,
                    MdoRefFacet::new(owner.0, owner.1.as_str().to_string()),
                );
            }
            // Manager-module: same identifier, different receiver shape.
            // `ЭтотОбъект` inside `<MDO>/Ext/ManagerModule.bsl` names the
            // manager itself (`Справочники.Номенклатура`). The adapter
            // chain then coerces `Ty::ThisManager` to
            // `Ty::ObjectManager` — see Step J in plan
            // `linear-tumbling-noodle.md` §2.5.
            if let Some(owner) = crate::this_object::resolve_this_manager_owner(self.db, &resolver)
            {
                trace!("resolved {} as ThisManager {{ owner: {:?} }}", name, owner);
                return self.db.mk_this_manager(
                    ConfigId::Root,
                    MdoRefFacet::new(owner.0, owner.1.as_str().to_string()),
                );
            }
            // Record-set module: `ЭтотОбъект` is the record set itself
            // (`MetadataRef{*RecordSet, name}`). One-to-one mapping per
            // register flavour (4 MdoType -> 4 *RecordSet kind), so we emit
            // the `MetadataRef` directly rather than going through a wrapper
            // variant — `*RecordSet` has no sibling kind to disambiguate the
            // way `Ty::ThisObject` disambiguates *Object from *Ref for
            // catalog/document/etc. modules.
            if let Some((mdo, name)) =
                crate::this_object::resolve_this_record_set_owner(self.db, &resolver)
            {
                if let Some(kind) = hir_def::ty::MetadataKind::record_set_kind_for(mdo) {
                    trace!(
                        "resolved {} as record-set MetadataRef {{ {:?}, {} }}",
                        name.as_str(),
                        kind,
                        name
                    );
                    return self.db.metadata_ref(kind, name.as_str().to_string(), &RootConfigCtx);
                }
            }
            // Managed-form fallback: `ЭтотОбъект` in a managed form module
            // names the form itself. There is no `MdoType` companion (forms
            // are outside the catalog/document/exchange-plan/CoA axis), so
            // we don't go through `Ty::ThisObject` — we hand back the
            // platform type directly. Subsequent `.Элементы` / `.Найти(…)`
            // chains then route through the existing platform-property /
            // platform-method adapters with no special-case code.
            if crate::this_object::is_managed_form_module(self.db, &resolver) {
                trace!("resolved {} as managed form Self", name);
                return self.db.platform_object(crate::form_self::FORM_TYPE_NAME.to_string());
            }
            // Fall through: record-set / ordinary-form / common-module
            // `ЭтотОбъект` stays Unknown.
        }

        let resolved = resolver.resolve_name(self.db, name);

        // 1. BSL implicit locals shadow every global-ish name in value /
        //    receiver position. Builtins are handled in `infer_call` only
        //    when the syntax is a call (`Name(...)`); a bare path `Name`
        //    cannot be a function value in BSL.
        if let Some(ty) = self.var_types.get(&name.as_str().to_lowercase()) {
            trace!("resolved {} via var_types = {:?}", name, ty);
            // Phase F — projection-less Query bindings get upgraded
            // via reaching-defs on `<name>.Текст` writes so bare-name
            // references (e.g. `Возврат Зап;` from a helper body)
            // carry the projection forward through the cross-method
            // return-type cascade. The eligibility gate matches
            // chain-dispatch Phase D so a path resolved here will
            // produce a receiver that `apply_sdbl_chain_rewrite` then
            // skips (already-projected receivers short-circuit).
            let ty_id = *ty;
            let ty_for_refinement = typeid_to_ty(self.db, ty_id);
            if crate::method_lookup::receiver_needs_refinement(&ty_for_refinement) {
                if let Some(projections) = crate::query_text_dataflow::refine_query_at_use_site(
                    self.db,
                    self.file_id,
                    self.owner,
                    expr_id,
                    name,
                    &self.body,
                ) {
                    let refined = self.db.query(
                        projections
                            .iter()
                            .map(|p| p.as_ref().map(|p| sdbl_projection_to_projection(self.db, p)))
                            .collect(),
                    );
                    trace!("Phase F refined {} to {:?}", name, refined);
                    return refined;
                }
            }
            return ty_id;
        }

        let user_shadows =
            matches!(resolved, Some(Resolution::Method(_)) | Some(Resolution::Variable(_)));
        let body_binding_shadows = self.body_declares_binding(name);

        // 2. Declared-but-untyped locals / parameters / module symbols still
        //    shadow platform builtins and globals in value position. We may
        //    not know their type yet, but resolving them as a builtin would
        //    be worse: e.g. `Строка = ...; Строка.Поле` must use the local
        //    receiver, while the builtin `Строка(...)` remains available in
        //    call position through `infer_call`.
        if user_shadows || body_binding_shadows {
            return self.db.unknown();
        }

        // 3. Builtins — union of Resolver's platform-global view and the
        //    narrower hir-ty signature table. At value position we collapse
        //    a builtin hit to `Ty::Unknown`; BSL has no first-class function
        //    values, and the typed function signature path starts from
        //    `infer_call`'s `Expr::Path` callee branch.
        let resolver_says_builtin = matches!(resolved, Some(Resolution::Builtin(_)));
        let hir_sig = builtin::builtin_functions().get(name.as_str());
        if resolver_says_builtin || hir_sig.is_some() {
            return self.db.unknown();
        }

        // 4. MDO plural globals (`Документы`, `Справочники`, …) lower into
        //    `Ty::ManagerCollection(MdoType)`. This is the single path a
        //    plural form takes when no local variable shadows it; consumers
        //    (hover / completion) observe the collective type and can
        //    eventually chain `.Name` into `Ty::ObjectManager` once HIR
        //    lifts standalone 2-segment paths into `Expr::QualifiedPath`.
        //    Note: `Справочники`, `Документы`, `Перечисления`, `РегистрыСведений`
        //    and other MDO plurals also appear in HBK as global-context
        //    properties (typed as `СправочникиМенеджер` etc.), so this step
        //    intentionally runs BEFORE the platform-global cascade below —
        //    otherwise three-level chains like `Справочники.Контрагенты.X`
        //    would lose their `Ty::ManagerCollection` shape and the existing
        //    `resolve_three_level_call` machinery would no longer see them.
        if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(name.as_str()) {
            if Ty::manager_collection(mdo_type).is_some() {
                trace!("resolved {} as manager collection {:?}", name, mdo_type);
                return self.db.manager_collection(mdo_type);
            }
        }

        // 5. Managed-form Self property — inside a managed-form module,
        //    bare `Элементы`, `Команды`, `Параметры`, `Заголовок`, … are
        //    properties of `ФормаКлиентскогоПриложения`. The form-self
        //    helper does a cheap platform-data lookup first, so non-form
        //    modules pay only one `FxHashMap` probe per unresolved name
        //    before bailing.
        //
        //    Ordering: BEFORE platform globals so a property name that
        //    coincidentally collides with a platform global resolves with
        //    the form's perspective. AFTER MDO plurals so `Документы` /
        //    `Справочники` continue to lower as `Ty::ManagerCollection`
        //    (no current ClientApplicationForm property collides with an
        //    MDO plural; a regression test in `tests/form_self.rs` pins
        //    that invariant).
        //
        //    Extra shadowing gate: a parameter or `Перем` local with this
        //    name must shadow the form-self property. `var_types` only
        //    captures *assigned* implicit locals; an unassigned parameter
        //    or `Перем X;` without a prior write is invisible to it.
        //    `body_declares_binding` plugs that gap.
        if !user_shadows && !body_binding_shadows {
            if let Some(resolution) =
                crate::form_self::resolve_form_self_property(self.db, &resolver, name)
            {
                trace!("resolved {} as managed-form Self property", name);
                return ty_to_typeid(self.db, &resolution.return_ty);
            }
        }

        // 5b. Managed-form attribute — user-declared реквизиты of the
        //     enclosing form (`<Attributes><Attribute name="…">` in
        //     Form.xml). Same shadowing gate as form-self property:
        //     module-level methods, `Перем` declarations, parameters and
        //     assigned implicit locals win over a same-named attribute.
        //
        //     Order matters:
        //     - AFTER form-self property (4) so platform members
        //       `Элементы` / `Команды` / `Параметры` keep their wrapper
        //       type even if the configuration declares an attribute with
        //       the same name (impossible in well-formed configurations,
        //       but the cascade choice is deliberate).
        //     - BEFORE platform globals (5) so user-defined attributes
        //       like `Метаданные`, `ОбработкаОшибок` win over the
        //       global-context property of the same name when typed in a
        //       form module.
        //     - AFTER MDO plurals (3) — names like `Документы` resolve as
        //       `Ty::ManagerCollection(Document)` regardless of whether
        //       the form happens to declare an attribute with that name.
        //
        //     Cheap-first probe: `resolve_form_attribute` opens with the
        //     same `is_managed_form_module` gate `form_self` uses, so non-form
        //     modules pay nothing.
        if !user_shadows && !body_binding_shadows {
            if let Some(ty) = crate::form_attr::resolve_form_attribute(self.db, &resolver, name) {
                trace!("resolved {} as managed-form attribute", name);
                return ty_to_typeid(self.db, &ty);
            }
        }

        let workspace_owns_common_module = resolver.user_common_module_exists(self.db, name);

        // 5c. ObjectModule implicit ЭтотОбъект.<name> — bare attribute,
        //     standard attribute, or tabular section of the owning MDO.
        //     Symmetric to 5b (form attribute) but gated on
        //     this_object::resolve_this_object_owner, which is None outside an
        //     ObjectModule of an MDO with an *Object companion
        //     (MetadataKind::object_kind_for). Extra workspace_owns_common_module
        //     guard so a user CommonModule with the same name wins (mirrors
        //     call-form precedence in dispatch_bare_ident_field_call).
        if !user_shadows && !body_binding_shadows && !workspace_owns_common_module {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_object_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit ЭтотОбъект.{} member", name, name);
                return ty_to_typeid(self.db, &ty);
            }
        }

        // 5d. RecordSetModule implicit ЭтотОбъект.<name> — bare reference
        //     to a platform property (`ДополнительныеСвойства`, `Отбор`,
        //     `ОбменДанными`, …) or a register dimension/resource/attribute.
        //     User methods are handled earlier as `Ty::Unknown` via the
        //     `user_shadows` / `body_binding_shadows` gates (see step 2),
        //     so we only reach here for field-shaped lookups.
        //     Symmetric to 5c (ObjectModule) but gated on
        //     this_object::resolve_this_record_set_owner, which is None outside a
        //     RecordSetModule of one of the four register flavours.
        if !user_shadows && !body_binding_shadows && !workspace_owns_common_module {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_record_set_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit record-set ЭтотОбъект.{} member", name, name);
                return ty_to_typeid(self.db, &ty);
            }
        }

        // 6. Platform global-context properties — top-level identifiers
        //    declared on `Global context` in HBK whose declared type is the
        //    foreign key into the platform type/method catalogue
        //    (`ОбработкаОшибок: МенеджерОбработкиОшибок`,
        //    `БиблиотекаКартинок: БиблиотекаКартинокМенеджер`,
        //    `Метаданные: КонфигурацияМетаданныеОбъект`, …). Inferring the
        //    bare identifier to its declared type plugs straight into the
        //    existing dot-call lookup (`platform_property_lookup`,
        //    `method_lookup`) — no new infrastructure needed.
        //
        //    Order matters: this comes AFTER the MDO plural step so names
        //    that are both global properties and MDO plurals (`Справочники`,
        //    `Документы`, `Перечисления`, `РегистрыСведений`, …) keep their
        //    `Ty::ManagerCollection` shape and feed the existing
        //    `resolve_three_level_call` machinery for `Справочники.X.Method()`
        //    chains. Only non-MDO globals (`ОбработкаОшибок`,
        //    `БиблиотекаКартинок`, `WSСсылки`, …) reach this step.
        // Narrowing: skip the platform fallback when the resolver already
        // sees the name as a module-level method or variable, or when the
        // workspace owns it as a CommonModule. The user has shadowed the
        // platform global (e.g. `Процедура ОбработкаОшибок() Экспорт` or
        // a `CommonModules/Метаданные/` user module); BSL semantics give
        // the local / workspace definition priority and we must not
        // silently retype a reference to it as `PlatformObject`.
        // Mirrors the cascade-gate ordering in
        // `dispatch_bare_ident_field_call` (gate 3 user-CM precedes gate
        // 4 platform) so both the bare-IDENT and the typed-receiver
        // paths agree on user-shadows-platform.
        if !user_shadows && !workspace_owns_common_module {
            // Phase 3 §4.G.5b: the helper is kernel-native now; bridge the
            // id back into this still-`Ty` inference path (Phase 4 removes it).
            if let Some(id) =
                crate::platform_global_lookup::resolve_platform_global_property_type(self.db, name)
            {
                trace!("resolved {} as platform global → {:?}", name, id);
                return id;
            }
        }

        // 7. Module-level methods / variables (Unknown today; Task 2.x
        //    will synthesise Ty::Function from MethodId).
        match resolved {
            Some(Resolution::Method(_)) | Some(Resolution::Variable(_)) => self.db.unknown(),
            // `Local` is unreachable here because `get_resolver` does not
            // push an ExprScope; any local-looking name already returned
            // from the `var_types` branch above.
            Some(Resolution::Builtin(_)) | Some(Resolution::Local(_)) | None => self.db.unknown(),
        }
    }

    /// Infer type from a literal.
    fn infer_literal(&self, lit: &Literal) -> TypeId {
        match lit {
            Literal::Number(_) => self.db.number(None, None),
            Literal::String(_) => self.db.string(None, false),
            Literal::Date(_) => self.db.date(DateComponent::DateTime),
            Literal::Bool(_) => self.db.boolean(),
            Literal::Undefined => self.db.undefined(),
            Literal::Null => self.db.null(),
        }
    }

    /// Infer type from a binary operation.
    fn infer_binary_op(&mut self, lhs: ExprId, rhs: ExprId, op: BinaryOp) -> TypeId {
        let lhs_ty = self.infer_expr(lhs);
        let rhs_ty = self.infer_expr(rhs);

        match op {
            // Arithmetic operations: Number op Number → Number
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                // Special case: String + Any → String (concatenation)
                let lhs_kind = self.db.lookup_type(lhs_ty);
                let rhs_kind = self.db.lookup_type(rhs_ty);
                if op == BinaryOp::Add
                    && (matches!(lhs_kind, TypeKind::String(_))
                        || matches!(rhs_kind, TypeKind::String(_)))
                {
                    self.db.string(None, false)
                } else if matches!(lhs_kind, TypeKind::Number(_))
                    && matches!(rhs_kind, TypeKind::Number(_))
                {
                    self.db.number(None, None)
                } else {
                    // Unknown operand types
                    self.db.unknown()
                }
            }

            // Comparison operations: Any op Any → Boolean
            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => self.db.boolean(),

            // Logical operations: Boolean op Boolean → Boolean
            BinaryOp::And | BinaryOp::Or => self.db.boolean(),
        }
    }

    /// Infer type from a unary operation.
    fn infer_unary_op(&mut self, expr: ExprId, op: UnaryOp) -> TypeId {
        let expr_ty = self.infer_expr(expr);

        match op {
            UnaryOp::Neg | UnaryOp::Plus => {
                // Numeric negation/plus
                let kind = self.db.lookup_type(expr_ty);
                if matches!(kind, TypeKind::Number(_)) {
                    self.db.number(None, None)
                } else {
                    self.db.unknown()
                }
            }
            UnaryOp::Not => {
                // Logical NOT
                self.db.boolean()
            }
        }
    }

    /// Infer type from a function call.
    fn infer_call(&mut self, callee: ExprId, args: &[ExprId]) -> TypeId {
        // Qualified callees come from body lowering only for
        // three-segment manager calls (`Документы.ПКО.Метод()`).
        // Two-segment `Module.Method()` was lifted out of `QualifiedPath`
        // in Phase 2 of the qualified-call refactor — those calls now
        // travel as `Expr::Call { callee: Expr::Field, … }` and dispatch
        // through `dispatch_bare_ident_field_call` in the Field branch
        // below. The form-self disambiguation that used to live here
        // moved to `infer_path_name`'s step 4 (no QualifiedPath shape
        // remains for it to gate).
        let callee_expr = self.body.expr(callee);
        if let Expr::QualifiedPath(qualified_path) = callee_expr {
            match qualified_path.segments().len() {
                3 => {
                    let mdo_type_plural = qualified_path.segments()[0].clone();
                    let mdo_name = qualified_path.segments()[1].clone();
                    let method_name = qualified_path.segments()[2].clone();
                    return self.infer_three_level_call(
                        &mdo_type_plural,
                        &mdo_name,
                        &method_name,
                        args,
                        callee,
                    );
                }
                other => {
                    // Invariant: only 3-segment QualifiedPath is
                    // currently constructed by lowering. 2-segment
                    // moved to Field-shape in Phase 2; 0/1/4+ never
                    // existed. A `debug_assert!` traps a future
                    // regression in tests; in release the call is
                    // demoted to `Ty::Unknown` (silent skip) rather
                    // than panicking inside the IDE process.
                    debug_assert!(
                        false,
                        "unexpected QualifiedPath segment count {other} in infer_call",
                    );
                    tracing::debug!(
                        segments = other,
                        ?qualified_path,
                        "QualifiedPath with unexpected segment count reached infer_call; \
                         falling through to Ty::Unknown"
                    );
                }
            }
        }

        // Method-call shape: `receiver.method(...)` lowers to
        // `Expr::Call { callee: Expr::Field { base, field } }` because
        // HIR never emits the dedicated `Expr::MethodCall` variant today
        // (dead-code branch, kept for future use). Route through
        // `method_lookup::lookup_method` with the receiver's inferred
        // type — otherwise `infer_expr(callee)` would go to the
        // `Expr::Field` branch, fail `lookup_field` (methods aren't
        // fields), and hand `infer_call` a `Ty::Unknown` callee, which
        // kills fluent chains like `Запрос.Выполнить().Выбрать()`.
        if let Expr::Field { base, field } = callee_expr {
            let base_id = ExprId::from_idx(*base);
            let method_name = field.clone();
            let receiver_ty = self.infer_expr(base_id);

            // Bare-IDENT cascade gate: when the receiver is a plain
            // `Expr::Path(name)` whose `infer_path_name` produced
            // `Ty::Unknown` — i.e. the name is neither a typed
            // binding nor a managed-form Self property, nor any
            // platform global — `dispatch_bare_ident_field_call`
            // takes over. It owns the resolution decision lowering
            // used to make from syntax alone (`M.Method()` ⇒
            // CommonModule), now done with the resolver and the
            // receiver type in hand. See
            // `dispatch_bare_ident_field_call` for the 5-gate
            // rationale (resolve_name → body_declares_binding →
            // user_common_module_exists → platform global →
            // ReceiverNotResolved).
            //
            // The gate is intentionally narrow to `Ty::Unknown` —
            // typed receivers (including `Ty::PlatformObject`) keep
            // going through the existing workspace + `lookup_method`
            // path below. `PlatformObject` reached from a typed
            // local (e.g. `Зап = Новый Запрос; Зап.Выполнить()`)
            // would otherwise tumble into gate 5's
            // `ReceiverNotResolved` because `Зап` is an implicit
            // local invisible to `Resolver::resolve_name`. The
            // `PlatformObject`-shaped miss case (e.g.
            // `ОбработкаОшибок.НеизвестныйМетод()`) is covered
            // separately at the `lookup_method` miss site below
            // (look for `try_resolve_platform_global_member` next to
            // `receiver_display_name`).
            if self.is_unknown(receiver_ty) {
                let base_expr = self.body.expr(base_id).clone();
                if let Expr::Path(path_name) = base_expr {
                    if let Some(return_ty) =
                        self.dispatch_bare_ident_field_call(&path_name, &method_name, args, callee)
                    {
                        self.expr_types.insert(callee, self.db.unknown());
                        return return_ty;
                    }
                }
            }

            for arg in args {
                self.infer_expr(*arg);
            }

            // Workspace-first lookup for `Ty::MetadataRef { *Object, .. }`
            // and `Ty::ThisObject` (which coerces to a matching
            // `*Object` `MetadataRef`). Phase B counterpart to the
            // `Ty::ObjectManager` branch below: methods declared in
            // `<MDO>/Ext/ObjectModule.bsl` are invisible to
            // `lookup_method` (platform-only via
            // `platform_manager_lookup::resolve_platform_metadata_ref_method`)
            // — without this branch every workspace object-module
            // method would slip through and surface a false-positive
            // `MethodNotFound`. Same precedence as Phase A: workspace
            // first, platform fallback on `MethodNotFound`,
            // authoritative miss otherwise.
            //
            // The strict `*Object` filter lives inside
            // `resolve_object_module_call`, so `*Ref` and register
            // kinds short-circuit to `MethodNotFound` and immediately
            // fall through to `lookup_method` (their platform path).
            //
            // `ThisObject` is coerced upfront — `lookup_method` does
            // the same coercion (`crate::this_object::coerce_to_metadata_ref`),
            // so doing it here mirrors that contract. After coercion
            // `ЭтотОбъект.МойМетод()` enters the workspace resolver
            // through the same `(MdoType, name)` pair the platform
            // path would see.
            let workspace_receiver_ty =
                crate::this_object::coerce_to_metadata_ref(&typeid_to_ty(self.db, receiver_ty))
                    .map(|ty| ty_to_typeid(self.db, &ty))
                    .unwrap_or(receiver_ty);
            let workspace_receiver_kind = self.db.lookup_type(workspace_receiver_ty);
            if let TypeKind::MetadataRef(facet) = workspace_receiver_kind {
                let kind = facet.kind;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_object_module_call(
                    self.db,
                    kind,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, workspace_receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
                        if args.len() < required || args.len() > total {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: callee,
                                    required_count: required,
                                    total_count: total,
                                    found: args.len(),
                                },
                            );
                        }
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {
                        // Workspace exhausted (or strict-filter reject
                        // for `*Ref`/register kinds) → fall through to
                        // platform.
                    }
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        unreachable!(
                            "resolve_object_module_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            // Workspace-first lookup for register-set receivers
            // (`Ty::MetadataRef { AccumulationRegisterRecordSet, .. }`).
            // Phase C counterpart to the Phase B *Object branch:
            // methods declared in
            // `<RegisterFolder>/<Name>/Ext/RecordSetModule.bsl` are
            // reachable via a record-set receiver per 1С semantics.
            //
            // The strict filter inside `resolve_record_set_module_call`
            // accepts ONLY `AccumulationRegisterRecordSet` —
            // `InformationRegisterRecordManager` is deliberately
            // excluded because that's a single-record handle, not a
            // record-set, and 1С rejects calls to
            // `RecordSetModule.bsl` exports through it. Other kinds
            // (`*Object`, `*Ref`, register parts) also short-circuit
            // to `MethodNotFound`, falling through to platform
            // `lookup_method` (which is now wired for register-record
            // composite typenames via `MetadataKind::platform_prefix`).
            //
            // Note: `ThisObject` cannot reach this branch because
            // `coerce_to_metadata_ref` only produces `*Object` kinds —
            // there is no `ThisObject → *RecordSet` coercion in BSL
            // semantics today. So we match on `receiver_ty` directly
            // here, not on `workspace_receiver_ty`.
            let receiver_kind = self.db.lookup_type(receiver_ty);
            if let TypeKind::MetadataRef(facet) = receiver_kind {
                let kind = facet.kind;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_record_set_module_call(
                    self.db,
                    kind,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
                        if args.len() < required || args.len() > total {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: callee,
                                    required_count: required,
                                    total_count: total,
                                    found: args.len(),
                                },
                            );
                        }
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {
                        // Strict-filter reject (non-register-record
                        // kinds) or workspace miss → fall through to
                        // platform `lookup_method`.
                    }
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        unreachable!(
                            "resolve_record_set_module_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            // Workspace-first lookup for `Ty::ObjectManager`. Methods
            // declared in `<MDO>/Ext/ManagerModule.bsl` are invisible to
            // `lookup_method` (platform-only via
            // `platform_manager_lookup::resolve_platform_manager_method`)
            // — so without this branch every workspace manager method
            // would slip through to the lookup_method-miss path and
            // surface a false-positive `MethodNotFound`. Mirrors the
            // 3-segment `infer_three_level_call` precedence:
            //   1. workspace `ManagerModule.bsl` (this branch).
            //   2. platform manager catalogue (`lookup_method` below).
            //   3. authoritative miss → `UnresolvedMethodCall`.
            // Only the `MethodNotFound` arm falls through — a non-
            // exported workspace method takes precedence over a same-
            // named platform method (a platform shadow on a real
            // workspace symbol would otherwise be silently chosen).
            //
            // `Ty::ThisManager` is coerced to `Ty::ObjectManager` here
            // so `ЭтотОбъект.МойМетодМенеджера()` enters the workspace
            // resolver via the same `(MdoType, name)` pair the platform
            // path would see — symmetric with Phase B's
            // `Ty::ThisObject` → `Ty::MetadataRef { *Object, .. }`
            // upfront-coercion just above.
            //
            // Bound at outer scope (not inside the `if let`) because
            // the value is consumed both by Phase A workspace lookup
            // and by the constant-arg-refinement check below. Pre-Step-J
            // both checked the original `receiver_ty`; without coercion
            // a future `Ty::ThisManager { kind: Constant, .. }` would
            // skip both refinement and Phase A.
            let manager_receiver_ty =
                crate::this_object::coerce_to_metadata_ref(&typeid_to_ty(self.db, receiver_ty))
                    .map(|ty| ty_to_typeid(self.db, &ty))
                    .unwrap_or(receiver_ty);
            let manager_receiver_kind = self.db.lookup_type(manager_receiver_ty);
            if let TypeKind::ObjectManager(facet) = manager_receiver_kind {
                let mdo_type = facet.mdo;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_aliased_manager_call(
                    self.db,
                    mdo_type,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
                        if args.len() < required || args.len() > total {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: callee,
                                    required_count: required,
                                    total_count: total,
                                    found: args.len(),
                                },
                            );
                        }
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {
                        // Workspace exhausted → fall through to platform.
                    }
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        // `resolve_aliased_manager_call` only maps
                        // `QualifiedMethodError::{NotFound, NotVisibleInConfigs}`
                        // → `MethodNotFound`; no other variant can reach this
                        // arm today. Listing the variants explicitly (instead
                        // of `Err(_)`) means a future kind added to either
                        // `QualifiedMethodError` or `UnresolvedMethodKind`
                        // without an explicit mapping won't silently fall
                        // through to the platform path — it will fail to
                        // compile here.
                        unreachable!(
                            "resolve_aliased_manager_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            // Phase D — variable-state refinement for SDBL chain
            // receivers. The ctx is passed by reference so non-SDBL
            // method lookups (the vast majority) bail out of the
            // `is_sdbl_chain_method` filter before touching it; only
            // `Запрос.Выполнить()` / `.Выбрать()` / `.ВыполнитьПакет()`
            // on a projection-less receiver consult the dataflow walk.
            let refine_ctx = crate::method_lookup::RefineCtx {
                db: self.db,
                file_id: self.file_id,
                owner: self.owner,
                body: &self.body,
                dispatch_expr_id: callee,
                receiver_expr_id: base_id,
                call_args: args,
            };
            let result = match crate::method_lookup::lookup_method_with_refinement(
                self.db,
                receiver_ty,
                &method_name,
                Some(&refine_ctx),
            ) {
                Some(info) => {
                    let mut return_ty = info.return_ty;
                    let mut params: Vec<TypeId> = info.params.to_vec();
                    let overloads: Vec<Arc<[TypeId]>> =
                        info.overloads.iter().cloned().map(Arc::from).collect();
                    // Argument type check (M4 Task 7 follow-up): the
                    // fluent-chain path historically skipped both arg-
                    // count and arg-type diagnostics. Emit the type
                    // check here — count-check stays deferred so this
                    // patch stays scoped to the TypeMismatch emitter.
                    // Use the post-coercion shape so a future
                    // `Ty::ThisManager { kind: Constant, .. }` (lands
                    // when ValueManagerModule support is added — see
                    // `this_object::resolve_this_manager_owner`) routes through
                    // the same refinement as the qualified-path
                    // `Константы.X.Имя()` shape.
                    let manager_receiver_kind = self.db.lookup_type(manager_receiver_ty);
                    if let TypeKind::ObjectManager(facet) = manager_receiver_kind {
                        if facet.mdo == bsl_metadata::MdoType::Constant {
                            let mdo_name = hir_def::Name::new(&facet.name);
                            self.refine_constant_method(
                                &mdo_name,
                                &method_name,
                                &mut return_ty,
                                &mut params,
                            );
                        }
                    }
                    self.record_call_arg_binding(
                        callee,
                        args,
                        ParamsShape::Overloaded {
                            flat: params.into(),
                            overloads: overloads.into(),
                        },
                    );
                    return_ty
                }
                None => {
                    // Authoritative-receiver gate. `MetadataRef` and
                    // `ThisObject` carry full type-system method tables
                    // through `lookup_method`, so a miss is conclusive.
                    // `ObjectManager` is **also** authoritative now,
                    // because the workspace branch above ran first —
                    // when execution reaches this `None`, both
                    // workspace `ManagerModule.bsl` (Phase A) and the
                    // platform manager catalogue have missed.
                    //
                    // Other receivers (`Unknown`, `Union`, primitives,
                    // `PlatformObject`, `ManagerCollection`, register
                    // kinds) stay silent: their method tables are
                    // partial or platform-data-only, so a miss is "we
                    // can't tell yet" rather than "method does not
                    // exist". `receiver_display_name` enforces this gate
                    // by returning `None` for such receivers.
                    if let Some(receiver_name) = receiver_display_name(self.db, receiver_ty) {
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                            expr: callee,
                            receiver_name,
                            method_name: method_name.clone(),
                            kind: UnresolvedMethodKind::MethodNotFound,
                        });
                    } else if matches!(
                        self.db.lookup_type(receiver_ty),
                        TypeKind::PlatformObject(_)
                    ) {
                        // PlatformObject bridge — replaces the Phase 1
                        // legacy `MissingCommonModuleMethod` coverage
                        // for `<Global>.<Method>` shapes where
                        // `<Global>` resolves through
                        // `infer_path_name`'s step 5 (e.g.
                        // `ОбработкаОшибок.НеизвестныйМетод()`).
                        // `receiver_display_name` keeps its general
                        // `None` for `PlatformObject` (the type tag
                        // alone is unsuitable as a user-facing receiver
                        // name; e.g. it would render `МенеджерОбработкиОшибок`,
                        // not `ОбработкаОшибок`). When the base is a
                        // bare `Expr::Path(name)` we have the original
                        // identifier — and the platform global tri-state
                        // tells us whether the receiver is a known
                        // container with a missing member (⇒
                        // `MethodNotFound`) or a typed value coming
                        // from somewhere else (⇒ stay silent — typed
                        // locals like `Зап = Новый Запрос; Зап.X()`
                        // get a `PlatformObject`-typed receiver too,
                        // and surfacing a diagnostic on those would be
                        // a regression the cascade gate carefully
                        // avoids).
                        let base_expr = self.body.expr(base_id).clone();
                        if let Expr::Path(receiver_path_name) = base_expr {
                            if matches!(
                                crate::platform_global_lookup::try_resolve_platform_global_member(
                                    &receiver_path_name,
                                    &method_name,
                                ),
                                crate::platform_global_lookup::PlatformGlobalLookup::KnownContainerMissingMember
                            ) {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::UnresolvedMethodCall {
                                        expr: callee,
                                        receiver_name: receiver_path_name,
                                        method_name: method_name.clone(),
                                        kind: UnresolvedMethodKind::MethodNotFound,
                                    },
                                );
                            }
                        }
                    }
                    self.db.unknown()
                }
            };
            // Cache the callee `Expr::Field`'s type so `infer_all`'s
            // second pass (which iterates every expression in the body)
            // does not re-visit it through the `Expr::Field` arm of
            // `infer_expr` and emit a spurious `UnresolvedField` on a
            // method name. BSL has no first-class method references —
            // the meaningful value type belongs to the surrounding
            // `Expr::Call`, which is cached at the call site.
            self.expr_types.insert(callee, self.db.unknown());
            return result;
        }

        // Bare-name callees (`ПустаяСтрока(СтрокаСоединения…)`,
        // `Сообщить(…)`, …) resolve through the hir-ty builtin
        // signature table directly. The bare-path arm of
        // `infer_path_name` no longer manufactures `Ty::Function`
        // values — BSL has no first-class function references — so a
        // typed arity / arg-type check on a builtin call has to start
        // from the callee `Expr::Path(name)` here, not from
        // `callee_ty`.
        if let Expr::Path(name) = callee_expr {
            let name = name.clone();
            if let Some(sigs) = builtin::builtin_functions().get(name.as_str()) {
                debug_assert!(
                    !sigs.is_empty(),
                    "BuiltinFunctions::get must never return an empty overload set"
                );

                // Lower the db-free descriptors to kernel signatures once
                // for this call site. (Body inference is Salsa-cached, so a
                // given builtin's strings are re-lowered at most once per
                // body revision.)
                let sigs: Vec<FunctionSignature> = sigs.iter().map(|s| s.lower(self.db)).collect();

                for arg in args {
                    self.infer_expr(*arg);
                }

                let arg_count = args.len();
                let inferred: Vec<TypeId> = args
                    .iter()
                    .map(|a| self.expr_types.get(a).copied().unwrap_or_else(|| self.db.unknown()))
                    .collect();

                // Two-stage overload selection — must consider BOTH arity
                // AND argument types. Stage 1 prefers an overload that
                // accepts on both axes (so `XYZ(string)` and `XYZ(num)`
                // overloads dispatch correctly when the caller's actual
                // arg type matches one of them). Stage 2 falls back to
                // the first arity-only match (covers callers with
                // `Ty::Unknown` arguments where types aren't yet refined).
                // Stage 3 picks the closest-by-distance overload purely
                // for the diagnostic message when nothing accepts.
                let mut full_match: Option<usize> = None;
                let mut arity_match: Option<usize> = None;
                let mut best_idx = 0usize;
                let mut best_distance = usize::MAX;
                for (idx, sig) in sigs.iter().enumerate() {
                    let required = sig.required_count();
                    let too_few = arg_count < required;
                    let too_many = sig.max_args.is_some_and(|m| arg_count > m as usize);
                    if !too_few && !too_many {
                        if arity_match.is_none() {
                            arity_match = Some(idx);
                        }
                        // Type check: zip arg types against this overload's
                        // params; `is_assignable` treats `Unknown` on either
                        // side as permissive.
                        let types_ok =
                            inferred.iter().zip(sig.params.iter()).all(|(actual, expected)| {
                                crate::subtype::is_assignable(self.db, *actual, *expected)
                            });
                        if types_ok && full_match.is_none() {
                            full_match = Some(idx);
                            break;
                        }
                    }
                    let upper = sig.max_args.map_or(arg_count, |m| m as usize);
                    let distance = if too_few {
                        required - arg_count
                    } else {
                        arg_count.saturating_sub(upper)
                    };
                    if distance < best_distance {
                        best_distance = distance;
                        best_idx = idx;
                    }
                }

                let chosen = match full_match.or(arity_match) {
                    Some(idx) => &sigs[idx],
                    None => {
                        let sig = &sigs[best_idx];
                        let required = sig.required_count();
                        self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                            call_expr: callee,
                            required_count: required,
                            total_count: sig.params.len(),
                            found: arg_count,
                        });
                        sig
                    }
                };

                // Argument-binding policy.
                //
                // We always record (do NOT short-circuit on a base-time
                // `full_match`). Reason: gradual typing makes
                // `is_assignable(Unknown, T) = true`, so a base-time
                // `full_match` is reachable for an arg whose inferred
                // type is `Unknown` — at which point narrowing could
                // refine it to a concrete type that mismatches the
                // chosen overload. Skipping the record on
                // `full_match.is_some()` would let those narrowing-
                // visible mismatches escape silently.
                //
                // `Overloaded { flat, overloads }` (instead of
                // `Single(chosen)`) lets the downstream
                // `arg_diagnostics_query`'s `any_accepts` retry against
                // ALL overloads with narrowed types — this matters when
                // base types match overload A but narrowing makes only
                // overload B acceptable, and vice versa. `flat` keeps
                // the base-time `chosen` so a "no overload accepts"
                // failure renders the message against the same overload
                // the legacy emitter would have picked.
                let overloads_arc: Arc<[Arc<[TypeId]>]> = sigs
                    .iter()
                    .map(|s| s.params.iter().copied().collect())
                    .collect::<Vec<_>>()
                    .into();
                self.record_call_arg_binding(
                    callee,
                    args,
                    ParamsShape::Overloaded {
                        flat: chosen.params.iter().copied().collect(),
                        overloads: overloads_arc,
                    },
                );
                // For multi-overload functions whose return types differ,
                // collapse to a union; otherwise just take the chosen
                // signature's return type. The 23 multi-overload platform
                // functions today all share a single return type per name
                // (e.g. `Булево` for AttachAddIn), so the union path is a
                // future-proof default.
                let ret = if sigs.len() == 1 {
                    chosen.ret
                } else {
                    self.db.union(sigs.iter().map(|s| s.ret).collect())
                };
                // Pin the bare callee's cached type so the second pass
                // in `infer_all` doesn't re-enter `Expr::Path` on the
                // function-name token. Mirrors the `Expr::Field`
                // callee fix above.
                self.expr_types.insert(callee, self.db.unknown());
                return ret;
            }
        }

        // Pre-extract the bare-name callee for the Phase O.11 cascade
        // arm in the `Ty::Unknown` match branch below. The match
        // pattern needs to outlive the upcoming mutable
        // `self.infer_expr(...)` calls, so we clone the name out
        // here while `callee_expr` is still cheap to borrow.
        let bare_callee_name: Option<hir_def::Name> = match callee_expr {
            Expr::Path(n) => Some(n.clone()),
            _ => None,
        };

        // Infer callee type for non-qualified calls
        let callee_ty = self.infer_expr(callee);

        // Infer argument types
        for arg in args {
            self.infer_expr(*arg);
        }

        // Check if callee is a function type
        let callee_kind = self.db.lookup_type(callee_ty);
        match callee_kind {
            TypeKind::Function(facet) => {
                // Arity check honours per-parameter defaults and the
                // documented `max_args` cap. The lower bound is the count
                // of required (non-default) leading parameters. The upper
                // bound is `max_args` (e.g. `Some(11)` for `СтрШаблон`,
                // `Some(2)` for `НСтр`, `None` for genuinely unbounded
                // variadics like the `ОписаниеТипов` fallback).
                let total = facet.params.len();
                let required = facet.min_args as usize;
                let too_few = args.len() < required;
                let too_many = match facet.max_args {
                    ArgArity::Fixed(n) => args.len() > n as usize,
                    ArgArity::Variadic => false,
                    _ => false,
                };
                if too_few || too_many {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr: callee,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                // M4 Task 7 follow-up: argument type check. Pairs zip
                // to `min(args, params)` so a prior `MismatchedArgCount`
                // does not double-fire as a `TypeMismatch` on the
                // unpaired tail. The per-pair predicate is
                // [`crate::subtype::is_assignable`], which treats
                // `Ty::Unknown` on either side as permissive — typical
                // for BSL where param declarations are often absent or
                // only partially typed via JSDoc.
                self.record_call_arg_binding(
                    callee,
                    args,
                    ParamsShape::Single(facet.params.iter().map(|p| p.ty).collect()),
                );

                // Return function's return type
                facet.returns
            }
            TypeKind::Unknown => {
                // Phase O.11 — same-module bare-fn cascade. When the
                // callee is a bare `Expr::Path(name)` that resolves
                // through the enclosing module's symbol_tree to a
                // user-defined method, consult
                // `method_return_type_query` so cold-body cascade
                // typing reaches the most common BSL idiom
                // (`Х = ЛокФн();` inside the same module). Builtins
                // were handled earlier in the function and returned
                // before reaching this arm; this branch only fires
                // for non-builtin bare names whose `callee_ty` came
                // back `Ty::Unknown` from `infer_path_name` (module
                // methods stay Unknown there per the doc at
                // `infer_path_name`).
                //
                // BSL scope rule (body-binding shadow): local
                // bindings (parameters, `Перем X;`, implicit locals
                // from `Stmt::Assign`) shadow module methods. The
                // body-binding probe mirrors the same guard at gate
                // 2 of `dispatch_bare_ident_field_call` so a
                // parameter named `Foo` does NOT spuriously resolve
                // through `symbol_tree.find_method("Foo")` to the
                // shadowed module method.
                //
                // Routing through `materialise_signature_enriched`
                // (same path as qualified-call enrichment) preserves
                // the docstring-wins precedence: if the resolved
                // method has an explicit return-type docstring, that
                // wins over `method_return_type_query` body inference.
                //
                // Scope (Codex O.12 C2): this arm surfaces only the
                // resolved method's RETURN type — it does NOT emit
                // arity (`MismatchedArgCount`) or argument-binding
                // diagnostics. The legacy `Ty::Unknown` arm did
                // not either, so this is no regression; same-module
                // bare-call argument validation is a separate
                // follow-up (mirror of `Ty::Function`-arm's
                // `record_call_arg_binding`/arity checks).
                if let Some(name) = bare_callee_name.as_ref() {
                    if !self.body_declares_binding(name)
                        && !self.assigned_var_names.contains(&name.as_str().to_lowercase())
                    {
                        let module_id = hir_def::ModuleId::new(self.file_id);
                        let symbol_tree = self.db.symbol_tree(module_id);
                        if let Some(method) = symbol_tree.find_method(name) {
                            let sig = crate::method_resolution::materialise_signature_enriched(
                                self.db, method.id, method,
                            );
                            let ret = sig.ret;
                            if !self.is_unknown(ret) {
                                return ret;
                            }
                        }
                    }
                }
                self.db.unknown()
            }
            _ => {
                // Callee is not a function type
                // Phase 2+: Could emit diagnostic here
                self.db.unknown()
            }
        }
    }

    /// Infer type from a qualified method call (Module.Method()).
    ///
    /// Phase 3: CommonModule.Method() resolution with diagnostics.
    fn infer_qualified_call(
        &mut self,
        module_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
    ) -> TypeId {
        // Infer argument types first
        for arg in args {
            self.infer_expr(*arg);
        }

        let resolver = self.get_resolver();

        // Resolve the qualified call. The Resolver reads `db.configurations()`
        // so `db.infer` transitively depends on the workspace config set,
        // and `set_all_config_paths` invalidates inference through Salsa.
        match method_resolution::resolve_qualified_call(
            self.db,
            module_name,
            method_name,
            &resolver,
        ) {
            Ok(resolution) => {
                // Method found!

                // Check export flag
                if !resolution.is_export {
                    self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: module_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                // Check argument count
                let total = resolution.signature.params.len();
                let required = resolution.signature.required_count();
                if args.len() < required || args.len() > total {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                // Argument type check — same rationale as the
                // plain-function branch in `infer_call`.
                self.record_call_arg_binding(
                    call_expr,
                    args,
                    ParamsShape::Single(resolution.signature.params.iter().copied().collect()),
                );

                // Return method's return type
                resolution.return_type
            }
            Err(kind) => {
                // Workspace miss — try the platform global-context catalogue
                // before declaring failure. `ОбработкаОшибок.КраткоеПредставлениеОшибки(...)`
                // and similar built-in calls live there: receiver is a global
                // identifier whose declared type carries the actual method.
                //
                // Two narrowings keep this from masking real diagnostics:
                //
                // 1. Only fall through on `MethodNotFound`. `MethodNotExport`
                //    means the workspace *has* a method with that name; the
                //    user must keep the export-error diagnostic instead of
                //    silently picking a coincidentally-named platform method.
                //
                // 2. Even within `MethodNotFound`, hir-def collapses two
                //    cases: "module unknown" AND "module known but method
                //    missing/non-visible". The platform fallback is only
                //    correct for the first one. A user CommonModule named
                //    e.g. `Метаданные` (shadowing the platform global) with
                //    a typo'd member must keep its own diagnostic — so probe
                //    `module_index` directly: a `Some` here means the
                //    workspace owns the receiver and platform fallback would
                //    mask a real bug.
                if matches!(kind, UnresolvedMethodKind::MethodNotFound) {
                    let source_root_id =
                        self.db.file_source_root_input(self.file_id).source_root_id(self.db);
                    let module_in_workspace = self
                        .db
                        .module_index(source_root_id)
                        .resolve_common_module(module_name)
                        .is_some();

                    if !module_in_workspace {
                        // Only the `Resolved` outcome short-circuits
                        // the Err arm here; `KnownContainerMissingMember`
                        // and `NotAContainer` keep the diagnostic
                        // emission below (preserves the legacy
                        // QualifiedPath path's behaviour — only
                        // `dispatch_bare_ident_field_call`'s gate 4
                        // splits the two miss outcomes into distinct
                        // kinds).
                        if let crate::platform_global_lookup::PlatformGlobalLookup::Resolved(
                            return_ty,
                        ) = crate::platform_global_lookup::try_resolve_platform_global_member(
                            module_name,
                            method_name,
                        ) {
                            self.expr_types.insert(call_expr, self.db.unknown());
                            return ty_to_typeid(self.db, &return_ty);
                        }
                    }
                }

                // Method not found - emit diagnostic
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name: module_name.clone(),
                    method_name: method_name.clone(),
                    kind,
                });

                self.db.unknown()
            }
        }
    }

    /// Cascade-gate dispatcher for bare-IDENT receiver in
    /// `Expr::Call { callee: Expr::Field { base: Expr::Path(name), field } }`.
    ///
    /// Wired into `infer_call`'s Field branch in Phase 2 of the
    /// clean-architecture refactor. Body lowering no longer rewrites
    /// `M.Method()` into `Expr::Call{QualifiedPath}` (the eager
    /// TwoLevel arm in `hir-def/src/body/lower/expr.rs` was removed
    /// in the same commit), so 2-segment `Module.Method()` calls
    /// reach inference as `Expr::Call { callee: Expr::Field { base:
    /// Expr::Path(name), field } }` and route through this dispatcher
    /// when `infer_expr(base) == Ty::Unknown`. The earlier
    /// `Expr::Call { callee: QualifiedPath }` shape is now reserved
    /// for callers that build it directly (none today) — the legacy
    /// `infer_qualified_call` is invoked only as gate 3's delegate.
    ///
    /// ## Cascade gate (clean-architecture rationale)
    ///
    /// Lowering classified `M.Method()` as a CommonModule call by
    /// negative inference (`not local var, not param`). That fired
    /// false positives for form attributes, implicit form globals,
    /// module-level `Перем` declarations, and platform globals.
    /// Inference owns the resolver and the receiver type, so it can
    /// classify positively. The 5 gates run in order; the first hit
    /// wins:
    ///
    /// 1. `Resolver::resolve_name` returns `Local | Variable | Method`
    ///    — bound name; caller's existing Field-call path handles the
    ///    typed receiver. Return `None` (silent).
    /// 2. Body-side bindings invisible to the resolver — declared
    ///    `Перем X;` (via `body_declares_binding`) AND implicit
    ///    locals from `Stmt::Assign` (via `var_types`). Both: silent.
    /// 3. `Resolver::user_common_module_exists` — workspace owns the
    ///    receiver name as a CommonModule. Delegate to
    ///    `infer_qualified_call`, which itself runs workspace-first
    ///    and falls through to platform globals when appropriate.
    ///    User shadows platform per BSL semantics — gate 3 runs
    ///    BEFORE gate 4, mirroring the existing
    ///    `infer_qualified_call:2056` workspace-first invariant and
    ///    the `test_user_module_shadows_platform_global` regression.
    /// 4. Platform global member (`ОбработкаОшибок.Краткое…`, …) via
    ///    `platform_global_lookup::try_resolve_platform_global_member`.
    /// 5. None of the above — emit `UnresolvedMethodCall {
    ///    ReceiverNotResolved }` and return `Ty::Unknown`.
    ///
    /// `Resolution::Builtin` from gate 1 falls through (a builtin
    /// container — like `ОбработкаОшибок` — is precisely the case
    /// gate 4 covers; treating it as "bound" would mask the method
    /// resolution).
    fn dispatch_bare_ident_field_call(
        &mut self,
        module_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
    ) -> Option<TypeId> {
        use hir_def::resolver::Resolution;

        let resolver = self.get_resolver();

        // Gate 1 — known scope binding (parameter / local / module
        // method / module variable). `Builtin` falls through: it
        // names a platform global container that gate 4 will resolve
        // as a method receiver.
        match resolver.resolve_name(self.db, module_name) {
            Some(Resolution::Local(_) | Resolution::Variable(_) | Resolution::Method(_)) => {
                return None;
            }
            Some(Resolution::Builtin(_)) | None => {}
        }

        // Gate 2 — body-side bindings invisible to `Resolver::resolve_name`.
        //
        //   (a) Declared `Перем X;` / parameter without a prior
        //       assignment: lives in `Body::Binding` but never reaches
        //       `var_types` / `Scope::ExprScope`. `body_declares_binding`
        //       covers it.
        //
        //   (b) Implicit local from `Stmt::Assign`
        //       (`X = НеизвестнаяФункция(); X.Метод()`): tracked through
        //       `InferenceContext::var_types` only — not a `Body::Binding`,
        //       not reachable through `resolver.resolve_local`. Without
        //       this probe the cascade would walk all the way to gate 5
        //       and falsely emit `ReceiverNotResolved` for a perfectly
        //       normal implicit local whose RHS happens to type to
        //       `Ty::Unknown`.
        //
        // Both subcases must short-circuit silent — the call is on a
        // local value, not a CommonModule or platform global. Lowercase
        // probe matches BSL's case-insensitive identifier semantics
        // (same key normalisation `infer_path_name` uses on `var_types`).
        if self.body_declares_binding(module_name) {
            return None;
        }
        if self.assigned_var_names.contains(&module_name.as_str().to_lowercase()) {
            return None;
        }

        // Gate 3 — user CommonModule. Precedence over platform
        // (gate 4) per `test_user_module_shadows_platform_global`.
        //
        // When the receiver positively resolves to a workspace
        // CommonModule, emit the two body-side family diagnostics
        // (RedundantAccessToObjectTwoLevel,
        // MissedRequiredParameterCommonModule) BEFORE delegating to
        // `infer_qualified_call`. The handlers run their own filters
        // — the redundant-access handler keeps the diagnostic only
        // when the receiver name matches the *enclosing* CommonModule
        // (and `ReturnValueReuse == DontUse`), and the missed-required
        // handler resolves the method through the SymbolTree and
        // surfaces only genuinely missing required slots — so over-
        // emission here is filtered downstream. The two channels are
        // intentional: lowering used to fire them eagerly without a
        // receiver type, which false-positived for form attributes,
        // module-level `Перем`, and platform globals; lifting them
        // here gates emission on `user_common_module_exists`.
        //
        // `infer_qualified_call` itself already runs workspace-first
        // and emits `UnresolvedMethodCall { MethodNotFound |
        // MethodNotExport }` on resolution failure — we just delegate
        // for the type and any further diagnostics it owns.
        if resolver.user_common_module_exists(self.db, module_name) {
            // `args: Vec<bool>` — per-slot presence flag for the
            // missed-required-parameter handler. Mirrors lowering's
            // `extract_arg_presence` but reads from `Body` directly:
            // an `Expr::Missing` placeholder sits in the slot when
            // the parser dropped a skipped positional argument
            // (`Foo(1,,3)`).
            let arg_presence: Vec<bool> = args
                .iter()
                .map(|arg_id| !matches!(self.body.expr(*arg_id), Expr::Missing))
                .collect();

            self.push_inference_diagnostic(InferenceDiagnostic::RedundantAccessToObjectTwoLevel {
                expr: call_expr,
                module: module_name.clone(),
            });

            self.push_inference_diagnostic(
                InferenceDiagnostic::MissedRequiredParameterCommonModule {
                    expr: call_expr,
                    callee: method_name.clone(),
                    module: module_name.clone(),
                    args: arg_presence,
                },
            );

            return Some(self.infer_qualified_call(module_name, method_name, args, call_expr));
        }

        // Gate 4 — platform global container. Tri-state: a `Resolved`
        // outcome wins; `KnownContainerMissingMember` ⇒ MethodNotFound
        // (the receiver IS a real platform global like `ОбработкаОшибок`,
        // the user just typo'd the method); `NotAContainer` falls through
        // to gate 5's `ReceiverNotResolved`.
        match crate::platform_global_lookup::try_resolve_platform_global_member(
            module_name,
            method_name,
        ) {
            crate::platform_global_lookup::PlatformGlobalLookup::Resolved(return_ty) => {
                for arg in args {
                    self.infer_expr(*arg);
                }
                self.expr_types.insert(call_expr, self.db.unknown());
                return Some(ty_to_typeid(self.db, &return_ty));
            }
            crate::platform_global_lookup::PlatformGlobalLookup::KnownContainerMissingMember => {
                for arg in args {
                    self.infer_expr(*arg);
                }
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name: module_name.clone(),
                    method_name: method_name.clone(),
                    kind: UnresolvedMethodKind::MethodNotFound,
                });
                return Some(self.db.unknown());
            }
            crate::platform_global_lookup::PlatformGlobalLookup::NotAContainer => {}
        }

        // Gate 5 — receiver is unknown to every layer. Emit
        // `ReceiverNotResolved` so the user gets a single, accurate
        // diagnostic instead of the misleading "method missing on
        // CommonModule" pair lowering used to produce.
        for arg in args {
            self.infer_expr(*arg);
        }
        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
            expr: call_expr,
            receiver_name: module_name.clone(),
            method_name: method_name.clone(),
            kind: UnresolvedMethodKind::ReceiverNotResolved,
        });
        Some(self.db.unknown())
    }

    /// Infer type from a three-segment manager-chain call
    /// (`Документы.ПКО.СоздатьДокумент()`).
    ///
    /// Delegates to [`method_resolution::resolve_three_level_call`], which
    /// in turn goes through [`Resolver::resolve_three_level_method`] — so
    /// `db.infer` transitively depends on `db.configurations()` via Salsa
    /// and the CFE visibility gate is enforced automatically.
    ///
    /// Diagnostic shape mirrors `infer_qualified_call`: the receiver name
    /// glued as `<mdo_type>.<mdo_name>` so callers see the full head when
    /// the method is missing or non-exported.
    fn infer_three_level_call(
        &mut self,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
    ) -> TypeId {
        for arg in args {
            self.infer_expr(*arg);
        }

        let resolver = self.get_resolver();
        let receiver_name =
            Name::new(&format!("{}.{}", mdo_type_plural.as_str(), mdo_name.as_str()));

        match method_resolution::resolve_three_level_call(
            self.db,
            mdo_type_plural,
            mdo_name,
            method_name,
            &resolver,
        ) {
            Ok(resolution) => {
                if !resolution.is_export {
                    self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: receiver_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                let total = resolution.signature.params.len();
                let required = resolution.signature.required_count();
                if args.len() < required || args.len() > total {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                // Argument type check — mirrors `infer_qualified_call`.
                self.record_call_arg_binding(
                    call_expr,
                    args,
                    ParamsShape::Single(resolution.signature.params.iter().copied().collect()),
                );

                resolution.return_type
            }
            Err(UnresolvedMethodKind::MethodNotFound) => {
                // Workspace lookup missed — fall back to the platform
                // manager catalogue. This is where stock methods like
                // `Справочники.X.СоздатьЭлемент()` /
                // `Документы.X.СоздатьДокумент()` live. Only the
                // `MethodNotFound` arm falls through: `MethodNotExport`
                // means the workspace *has* a method with this name
                // that is just not exported — the diagnostic belongs to
                // the user's module, not to the platform catalogue.
                //
                // Gating: only fall back when the MDO itself is
                // declared in a visible configuration. Without this
                // check, a typo'd receiver
                // (`Документы.НетТакогоДокумента.ПолучитьСсылку()`) would
                // silently succeed against platform data instead of
                // surfacing the missing MDO. `configurations.is_empty()`
                // preserves historic behaviour for fixture-only tests
                // that never register a configuration — they pre-date
                // the platform fallback and rely on the diagnostic
                // firing.
                let mdo_type_opt = bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str());
                let plat_res: Option<PlatformMethodResolution> = mdo_type_opt
                    .filter(|mdo_type| self.mdo_declared(*mdo_type, mdo_name))
                    .and_then(|mdo_type| {
                        resolve_platform_manager_method(mdo_type, mdo_name, method_name)
                    });
                if let Some(mut res) = plat_res {
                    if mdo_type_opt == Some(bsl_metadata::MdoType::Constant) {
                        let mut return_ty = res.return_typeid(self.db);
                        let mut params = res.signature_params_typeid(self.db);
                        self.refine_constant_method(
                            mdo_name,
                            method_name,
                            &mut return_ty,
                            &mut params,
                        );
                        res.return_ty = typeid_to_ty(self.db, return_ty);
                        res.signature.params = params
                            .iter()
                            .map(|id| typeid_to_ty(self.db, *id))
                            .collect::<Vec<_>>()
                            .into_boxed_slice();
                    }
                    let total = res.signature.params.len();
                    let required = res.signature.required_count();
                    if args.len() < required || args.len() > total {
                        self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                            call_expr,
                            required_count: required,
                            total_count: total,
                            found: args.len(),
                        });
                    }
                    self.record_call_arg_binding(
                        call_expr,
                        args,
                        ParamsShape::Single(res.signature_params_typeid(self.db).into()),
                    );
                    return res.return_typeid(self.db);
                }

                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind: UnresolvedMethodKind::MethodNotFound,
                });
                self.db.unknown()
            }
            Err(kind) => {
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind,
                });
                self.db.unknown()
            }
        }
    }

    /// Is `(mdo_type, mdo_name)` declared in a configuration visible from
    /// the current file?
    ///
    /// Guards the platform-manager fallback in
    /// [`Self::infer_three_level_call`]: without this check a typo'd
    /// receiver would silently hit platform data. The policy mirrors
    /// `Resolver::mdo_visible_in_configs` — the visibility gate the
    /// resolver itself uses before traversing manager modules.
    ///
    /// Returns `false` when no configuration is registered so the
    /// behaviour for fixture-only tests stays consistent with the
    /// pre-fallback baseline (they never saw platform resolution and
    /// relied on `UnresolvedMethodCall` for missing MDO names).
    fn mdo_declared(&self, mdo_type: bsl_metadata::MdoType, mdo_name: &Name) -> bool {
        let configs = self.db.configurations(self.file_id);
        if configs.is_empty() {
            return false;
        }
        let needle = mdo_name.as_str();
        configs.iter().any(|vc| {
            // `find_metadata_object` covers catalog / document / enum /
            // chart-of-* / task / business-process / exchange-plan, but
            // registers live in a parallel `find_register_by_type_and_name`
            // table. Check both so a real
            // `РегистрыСведений.Курсы.СоздатьМенеджерЗаписи()` reaches
            // the platform fallback.
            vc.configuration.find_metadata_object(mdo_type, needle).is_some()
                || vc.configuration.find_register_by_type_and_name(mdo_type, needle).is_some()
        })
    }

    /// Look up a constant's declared value type in the visible
    /// configurations.
    ///
    /// Iterates `configurations(file_id)` in reverse so a CFE extension
    /// declaration of the constant wins over the main configuration —
    /// matches the override-wins policy already in place for
    /// field/manager lookups (`crates/hir-ty/src/field_lookup.rs`,
    /// `crates/hir-ty/src/manager_lookup.rs`).
    ///
    /// Returns `None` when the constant is not declared in any visible
    /// configuration **or** is declared without a `<Type>` element. The
    /// caller treats `None` as "fall back to whatever the platform
    /// lookup produced" — for `Получить` / `Установить` that is
    /// `Ty::Unknown` after the M4 `Произвольный` fix in
    /// [`crate::method_lookup::resolve_platform_type_name`], which the
    /// gradual rule then accepts in any typed slot.
    fn resolve_constant_value_type(&self, mdo_name: &Name) -> Option<TypeId> {
        let configs = self.db.configurations(self.file_id);
        if configs.is_empty() {
            return None;
        }
        let needle = mdo_name.as_str();
        for vc in configs.iter().rev() {
            // Lookup the *constant declaration* (not the type) — an
            // extension that declares the constant without a `<Type>`
            // element shadows the base configuration's declaration. If
            // we keyed on `find_constant_type` (which returns `None`
            // for both "no constant" and "constant without type"),
            // the iteration would fall through to the base and serve
            // the wrong type for an explicit untyped override.
            let Some(mdo) =
                vc.configuration.find_metadata_object(bsl_metadata::MdoType::Constant, needle)
            else {
                continue;
            };
            return mdo.constant_type.as_ref().map(|attr| {
                let type_ref = hir_def::TypeRef::from_attribute_type(attr);
                ty_to_typeid(self.db, &TyLoweringContext::new().lower_type_ref(&type_ref))
            });
        }
        None
    }

    /// Refine the `Получить` / `Установить` slot of a `Константы.<Имя>`
    /// method resolution with the constant's configuration-declared
    /// value type.
    ///
    /// Whitelisted methods only:
    /// - `Получить` / `Get` — overrides `return_ty` when the platform
    ///   lookup yielded `Ty::Unknown` (the post-fix-#1 lowering of
    ///   `"Произвольный"`).
    /// - `Установить` / `Set` — overrides the first parameter type
    ///   when it is `Ty::Unknown`.
    ///
    /// All other constant-manager methods (`Метаданные`,
    /// `СоздатьМенеджерЗначения`, `SetDataHistoryVersion*`, …) pass
    /// through untouched. The `== Ty::Unknown` guard preserves any
    /// future generic-metadata refinement that lands in
    /// [`crate::platform_manager_lookup::map_generic_metadata_return_type`].
    ///
    /// `to_lowercase()` is used because `eq_ignore_ascii_case` does
    /// not fold Cyrillic; `infer.rs` does not currently expose a
    /// reusable bilingual matcher, so the comparison stays inline.
    fn refine_constant_method(
        &self,
        mdo_name: &Name,
        method_name: &Name,
        return_ty: &mut TypeId,
        params: &mut [TypeId],
    ) {
        let lc = method_name.as_str().to_lowercase();
        let is_get = lc == "получить" || lc == "get";
        let is_set = lc == "установить" || lc == "set";
        if !is_get && !is_set {
            return;
        }
        let needs_override = (is_get && self.is_unknown(*return_ty))
            || (is_set && params.first().is_some_and(|id| self.is_unknown(*id)));
        if !needs_override {
            return;
        }
        let Some(value_ty) = self.resolve_constant_value_type(mdo_name) else {
            return;
        };
        if is_get {
            *return_ty = value_ty;
        } else if is_set {
            params[0] = value_ty;
        }
    }
}

/// Build the qualified `Plural.MDO` receiver-name string used in
/// `UnresolvedMethodCall` diagnostics for a 2-shape `receiver.method()`
/// call site.
///
/// Returns `Some` only for **authoritative** receivers (`MetadataRef`,
/// `ThisObject`) where pre-fix the standalone `Expr::Field` arm already
/// emitted `UnresolvedField` for the same miss — this fix just renames
/// the diagnostic to the correct kind. Returns `None` for everything
/// else — partial-table receivers (`Unknown`, `Union`, primitives,
/// `PlatformObject`, `ManagerCollection`, register kinds) stay silent
/// because a miss is "we can't tell yet" rather than "method does not
/// exist".
///
/// `Ty::ObjectManager` is included as authoritative: the call-site in
/// `infer_call`'s `Expr::Field` branch consults the workspace
/// `ManagerModule.bsl` resolver first (Phase A) and only reaches the
/// `lookup_method` miss path when that workspace lookup also failed,
/// so by the time `receiver_display_name` is queried both layers
/// agree. `Ty::ThisManager` is included for completeness — `infer_call`
/// only reaches `receiver_display_name` after the Phase A coerce-and-
/// match has already produced a workspace verdict, but the
/// `ThisObject | ThisManager` arm here means a direct caller (e.g. a
/// non-coerced fall-through path) still gets a sensible display name.
///
/// The user-visible form matches the 3-segment path's `<Plural>.<MDO>`
/// convention rendered by `unresolved_method_call::from_hir`.
/// Evaluate a bare numeric literal as a non-negative integer index.
///
/// Used by `Expr::Index` on `Ty::QueryBatchResult` to extract the
/// i-th sub-query's projection. Deliberately rejects anything that
/// isn't a positive integer literal — negative values are
/// `Expr::UnaryOp { op: Minus, .. }`, named locals are `Expr::Local`,
/// arithmetic is `Expr::BinaryOp`. None of those carry a statically
/// known index at this layer, so they degrade to `None` (which
/// surfaces as `Ty::QueryResult{None}`, mirroring the
/// platform-runtime behaviour of "the call returned a result of
/// unknown schema").
fn const_eval_literal_index(expr: &Expr) -> Option<usize> {
    let Expr::Literal(Literal::Number(n)) = expr else {
        return None;
    };
    let f = n.into_inner();
    if !f.is_finite() || f < 0.0 || f.fract() != 0.0 {
        return None;
    }
    Some(f as usize)
}

fn sdbl_projection_to_projection(
    _db: &dyn TypeKernelDb,
    projection: &Arc<SdblProjection>,
) -> Arc<Projection> {
    let fields: Arc<[ProjectionField]> = projection
        .fields
        .iter()
        .map(|(name, ty)| {
            ProjectionField::new(name.as_str().to_string(), *ty, ProjectionFieldSource::Unknown)
        })
        .collect();
    let raw_sdbl_types = projection.raw_sdbl_types.as_ref().map(|shadows| {
        shadows
            .iter()
            .map(|s| bsl_types::facet::SdblTypeShadowFacet::new(s.display.clone()))
            .collect::<Arc<[_]>>()
    });
    Arc::new(Projection::new(fields, ProjectionOrigin::SdblQuery, raw_sdbl_types))
}

fn receiver_display_name(db: &dyn TypeKernelDb, receiver_ty: TypeId) -> Option<hir_def::Name> {
    match db.lookup_type(receiver_ty) {
        TypeKind::MetadataRef(facet) => {
            let plural = mdo_kind_to_plural(facet.kind)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::ThisObject { owner, .. } | TypeKind::ThisManager { owner, .. } => {
            let plural = mdo_type_to_plural(owner.mdo_type)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, owner.name.as_str())))
        }
        TypeKind::ObjectManager(facet) => {
            let plural = mdo_type_to_plural(facet.mdo)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::FormData { kind: FormDataFacet::Collection, underlying: Some(underlying) } => {
            let plural = mdo_type_to_plural(underlying.mdo_type)?;
            let name = &underlying.name;
            Some(hir_def::Name::new(&format!("{}.{}", plural, name.as_str())))
        }
        _ => None,
    }
}

/// Russian plural form for an [`MdoType`] — the inverse of
/// [`bsl_metadata::MdoType::from_plural`]. Mirrors only those flavours
/// that have a stable public-call surface (`Документы.X.Метод()` etc.);
/// flavours without a manager-style call surface (`Constant`, `Cube`,
/// `DimensionTable`, `CommonModule`, `ExternalDataSource`) return `None`.
fn mdo_type_to_plural(mdo_type: bsl_metadata::MdoType) -> Option<&'static str> {
    use bsl_metadata::MdoType;
    Some(match mdo_type {
        MdoType::Document => "Документы",
        MdoType::Catalog => "Справочники",
        MdoType::InformationRegister => "РегистрыСведений",
        MdoType::AccumulationRegister => "РегистрыНакопления",
        MdoType::AccountingRegister => "РегистрыБухгалтерии",
        MdoType::CalculationRegister => "РегистрыРасчета",
        MdoType::ChartOfCharacteristicTypes => "ПланыВидовХарактеристик",
        MdoType::ChartOfAccounts => "ПланыСчетов",
        MdoType::ChartOfCalculationTypes => "ПланыВидовРасчета",
        MdoType::BusinessProcess => "БизнесПроцессы",
        MdoType::Task => "Задачи",
        MdoType::Enum => "Перечисления",
        MdoType::ExchangePlan => "ПланыОбмена",
        MdoType::DataProcessor => "Обработки",
        MdoType::Report => "Отчеты",
        _ => return None,
    })
}

/// Map a [`MetadataKind`] to its parent MDO plural for diagnostic
/// display, covering object/ref/manager kinds whose owner family is
/// uniquely determined.
///
/// Register-record kinds
/// (`InformationRegisterRecordManager`,
/// `AccumulationRegisterRecordSet`) return their parent register
/// plural — Phase C wired `MetadataKind::platform_prefix` for these
/// kinds (and the workspace `RecordSetModule.bsl` resolver for the
/// record-set kind only), so a `lookup_method` miss is authoritative:
///
/// - `InformationRegisterRecordManager` — platform path wired (1С's
///   record-manager methods like `Записать`, `Прочитать`); no
///   workspace path because `RecordSetModule.bsl` requires a
///   record-set receiver, not a record-manager.
/// - `AccumulationRegisterRecordSet` — both platform AND workspace
///   `RecordSetModule.bsl` paths wired.
///
/// `*RegisterRef` value kinds remain `None` — those are XML-emitted
/// reference forms whose call surface lives elsewhere (no platform
/// or RecordSetModule.bsl analogue). Their workspace resolution
/// would require dedicated MetadataKind variants and is out of scope.
fn mdo_kind_to_plural(kind: hir_def::ty::MetadataKind) -> Option<&'static str> {
    use hir_def::ty::MetadataKind;
    let mdo = match kind {
        MetadataKind::CatalogObject | MetadataKind::CatalogRef => bsl_metadata::MdoType::Catalog,
        MetadataKind::DocumentObject | MetadataKind::DocumentRef => bsl_metadata::MdoType::Document,
        MetadataKind::EnumRef => bsl_metadata::MdoType::Enum,
        MetadataKind::TaskRef | MetadataKind::TaskObject => bsl_metadata::MdoType::Task,
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            bsl_metadata::MdoType::BusinessProcess
        }
        MetadataKind::DataProcessorObject => bsl_metadata::MdoType::DataProcessor,
        MetadataKind::ReportObject => bsl_metadata::MdoType::Report,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
            bsl_metadata::MdoType::ExchangePlan
        }
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            bsl_metadata::MdoType::ChartOfAccounts
        }
        // Register-record kinds: Phase C wired the platform side
        // (`platform_prefix` + `metadata_kind_to_prefix_and_mdo`) so
        // misses are authoritative. The record-set kind also has a
        // workspace `RecordSetModule.bsl` path; the record-manager
        // kind does not (1С semantics — record-manager doesn't reach
        // record-set module exports).
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet => bsl_metadata::MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet => bsl_metadata::MdoType::AccumulationRegister,
        MetadataKind::AccountingRegisterRecordSet => bsl_metadata::MdoType::AccountingRegister,
        MetadataKind::CalculationRegisterRecordSet => bsl_metadata::MdoType::CalculationRegister,
        // `*RegisterRef` value kinds: no module-level call surface
        // (no `RecordSetModule.bsl` for the *Ref form), no platform
        // surface. Silence is the honest answer.
        // `*RegisterRecord` element kinds (yielded by iterating a
        // record-set with `Для каждого … Из …`): platform methods are
        // wired via `platform_prefix`, but workspace doesn't expose a
        // module-level call surface for individual records. Same answer.
        MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecord => return None,
        // Tabular-section kinds: `lookup_method` resolves their methods
        // through `PlatformData["Tabular section"]` and field_lookup
        // resolves their row properties through
        // `PlatformData["Line of a tabular section"]`, so a miss is
        // authoritative — surface it as `<Plural>.<MdoName>.<Section>`
        // (the section name is already encoded in `MetadataRef::name`).
        MetadataKind::TabularSection { parent } | MetadataKind::TabularSectionRow { parent } => {
            parent
        }
        // Register-part kinds and the synthetic `RegisterFilter` carry
        // a parent payload but no manager-style call surface. Returning
        // `None` keeps the diagnostic silent on these — `lookup_method`
        // either returns `None` or routes Filter methods through a
        // scalar-key side channel, so we never construct a misleading
        // `Регистры…/<Раздел>` receiver name.
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. } => return None,
    };
    mdo_type_to_plural(mdo)
}

/// Salsa tracked query: Infer types for all expressions in a file.
///
/// This is the main entry point for type inference. It:
/// 1. Gets the HIR bodies for the file via module_bodies query
/// 2. Creates an InferenceContext for each body
/// 3. Runs inference on all expressions
/// 4. Returns the cached result
///
/// # Caching
///
/// Salsa memoizes the `Arc<InferenceResult>` keyed on `FileIdInput`,
/// so repeated `db.infer(file_id)` calls inside one revision return the
/// same Arc without re-running inference. Without this attribute the
/// function ran in full for every caller — `narrow_query`,
/// `type_of_expr_query`, `Semantics::type_of_expr`, hover, goto-def,
/// completion — turning each interaction into a fresh whole-file pass.
///
/// Invalidation flows from the underlying tracked queries
/// (`module_bodies`, `infer_method_query`, `infer_module_code_query`,
/// configuration, etc.); we don't enumerate dependencies manually.
///
/// # Phase O.16a — thin fan-out wrapper
///
/// O.16a transformed this from an inline body-walker into a thin
/// wrapper that fans out through the per-body Salsa queries:
///   1. `db.infer_module_code(file_id)` for module-level code (O.14).
///   2. `db.infer_method(MethodIdInput)` for each method body, walked
///      in [`ModuleBodies::iter_bodies`] insertion order (O.15).
///   3. Folds every per-body `Arc<…InferenceResult>` payload back into
///      the file-level [`InferenceResult`] for diagnostics /
///      find-references / `arg_diagnostics_query` consumers.
///
/// Per-body inference now lives in its own Salsa cell. Narrow callers
/// (hover / highlight / goto-def — wired in O.17) read those cells
/// directly without re-entering the wrapper. File-wide consumers
/// (arg_diagnostics_query, find-references, publishDiagnostics, ide
/// tests) continue to read the wrapper's aggregate; the trade is one
/// clone per per-owner map field plus entry/vector clones during the
/// fold in exchange for the per-method partitioning that lets warm
/// narrow paths skip the wrapper entirely.
///
/// # Determinism invariant (O.16a contract)
///
/// `ModuleBodies::iter_bodies` is IndexMap-backed (Phase O.1, Lni.A)
/// and yields methods in insertion order, which equals their
/// `LocalBodyId`-sorted order because the lowering loop inserts in
/// increasing index. The fold therefore visits methods in a single
/// file-fixed order; `result.diagnostics`, `result.call_arg_bindings`
/// and the iteration-dependent `result.var_types` last-write-wins
/// outcome are byte-for-byte deterministic across runs.
#[salsa::tracked(lru = 256)]
pub fn infer_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<InferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_query", ?file_id).entered();

    // Get HIR bodies from DefDatabase
    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

    let mut result = InferenceResult::default();

    // Phase O.16a fold helpers — both per-body results land in the
    // same file-level aggregate. Take `&` because `db.infer_method` /
    // `db.infer_module_code` return `Arc<…>` whose payload is also
    // pinned by Salsa's cache; we cannot move out of it, so the
    // fields are cloned into `result`. Pre-O.16a the walker built
    // the per-body result inline and moved it; the new wrapper pays
    // **one clone per per-owner map field plus entry/vector clones**
    // per body — three map clones (`expr_types`, `binding_types`,
    // `implicit_locals`) plus entry-by-entry extends of
    // `var_types`, `diagnostics`, `call_arg_bindings`. In exchange
    // each body gets its own Salsa partition so warm narrow callers
    // skip the wrapper entirely.
    let fold_body = |result: &mut InferenceResult, body_result: &BodyInferenceResult| {
        let owner = body_result.owner;
        // Preserve per-body expr_types so `Semantics::type_of_expr`
        // (M3 Task 9) can look them up via `BodySourceMap`.
        result.expr_types_by_body.insert(owner, body_result.expr_types.clone());
        // Per-binding map: keyed by `BindingId` so name shadowing within
        // or across bodies does not collide. Used by binding-anchored
        // hover (declaration-site loop variable, classic-for counter,
        // parameter).
        result.binding_types_by_body.insert(owner, body_result.binding_types.clone());
        // `var_types` stays file-global: completion matches variables by
        // name across bodies. Iteration is fixed by ModuleBodies' IndexMap
        // backing, so last-write-wins outcome is deterministic.
        result.var_types.extend(body_result.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, body_result.implicit_locals.clone());
        // `diagnostics` is flat file-wide but each entry is paired with
        // its `DefWithBodyId` owner so ide-diagnostics can resolve the
        // body-local `ExprId` through the correct `BodySourceMap`.
        result.diagnostics.extend(body_result.diagnostics.iter().map(|d| (owner, d.clone())));
        // Call-site arg bindings carry their own owner field, so we just
        // append. `arg_diagnostics_query` consumes the file-wide list.
        result.call_arg_bindings.extend(body_result.call_arg_bindings.iter().cloned());
    };

    let fold_module_code = |result: &mut InferenceResult,
                            module_code: &ModuleCodeInferenceResult| {
        // Identical fold logic to `fold_body` minus the
        // `return_expr_ids` dimension (module code has no method-level
        // Return cascade consumer — see `ModuleCodeInferenceResult`
        // doc-comment).
        let owner = module_code.owner;
        result.expr_types_by_body.insert(owner, module_code.expr_types.clone());
        result.binding_types_by_body.insert(owner, module_code.binding_types.clone());
        result.var_types.extend(module_code.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, module_code.implicit_locals.clone());
        result.diagnostics.extend(module_code.diagnostics.iter().map(|d| (owner, d.clone())));
        result.call_arg_bindings.extend(module_code.call_arg_bindings.iter().cloned());
    };

    // Module-level code first — through the dedicated Salsa cell so a
    // warm hit is a cheap Arc clone (see `infer_module_code_query`).
    {
        let _bspan = tracing::info_span!("infer_query.body", kind = "module_code").entered();
        let module_code = db.infer_module_code(file_id);
        fold_module_code(&mut result, &module_code);
    }

    // Per-method bodies, walked in IndexMap-insertion order (== sorted
    // by LocalBodyId via Phase O.1 Lni.A). Each body lives in its own
    // `infer_method_query` Salsa cell; the wrapper's job here is pure
    // aggregation. On warm cache every `db.infer_method(…)` below is
    // an `Arc::clone`, so the wrapper touches no inference logic; on
    // cold first hit each cell still pays for its own body walk, but
    // the work is partitioned by method so narrow callers (hover /
    // highlight / goto-def) reuse those cells without re-entering the
    // wrapper.
    for (local_id, _body) in module_bodies.iter_bodies() {
        let _bspan = tracing::info_span!("infer_query.body", kind = "method").entered();
        let method_id = hir_def::MethodId { module: module_id, local_id };
        let method_input = MethodIdInput::new(db, method_id);
        let body_result = db.infer_method(method_input);
        fold_body(&mut result, &body_result);
    }

    info!(
        "type inference complete: {} bodies, {} var types, {} diagnostics",
        result.expr_types_by_body.len(),
        result.var_types.len(),
        result.diagnostics.len()
    );

    Arc::new(result)
}

/// Salsa query: per-file inference for the module-code body (Phase O.14).
///
/// Covers everything in [`DefWithBodyId::ModuleCode`] — module-level
/// `Перем` declarations, top-level `Stmt::Assign`, and any other
/// expressions outside a procedure / function. Narrow callers
/// targeting module-level scope route through
/// [`InferOwnerResult::ModuleCode`]; the file-wide `infer_query`
/// folds the result into [`InferenceResult::var_types`] for completion
/// and into `expr_types_by_body[ModuleCode]` for `Semantics::type_of_expr`.
///
/// # Cache key
///
/// Keyed on the salsa-interned [`FileIdInput`]; results are reused
/// across every call within the same revision. `lru = 1024` matches
/// the working set size that is realistic for ERP-scale workspaces
/// (most files have ≤ 1 module-code body each).
///
/// # Cycle safety
///
/// Calls `db.module_bodies` (cycle-free), runs inference inside
/// [`InferenceContext`] which may invoke `method_return_type_query`
/// (already cycle-safe via Phase J `cycle_fn` / `cycle_initial`), and
/// otherwise reads workspace symbols / resolver like any body.
/// Crucially does **not** call `infer_method` or `infer_query`, so the
/// query graph stays acyclic.
///
/// # Empty-result contract
///
/// Returns a default [`ModuleCodeInferenceResult`] when the file has
/// no module-code body (e.g. ManagerModule.bsl that only declares
/// methods). Callers can treat the empty case identically to a body
/// with no expressions — `var_types` / `expr_types` will be empty.
#[salsa::tracked(lru = 1024)]
pub fn infer_module_code_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleCodeInferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_module_code_query", ?file_id).entered();

    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

    let Some(body) = module_bodies.module_code() else {
        return Arc::new(ModuleCodeInferenceResult::default());
    };

    let mut ctx =
        InferenceContext::new(db, file_id, DefWithBodyId::ModuleCode, &Arc::new(body.clone()));
    ctx.infer_all();
    let body_result = ctx.finish();
    Arc::new(ModuleCodeInferenceResult::from_body(body_result))
}

/// Salsa query: Get type of an expression in a specific body.
///
/// `ExprId` is only unique within a single `Body`, so callers must
/// disambiguate with `DefWithBodyId` — `Method(local_id)` for a
/// procedure / function, `ModuleCode` for module-level code. The
/// `Semantics::type_of_expr(SyntaxNode)` helper in `hir` derives the
/// owner automatically by walking up the syntax tree.
///
/// # Returns
///
/// - The interned [`TypeId`] for `(owner, expr)` if present.
/// - The kernel `Unknown` id if inference produced no entry for that pair.
pub fn type_of_expr_query(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    expr: ExprId,
) -> TypeId {
    // Phase O.17: route through per-owner Salsa cell instead of the
    // file-wide `infer_query` aggregate. Warm hits become a single
    // Arc::clone on the `infer_method` / `infer_module_code` cell.
    // Phase 3 §4.G.5a: the public boundary is kernel-native — return the
    // interned `TypeId` directly, no `Ty` bridge at the query.
    infer_owner(db, file_id, owner).type_id_of_expr(expr).unwrap_or_else(|| db.unknown())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_bridge::{ty_to_typeid, typeid_to_ty};
    use bsl_types::testing::InMemoryDb;

    /// `ParamsShape` stores kernel ids directly; bridge back only when a
    /// legacy `Ty` view is needed by an assertion or downstream payload.
    #[test]
    fn params_shape_typeids_round_trip_via_ty() {
        let db = InMemoryDb::new();

        let single = ParamsShape::Single(Arc::from([
            ty_to_typeid(&db, &Ty::Number),
            ty_to_typeid(&db, &Ty::String),
        ]));
        match single {
            ParamsShape::Single(ids) => {
                let via_ty: Vec<Ty> = ids.iter().map(|id| typeid_to_ty(&db, *id)).collect();
                assert_eq!(via_ty, vec![Ty::Number, Ty::String]);
            }
            _ => panic!("expected Single"),
        }

        let overloaded = ParamsShape::Overloaded {
            flat: Arc::from([ty_to_typeid(&db, &Ty::Number)]),
            overloads: Arc::from([Arc::from([ty_to_typeid(&db, &Ty::Number)]) as Arc<[TypeId]>]),
        };
        match overloaded {
            ParamsShape::Overloaded { flat, overloads } => {
                let flat_via_ty: Vec<Ty> = flat.iter().map(|id| typeid_to_ty(&db, *id)).collect();
                assert_eq!(flat_via_ty, vec![Ty::Number]);
                assert_eq!(overloads.len(), 1);
                let row_via_ty: Vec<Ty> =
                    overloads[0].iter().map(|id| typeid_to_ty(&db, *id)).collect();
                assert_eq!(row_via_ty, vec![Ty::Number]);
            }
            _ => panic!("expected Overloaded"),
        }
    }

    #[test]
    fn test_inference_result_default() {
        let result = InferenceResult::default();
        assert_eq!(result.expr_types_by_body.len(), 0);
        assert_eq!(result.diagnostics.len(), 0);
        assert!(!result.has_diagnostics());
    }

    #[test]
    fn test_mismatched_arg_count_diagnostic() {
        // Test that MismatchedArgCount diagnostic is created correctly
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let diag = InferenceDiagnostic::MismatchedArgCount {
            call_expr: expr_id,
            required_count: 2,
            total_count: 2,
            found: 1,
        };

        match diag {
            InferenceDiagnostic::MismatchedArgCount {
                call_expr,
                required_count,
                total_count,
                found,
            } => {
                assert_eq!(call_expr, expr_id);
                assert_eq!(required_count, 2);
                assert_eq!(total_count, 2);
                assert_eq!(found, 1);
            }
            _ => panic!("Expected MismatchedArgCount"),
        }
    }

    #[test]
    fn test_unresolved_method_call_diagnostic() {
        // Test that UnresolvedMethodCall diagnostic is created correctly
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let receiver_name = Name::new("CommonModule");
        let method_name = Name::new("Method");

        let diag = InferenceDiagnostic::UnresolvedMethodCall {
            expr: expr_id,
            receiver_name: receiver_name.clone(),
            method_name: method_name.clone(),
            kind: UnresolvedMethodKind::MethodNotFound,
        };

        match diag {
            InferenceDiagnostic::UnresolvedMethodCall {
                expr,
                receiver_name: r,
                method_name: m,
                kind,
            } => {
                assert_eq!(expr, expr_id);
                assert_eq!(r, receiver_name);
                assert_eq!(m, method_name);
                assert_eq!(kind, UnresolvedMethodKind::MethodNotFound);
            }
            _ => panic!("Expected UnresolvedMethodCall"),
        }
    }

    #[test]
    fn test_type_mismatch_diagnostic() {
        // Test that TypeMismatch diagnostic is created correctly
        let db = InMemoryDb::new();
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let expected_ty = ty_to_typeid(&db, &Ty::Number);
        let actual_ty = ty_to_typeid(&db, &Ty::String);

        let diag = InferenceDiagnostic::TypeMismatch {
            expr: expr_id,
            expected: expected_ty,
            actual: actual_ty,
        };

        match diag {
            InferenceDiagnostic::TypeMismatch { expr, expected, actual } => {
                assert_eq!(expr, expr_id);
                assert_eq!(expected, expected_ty);
                assert_eq!(actual, actual_ty);
            }
            _ => panic!("Expected TypeMismatch"),
        }
    }

    #[test]
    fn test_unresolved_field_diagnostic() {
        // Test that UnresolvedField diagnostic carries the receiver type
        // and field name verbatim, so the ide-diagnostics layer can render
        // `<ReceiverType>.<field_name>` without re-running inference.
        let db = InMemoryDb::new();
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let receiver_ty = ty_to_typeid(
            &db,
            &Ty::MetadataRef {
                kind: hir_def::ty::MetadataKind::CatalogRef,
                name: Name::new("Номенклатура"),
            },
        );
        let field_name = Name::new("НесуществующееПоле");

        let diag = InferenceDiagnostic::UnresolvedField {
            expr: expr_id,
            receiver_ty,
            field_name: field_name.clone(),
        };

        match diag {
            InferenceDiagnostic::UnresolvedField { expr, receiver_ty: r, field_name: f } => {
                assert_eq!(expr, expr_id);
                assert_eq!(r, receiver_ty);
                assert_eq!(f, field_name);
            }
            _ => panic!("Expected UnresolvedField"),
        }
    }

    #[test]
    fn test_inference_result_with_diagnostics() {
        let mut result = InferenceResult::default();
        assert!(!result.has_diagnostics());

        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        result.diagnostics.push((
            DefWithBodyId::ModuleCode,
            InferenceDiagnostic::MismatchedArgCount {
                call_expr: expr_id,
                required_count: 2,
                total_count: 2,
                found: 1,
            },
        ));

        assert!(result.has_diagnostics());
        assert_eq!(result.diagnostics.len(), 1);
    }

    /// `BuiltinFunctions::get` returns a slice (multi-overload aware) — for
    /// the single-overload functions exercised here, the lone signature is
    /// at index 0.
    fn first_sig(
        db: &dyn bsl_types::intern::TypeKernelDb,
        builtins: &builtin::BuiltinFunctions,
        name: &str,
    ) -> FunctionSignature {
        let sigs = builtins.get(name).unwrap_or_else(|| panic!("{name} should exist"));
        sigs[0].lower(db)
    }

    #[test]
    fn test_builtin_function_lookup() {
        // Verify builtin functions are accessible for inference.
        // The actual integration happens in infer_expr() for Expr::Path.
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        // Test that СтрДлина returns Number
        let strlen_sig = first_sig(&db, builtins, "стрдлина");
        assert_eq!(strlen_sig.ret, db.number(None, None));
        assert_eq!(strlen_sig.params.len(), 1);
        assert_eq!(strlen_sig.params[0], db.string(None, false));

        // Test English variant
        let strlen_en = first_sig(&db, builtins, "strlen");
        assert_eq!(strlen_en.ret, db.number(None, None));

        // Test case-insensitive lookup
        let upper_case = builtins.get("СТРДЛИНА");
        assert!(upper_case.is_some(), "Lookup should be case-insensitive");
    }

    #[test]
    fn test_builtin_date_function() {
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        // ТекущаяДата() -> Дата
        let current_date = first_sig(&db, builtins, "текущаядата");
        assert_eq!(current_date.ret, db.date(bsl_types::facet::DateComponent::DateTime));
        assert!(current_date.params.is_empty());

        // Год(Дата) -> Число
        let year = first_sig(&db, builtins, "год");
        assert_eq!(year.ret, db.number(None, None));
        assert_eq!(year.params.len(), 1);
        assert_eq!(year.params[0], db.date(bsl_types::facet::DateComponent::DateTime));
    }

    #[test]
    fn test_builtin_type_function() {
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        // ТипЗнч(Any) -> Type
        let type_of = first_sig(&db, builtins, "типзнч");
        assert_eq!(type_of.ret, db.type_descriptor());
    }

    // ----- Phase O.13 (Lni.1–Lni.3) type-lift unit tests -----

    fn make_expr(idx: u32) -> ExprId {
        ExprId::from_raw(la_arena::RawIdx::from_u32(idx))
    }

    #[test]
    fn body_inference_result_empty_for_preserves_owner() {
        let method_owner = DefWithBodyId::Method(7);
        let method_empty = BodyInferenceResult::empty_for(method_owner);
        assert_eq!(method_empty.owner, method_owner);
        assert!(method_empty.var_types.is_empty());
        assert!(method_empty.implicit_locals.is_empty());
        assert!(method_empty.binding_types.is_empty());
        assert!(method_empty.expr_types.is_empty());
        assert!(method_empty.diagnostics.is_empty());
        assert!(method_empty.call_arg_bindings.is_empty());
        assert!(method_empty.return_expr_ids.is_empty());

        let module_empty = BodyInferenceResult::empty_for(DefWithBodyId::ModuleCode);
        assert_eq!(module_empty.owner, DefWithBodyId::ModuleCode);
    }

    #[test]
    fn module_code_inference_result_default_is_module_owner() {
        let result = ModuleCodeInferenceResult::default();
        assert_eq!(result.owner, DefWithBodyId::ModuleCode);
        assert!(result.var_types.is_empty());
        assert!(result.expr_types.is_empty());
    }

    #[test]
    fn module_code_inference_result_from_body_preserves_fields() {
        // Phase 3 §4.D: result-struct maps store `TypeId`; intern via a
        // sandbox kernel to populate the payload.
        let db = InMemoryDb::new();
        let mut body = BodyInferenceResult::empty_for(DefWithBodyId::ModuleCode);
        body.var_types.insert("х".to_string(), ty_to_typeid(&db, &Ty::String));
        body.expr_types.insert(make_expr(3), ty_to_typeid(&db, &Ty::Boolean));
        body.diagnostics.push(InferenceDiagnostic::TypeMismatch {
            expr: make_expr(4),
            expected: ty_to_typeid(&db, &Ty::String),
            actual: ty_to_typeid(&db, &Ty::Number),
        });

        let lifted = ModuleCodeInferenceResult::from_body(body);

        assert_eq!(lifted.owner, DefWithBodyId::ModuleCode);
        assert_eq!(
            lifted.var_types.get("х").copied().map(|id| typeid_to_ty(&db, id)),
            Some(Ty::String),
        );
        assert_eq!(
            lifted.expr_types.get(&make_expr(3)).copied().map(|id| typeid_to_ty(&db, id)),
            Some(Ty::Boolean),
        );
        assert_eq!(lifted.diagnostics.len(), 1);
    }

    #[test]
    fn infer_owner_result_method_accessors_route_to_method_payload() {
        let db = InMemoryDb::new();
        let mut body = BodyInferenceResult::empty_for(DefWithBodyId::Method(2));
        body.var_types.insert("х".to_string(), ty_to_typeid(&db, &Ty::Number));
        body.expr_types.insert(make_expr(5), ty_to_typeid(&db, &Ty::String));

        let routed = InferOwnerResult::Method(Arc::new(body));

        assert_eq!(routed.owner(), DefWithBodyId::Method(2));
        assert_eq!(routed.type_of_expr(&db, make_expr(5)), Some(Ty::String));
        assert_eq!(routed.type_of_expr(&db, make_expr(99)), None);
        assert_eq!(
            routed.var_types().get("х").copied().map(|id| typeid_to_ty(&db, id)),
            Some(Ty::Number),
        );
        assert!(routed.implicit_locals().is_empty());
        assert!(routed.binding_types().is_empty());
    }

    #[test]
    fn infer_owner_result_module_code_accessors_route_to_module_payload() {
        let db = InMemoryDb::new();
        let mut mc = ModuleCodeInferenceResult::default();
        mc.var_types.insert("у".to_string(), ty_to_typeid(&db, &Ty::Boolean));
        mc.expr_types.insert(make_expr(1), ty_to_typeid(&db, &Ty::Date));

        let routed = InferOwnerResult::ModuleCode(Arc::new(mc));

        assert_eq!(routed.owner(), DefWithBodyId::ModuleCode);
        assert_eq!(routed.type_of_expr(&db, make_expr(1)), Some(Ty::Date));
        assert_eq!(
            routed.var_types().get("у").copied().map(|id| typeid_to_ty(&db, id)),
            Some(Ty::Boolean),
        );
    }
}
