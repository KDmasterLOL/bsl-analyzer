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

use cfg_types::IdConversion;
use hir_def::body::Body;
use hir_def::hir::{BinaryOp, Expr, Literal, Stmt, StmtIdx, UnaryOp};
use hir_def::resolver::Resolver;
use hir_def::ty::Ty;
use hir_def::{DefWithBodyId, ExprId, Name};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
use crate::method_resolution;
use crate::platform_manager_lookup::{resolve_platform_manager_method, PlatformMethodResolution};

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
    /// map = that body's `ExprId -> Ty`.
    ///
    /// Shape note: M3 ships this as a nested map, not a flat
    /// `FxHashMap<(DefWithBodyId, ExprId), Ty>`. No current caller uses
    /// the "grab the whole body's map in one lookup" shortcut the nesting
    /// would enable (`Semantics::type_of_expr` does a single point
    /// lookup), but the nested shape stays open for Tasks 10-12 hooks
    /// (body-scoped completion, narrowing) that need per-body iteration.
    /// If those never materialise, a later cleanup can flatten.
    pub expr_types_by_body: FxHashMap<DefWithBodyId, FxHashMap<ExprId, Ty>>,

    /// Variable types inferred from assignments.
    ///
    /// Maps lowercase variable name to its last inferred type.
    /// Populated by tracking `Stmt::Assign { target: Path(name), value }` during inference.
    /// Used by completion to resolve receiver types for method lookup.
    pub var_types: FxHashMap<String, Ty>,

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

impl InferenceResult {
    /// Get the type of an expression in a specific body.
    ///
    /// `owner` identifies the body — `DefWithBodyId::Method(local_id)`
    /// for a procedure / function, `DefWithBodyId::ModuleCode` for
    /// module-level code. Returns `None` if inference produced no entry
    /// for that `(owner, expr)` pair.
    pub fn type_of_expr_in(&self, owner: DefWithBodyId, expr: ExprId) -> Option<&Ty> {
        self.expr_types_by_body.get(&owner)?.get(&expr)
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
    TypeMismatch { expr: ExprId, expected: Ty, actual: Ty },

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
    UnresolvedField { expr: ExprId, receiver_ty: Ty, field_name: Name },

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
    ReadOnlyPropertyAssignment { lhs: ExprId, receiver_ty: Ty, field_name: Name },
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
/// `Arc<[Ty]>` is used (instead of `Vec<Ty>`) so that the same signature
/// can be shared across many call sites of the same method without
/// re-allocating the parameter list per record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamsShape {
    /// Single parameter list — covers plain functions, workspace
    /// `CommonModule.Method`, three-segment manager calls,
    /// non-overloaded receiver methods, and `Ty::Function` callees.
    Single(Arc<[Ty]>),

    /// Multi-overload signature.
    ///
    /// `flat` is the legacy "flattened" parameter list used as a
    /// fallback when the overload set is empty (mirrors the old
    /// `emit_arg_type_mismatches_overloaded` contract). `overloads`
    /// carries one entry per declared overload — validation accepts
    /// the call when any entry's per-arg `is_assignable` check passes,
    /// and falls back to the closest-by-arity overload for the
    /// diagnostic message otherwise.
    Overloaded { flat: Arc<[Ty]>, overloads: Arc<[Arc<[Ty]>]> },
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

    /// Variable types tracked from assignments (lowercase name → Ty).
    var_types: FxHashMap<String, Ty>,

    /// Per-body `ExprId -> Ty` cache. Doubles as the memoisation table
    /// (`infer_expr` short-circuits when the entry already exists) and
    /// as the payload we hand to the merged result keyed by `owner`.
    expr_types: FxHashMap<ExprId, Ty>,

    /// Diagnostics collected while inferring this body.
    diagnostics: Vec<InferenceDiagnostic>,

    /// Call-site arg/param bindings recorded during inference for the
    /// downstream narrowing-aware
    /// [`crate::arg_diagnostics::arg_diagnostics_query`].
    call_arg_bindings: Vec<CallArgBinding>,
}

/// Single-body inference output.
///
/// Intermediate record returned from [`InferenceContext::finish`]: keeps
/// the body's expr-type map and diagnostics separate from the variable
/// map so `infer_query` can fold them into the file-level
/// [`InferenceResult`] without re-walking anything.
pub struct BodyInferenceResult {
    /// Owner of the body that produced this output.
    pub owner: DefWithBodyId,
    /// Variable types discovered during inference (lowercase name → Ty).
    pub var_types: FxHashMap<String, Ty>,
    /// Expression types keyed by body-local `ExprId`.
    pub expr_types: FxHashMap<ExprId, Ty>,
    /// Diagnostics collected during inference.
    pub diagnostics: Vec<InferenceDiagnostic>,
    /// Call-site arg/param bindings collected during inference, to be
    /// folded into [`InferenceResult::call_arg_bindings`].
    pub call_arg_bindings: Vec<CallArgBinding>,
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
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
        }
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
        };
        if self.body.is_recovered(key) {
            return;
        }
        self.diagnostics.push(diag);
    }

    /// Finish inference and return the per-body output.
    pub fn finish(self) -> BodyInferenceResult {
        BodyInferenceResult {
            owner: self.owner,
            var_types: self.var_types,
            expr_types: self.expr_types,
            diagnostics: self.diagnostics,
            call_arg_bindings: self.call_arg_bindings,
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
                        let user_shadows = matches!(
                            resolver.resolve_name(self.db, name),
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
                                    let form_ty = Ty::PlatformObject(hir_def::Name::new(
                                        crate::form_self::FORM_TYPE_NAME,
                                    ));
                                    self.push_inference_diagnostic(
                                        InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                            lhs: ExprId::from_idx(*target),
                                            receiver_ty: form_ty,
                                            field_name: name.clone(),
                                        },
                                    );
                                }
                            }
                            None => {
                                if !value_ty.is_unknown() {
                                    self.var_types.insert(name.as_str().to_lowercase(), value_ty);
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
                        if let Some(info) =
                            crate::field_lookup::lookup_field(&configs, &base_ty, field)
                        {
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

            Stmt::For { from, to, body, .. } => {
                self.infer_expr(ExprId::from_idx(*from));
                self.infer_expr(ExprId::from_idx(*to));
                self.infer_stmts(body);
            }

            Stmt::ForEach { collection, body, .. } => {
                self.infer_expr(ExprId::from_idx(*collection));
                self.infer_stmts(body);
            }

            Stmt::Try { body, except } => {
                self.infer_stmts(body);
                self.infer_stmts(except);
            }

            Stmt::Return { value } => {
                if let Some(expr_idx) = value {
                    self.infer_expr(ExprId::from_idx(*expr_idx));
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

    /// Infer the type of an expression.
    ///
    /// This is the core type inference function. It pattern-matches on the expression
    /// kind and dispatches to specialized inference functions.
    fn infer_expr(&mut self, expr_id: ExprId) -> Ty {
        // Check if already inferred (avoid re-inference)
        if let Some(ty) = self.expr_types.get(&expr_id) {
            return ty.clone();
        }

        // Clone the expression to avoid borrow checker issues
        // (we need &mut self for recursive infer_expr calls)
        let expr = self.body.expr(expr_id).clone();
        trace!("inferring expr {:?}: {:?}", expr_id, expr);

        let ty = match &expr {
            Expr::Missing => Ty::Unknown,

            Expr::Literal(lit) => self.infer_literal(lit),

            Expr::Path(name) => self.infer_path_name(name),

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
                Ty::Unknown
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
                    Ty::Unknown
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
                crate::method_lookup::lookup_method(&receiver_ty, method)
                    .map(|info| info.return_ty)
                    .unwrap_or(Ty::Unknown)
            }

            Expr::Index { base, index } => {
                self.infer_expr(ExprId::from_idx(*base));
                self.infer_expr(ExprId::from_idx(*index));

                // Phase 1: Return Unknown
                // Phase 2+: Could infer element type for arrays
                Ty::Unknown
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
                if let Some(info) = crate::field_lookup::lookup_field(&configs, &base_ty, field) {
                    info.ty
                } else if let Some(info) =
                    crate::manager_lookup::lookup_manager_field(&configs, &base_ty, field)
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
                    if matches!(base_ty, Ty::MetadataRef { .. } | Ty::ThisObject { .. }) {
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedField {
                            expr: expr_id,
                            receiver_ty: base_ty,
                            field_name: field.clone(),
                        });
                    }
                    Ty::Unknown
                }
            }

            Expr::New { type_name, args } => {
                // Infer arguments
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                // Lower the constructor name through the shared TypeRef →
                // Ty adapter. The cascade (builtin → MDO plural → platform
                // object fallback) moved into `lower_bare_name`, so every
                // syntactic source of type info (`Новый X`, `Тип("…")`,
                // JSDoc) now takes the same path — editing the fallback
                // rules in one place is enough.
                match type_name {
                    Some(name) => TyLoweringContext::new().lower_bare_name(name),
                    None => Ty::Unknown,
                }
            }

            Expr::Array(elements) => {
                // Infer element types
                for &elem in elements.iter() {
                    self.infer_expr(ExprId::from_idx(elem));
                }

                Ty::Array
            }

            Expr::Await { expr } => {
                // BSL Await returns the same type as the awaited expression
                self.infer_expr(ExprId::from_idx(*expr))
            }
        };

        // Store the inferred type
        self.expr_types.insert(expr_id, ty.clone());
        ty
    }

    /// Resolve a bare `Expr::Path` identifier to a [`Ty`].
    ///
    /// Lookup order mirrors BSL visibility:
    ///
    /// 1. **Platform builtins** — acknowledged by either
    ///    [`Resolver::resolve_name`] (via the `Scope::Builtins` port into
    ///    `bsl_platform`) **or** by the hand-curated `hir-ty::builtin`
    ///    signature table. Either source is enough: the platform index
    ///    covers more names, but the `hir-ty::builtin` table carries the
    ///    only typed signatures today and includes constructor-like
    ///    globals (`Новый`, `ПустоеЗначение`, `ОписаниеТипов`, `Выполнить`,
    ///    …) that are absent from the platform global-function index.
    ///    Builtins are never shadowed by user code.
    /// 2. **Implicit locals** — BSL has no explicit `Var` declarations;
    ///    a name springs into existence at its first assignment. The
    ///    inference context captures those types in [`Self::var_types`]
    ///    as [`Stmt::Assign`] is walked in [`Self::infer_stmts`].
    ///    Implicit locals *do* shadow module-level names, so `var_types`
    ///    is checked before the module/variable Resolver branches.
    /// 3. **Module-level methods / variables** — returned as `Unknown`
    ///    today (no signature carrier yet); Task 2.x will synthesise
    ///    `Ty::Function` from `MethodId`.
    fn infer_path_name(&mut self, name: &hir_def::Name) -> Ty {
        use hir_def::resolver::Resolution;

        let resolver = self.get_resolver();

        // 0. `ЭтотОбъект` / `ThisObject` — intercepted ahead of every
        //    other scope because BSL treats the identifier like a
        //    platform global: not shadowable, resolved through module
        //    metadata (the enclosing MDO) rather than the scope chain.
        //    `Resolver::resolve_this_object` returns `None` for module
        //    kinds that have no `*Object` companion (forms, record
        //    sets, manager / common / command modules) — those fall
        //    through to the normal cascade and become `Ty::Unknown`,
        //    matching the "best-effort" inference semantics.
        let name_lower = name.as_str().to_lowercase();
        if name_lower == "этотобъект" || name_lower == "thisobject" {
            if let Some(owner) = resolver.resolve_this_object(self.db) {
                trace!("resolved {} as ThisObject {{ owner: {:?} }}", name, owner);
                return Ty::ThisObject { owner };
            }
            // Managed-form fallback: `ЭтотОбъект` in a managed form module
            // names the form itself. There is no `MdoType` companion (forms
            // are outside the catalog/document/exchange-plan/CoA axis), so
            // we don't go through `Ty::ThisObject` — we hand back the
            // platform type directly. Subsequent `.Элементы` / `.Найти(…)`
            // chains then route through the existing platform-property /
            // platform-method adapters with no special-case code.
            if resolver.resolve_this_form(self.db) {
                trace!("resolved {} as managed form Self", name);
                return Ty::PlatformObject(hir_def::Name::new(crate::form_self::FORM_TYPE_NAME));
            }
            // Fall through: record-set / ordinary-form / common-module
            // `ЭтотОбъект` stays Unknown for now (M4 Task 5 follow-up).
        }

        let resolved = resolver.resolve_name(self.db, name);

        // 1. Builtins — union of Resolver's platform-global view and the
        //    narrower hir-ty signature table. BSL has no first-class
        //    function values: a bare identifier `СтрокаСоединения…`
        //    without parentheses cannot evaluate to a function — the
        //    only way to invoke a builtin is `Name(...)`, handled by
        //    `infer_call`'s `Expr::Path` callee branch which looks up
        //    the signature directly. So at value position we collapse
        //    the builtin hit to `Ty::Unknown`; otherwise a parameter
        //    or local variable that happens to share its name with a
        //    platform global (e.g. the user-declared
        //    `Знач СтрокаСоединенияИнформационнойБазы = ""`) would
        //    false-fire `TypeMismatch { expected: String, actual:
        //    Function }` when read as an argument.
        let resolver_says_builtin = matches!(resolved, Some(Resolution::Builtin(_)));
        let hir_sig = builtin::builtin_functions().get(name.as_str());
        if resolver_says_builtin || hir_sig.is_some() {
            return Ty::Unknown;
        }

        // 2. BSL implicit locals shadow module-level and manager names.
        //    A user writing `Документы = 42;` rebinds the identifier in the
        //    local scope — the manager collective is only visible if no
        //    local assignment exists.
        if let Some(ty) = self.var_types.get(&name.as_str().to_lowercase()) {
            trace!("resolved {} via var_types = {:?}", name, ty);
            return ty.clone();
        }

        // 3. MDO plural globals (`Документы`, `Справочники`, …) lower into
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
            if let Some(ty) = Ty::manager_collection(mdo_type) {
                trace!("resolved {} as manager collection {:?}", name, mdo_type);
                return ty;
            }
        }

        // Shared shadowing rule for steps 4 and 5: a module-level `Метод()`
        // or `Перем` declaration with this name takes priority over any
        // platform / form-self property. We hoist the predicate so both
        // steps consult the same binding.
        let user_shadows =
            matches!(resolved, Some(Resolution::Method(_)) | Some(Resolution::Variable(_)));

        // 4. Managed-form Self property — inside a managed-form module,
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
        if !user_shadows && !self.body_declares_binding(name) {
            if let Some(resolution) =
                crate::form_self::resolve_form_self_property(self.db, &resolver, name)
            {
                trace!("resolved {} as managed-form Self property", name);
                return resolution.return_ty;
            }
        }

        // 5. Platform global-context properties — top-level identifiers
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
        // sees the name as a module-level method or variable. The user has
        // shadowed the platform global (e.g. `Процедура ОбработкаОшибок()
        // Экспорт`); BSL semantics give the local definition priority and
        // we must not silently retype a reference to it as `PlatformObject`.
        if !user_shadows {
            if let Some(prop) =
                bsl_platform::PlatformDataInner::instance().get_global_property(name.as_str())
            {
                if let Some(declared) = prop.property_types.first() {
                    trace!("resolved {} as platform global → {}", name, declared);
                    let lowering = crate::lower::TyLoweringContext::new();
                    return lowering.lower_bare_name(&hir_def::Name::new(declared.as_str()));
                }
            }
        }

        // 6. Module-level methods / variables (Unknown today; Task 2.x
        //    will synthesise Ty::Function from MethodId).
        match resolved {
            Some(Resolution::Method(_)) | Some(Resolution::Variable(_)) => Ty::Unknown,
            // `Local` is unreachable here because `get_resolver` does not
            // push an ExprScope; any local-looking name already returned
            // from the `var_types` branch above.
            Some(Resolution::Builtin(_)) | Some(Resolution::Local(_)) | None => Ty::Unknown,
        }
    }

    /// Infer type from a literal.
    fn infer_literal(&self, lit: &Literal) -> Ty {
        match lit {
            Literal::Number(_) => Ty::Number,
            Literal::String(_) => Ty::String,
            Literal::Date(_) => Ty::Date,
            Literal::Bool(_) => Ty::Boolean,
            Literal::Undefined => Ty::Undefined,
            Literal::Null => Ty::Null,
        }
    }

    /// Infer type from a binary operation.
    fn infer_binary_op(&mut self, lhs: ExprId, rhs: ExprId, op: BinaryOp) -> Ty {
        let lhs_ty = self.infer_expr(lhs);
        let rhs_ty = self.infer_expr(rhs);

        match op {
            // Arithmetic operations: Number op Number → Number
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                // Special case: String + Any → String (concatenation)
                if op == BinaryOp::Add && (lhs_ty == Ty::String || rhs_ty == Ty::String) {
                    Ty::String
                } else if lhs_ty == Ty::Number && rhs_ty == Ty::Number {
                    Ty::Number
                } else {
                    // Unknown operand types
                    Ty::Unknown
                }
            }

            // Comparison operations: Any op Any → Boolean
            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => Ty::Boolean,

            // Logical operations: Boolean op Boolean → Boolean
            BinaryOp::And | BinaryOp::Or => Ty::Boolean,
        }
    }

    /// Infer type from a unary operation.
    fn infer_unary_op(&mut self, expr: ExprId, op: UnaryOp) -> Ty {
        let expr_ty = self.infer_expr(expr);

        match op {
            UnaryOp::Neg | UnaryOp::Plus => {
                // Numeric negation/plus
                if expr_ty == Ty::Number {
                    Ty::Number
                } else {
                    Ty::Unknown
                }
            }
            UnaryOp::Not => {
                // Logical NOT
                Ty::Boolean
            }
        }
    }

    /// Infer type from a function call.
    fn infer_call(&mut self, callee: ExprId, args: &[ExprId]) -> Ty {
        // Qualified callees dispatch by segment count:
        //   2 → `CommonModule.Method()`  → resolve_qualified_call
        //   3 → `Документы.ПКО.Метод()` → resolve_three_level_call
        // Everything else falls through to the generic function-type path.
        let callee_expr = self.body.expr(callee);
        if let Expr::QualifiedPath(qualified_path) = callee_expr {
            match qualified_path.segments().len() {
                2 => {
                    let module_name = qualified_path.first().clone();
                    let method_name = qualified_path.last().clone();

                    // Managed-form Self property method-call dispatch.
                    // HIR lowering (`analyze_qualified_call`) cannot
                    // distinguish a CommonModule call (`М.Метод()`) from a
                    // method on a managed-form Self property
                    // (`Элементы.Найти()`) — both lower to a 2-segment
                    // `QualifiedPath` because lowering has no `db`. So the
                    // disambiguation runs here in Phase B: if the first
                    // segment names a property of `ФормаКлиентскогоПриложения`
                    // **and** the enclosing module is a managed form
                    // **and** the receiver name is NOT also a real user
                    // CommonModule (a user `Метаданные` CommonModule
                    // shadowing the platform global must keep its
                    // CommonModule semantics — same precedence rule as
                    // `missing_common_module_method::from_hir`), the call
                    // is `<property>.<method>` and routes through the
                    // platform `lookup_method` adapter on the property's
                    // declared `Ty`. Otherwise it falls through to the
                    // CommonModule resolver as before.
                    let resolver = self.get_resolver();
                    // Full shadowing gate, mirroring `infer_path_name`'s
                    // step 4 and the assignment handler:
                    //   * `user_common_module_exists` — visibility-aware
                    //     CommonModule precedence (a raw `module_index`
                    //     probe would falsely shadow a config-hidden
                    //     module and silently miss when the CommonModule
                    //     path can't see the receiver);
                    //   * module-level `Метод()` / `Перем` declared in
                    //     the current module overrides the form-self
                    //     property (BSL: explicit user declaration wins
                    //     over implicit Self);
                    //   * any parameter or `Перем` declared in this body
                    //     covers the gap left by `Resolver::resolve_name`
                    //     (no ExprScope) and `var_types` (no entry until
                    //     first assign).
                    // Lowering already drops `local_vars`/`param_names`
                    // before classifying as `QualifiedPath`, but the
                    // module-level / declared-binding checks here close
                    // the cases lowering cannot see.
                    let module_level_shadow = matches!(
                        resolver.resolve_name(self.db, &module_name),
                        Some(hir_def::resolver::Resolution::Method(_))
                            | Some(hir_def::resolver::Resolution::Variable(_))
                    );
                    let body_shadow = self.body_declares_binding(&module_name);
                    if !resolver.user_common_module_exists(self.db, &module_name)
                        && !module_level_shadow
                        && !body_shadow
                    {
                        if let Some(prop_resolution) = crate::form_self::resolve_form_self_property(
                            self.db,
                            &resolver,
                            &module_name,
                        ) {
                            for arg in args {
                                self.infer_expr(*arg);
                            }
                            let receiver_ty = prop_resolution.return_ty;
                            if let Some(info) =
                                crate::method_lookup::lookup_method(&receiver_ty, &method_name)
                            {
                                // Argument binding mirrors the
                                // `Expr::Field`-callee platform path on
                                // line ~1453: without it, narrow / arg
                                // diagnostics silently drop for
                                // form-self method calls.
                                self.record_call_arg_binding(
                                    callee,
                                    args,
                                    ParamsShape::Overloaded {
                                        flat: Arc::<[Ty]>::from(&info.params[..]),
                                        overloads: info
                                            .overloads
                                            .iter()
                                            .map(|ov| Arc::<[Ty]>::from(&ov[..]))
                                            .collect::<Vec<Arc<[Ty]>>>()
                                            .into(),
                                    },
                                );
                                trace!(
                                    "resolved form-self call {}.{} as {:?}",
                                    module_name,
                                    method_name,
                                    info.return_ty
                                );
                                return info.return_ty;
                            }
                            // Form-self property exists, but the method
                            // is missing on its type — silent for now
                            // (mirrors the Authoritative-receiver gate
                            // in the Expr::Field path: we don't fire
                            // UnresolvedMethodCall here so the user
                            // doesn't see two error sources for one
                            // misspelled member name).
                            return Ty::Unknown;
                        }
                    }

                    return self.infer_qualified_call(&module_name, &method_name, args, callee);
                }
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
                _ => {}
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
            let workspace_receiver_ty = crate::this_object::coerce_to_metadata_ref(&receiver_ty)
                .unwrap_or_else(|| receiver_ty.clone());
            if let Ty::MetadataRef { kind, name: mdo_name } = &workspace_receiver_ty {
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_object_module_call(
                    self.db,
                    *kind,
                    mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(&workspace_receiver_ty)
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
                            ParamsShape::Single(Arc::<[Ty]>::from(
                                &resolution.signature.params[..],
                            )),
                        );
                        self.expr_types.insert(callee, Ty::Unknown);
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
            if let Ty::MetadataRef { kind, name: mdo_name } = &receiver_ty {
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_record_set_module_call(
                    self.db,
                    *kind,
                    mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name =
                            receiver_display_name(&receiver_ty).unwrap_or_else(|| mdo_name.clone());
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
                            ParamsShape::Single(Arc::<[Ty]>::from(
                                &resolution.signature.params[..],
                            )),
                        );
                        self.expr_types.insert(callee, Ty::Unknown);
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
            if let Ty::ObjectManager { kind: mdo_type, name: mdo_name } = &receiver_ty {
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_aliased_manager_call(
                    self.db,
                    *mdo_type,
                    mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name =
                            receiver_display_name(&receiver_ty).unwrap_or_else(|| mdo_name.clone());
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
                            ParamsShape::Single(Arc::<[Ty]>::from(
                                &resolution.signature.params[..],
                            )),
                        );
                        self.expr_types.insert(callee, Ty::Unknown);
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

            let result = match crate::method_lookup::lookup_method(&receiver_ty, &method_name) {
                Some(mut info) => {
                    // Argument type check (M4 Task 7 follow-up): the
                    // fluent-chain path historically skipped both arg-
                    // count and arg-type diagnostics. Emit the type
                    // check here — count-check stays deferred so this
                    // patch stays scoped to the TypeMismatch emitter.
                    if let Ty::ObjectManager {
                        kind: bsl_metadata::MdoType::Constant,
                        name: mdo_name,
                    } = &receiver_ty
                    {
                        self.refine_constant_method(
                            mdo_name,
                            &method_name,
                            &mut info.return_ty,
                            &mut info.params,
                        );
                    }
                    self.record_call_arg_binding(
                        callee,
                        args,
                        ParamsShape::Overloaded {
                            flat: Arc::<[Ty]>::from(&info.params[..]),
                            overloads: info
                                .overloads
                                .iter()
                                .map(|ov| Arc::<[Ty]>::from(&ov[..]))
                                .collect::<Vec<Arc<[Ty]>>>()
                                .into(),
                        },
                    );
                    info.return_ty
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
                    if let Some(receiver_name) = receiver_display_name(&receiver_ty) {
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                            expr: callee,
                            receiver_name,
                            method_name: method_name.clone(),
                            kind: UnresolvedMethodKind::MethodNotFound,
                        });
                    }
                    Ty::Unknown
                }
            };
            // Cache the callee `Expr::Field`'s type so `infer_all`'s
            // second pass (which iterates every expression in the body)
            // does not re-visit it through the `Expr::Field` arm of
            // `infer_expr` and emit a spurious `UnresolvedField` on a
            // method name. BSL has no first-class method references —
            // the meaningful value type belongs to the surrounding
            // `Expr::Call`, which is cached at the call site.
            self.expr_types.insert(callee, Ty::Unknown);
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

                for arg in args {
                    self.infer_expr(*arg);
                }

                let arg_count = args.len();
                let inferred: Vec<Ty> = args
                    .iter()
                    .map(|a| self.expr_types.get(a).cloned().unwrap_or(Ty::Unknown))
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
                    let required = sig
                        .defaults
                        .iter()
                        .rposition(|has_default| !*has_default)
                        .map_or(0, |i| i + 1);
                    let too_few = arg_count < required;
                    let too_many = sig.max_args.is_some_and(|m| arg_count > m as usize);
                    if !too_few && !too_many {
                        if arity_match.is_none() {
                            arity_match = Some(idx);
                        }
                        // Type check: zip arg types against this overload's
                        // params; `is_assignable` treats `Ty::Unknown` on
                        // either side as permissive.
                        let types_ok =
                            inferred.iter().zip(sig.params.iter()).all(|(actual, expected)| {
                                crate::subtype::is_assignable(actual, expected)
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
                        let required = sig
                            .defaults
                            .iter()
                            .rposition(|has_default| !*has_default)
                            .map_or(0, |i| i + 1);
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
                let overloads_arc: Arc<[Arc<[Ty]>]> = sigs
                    .iter()
                    .map(|s| Arc::<[Ty]>::from(&s.params[..]))
                    .collect::<Vec<_>>()
                    .into();
                self.record_call_arg_binding(
                    callee,
                    args,
                    ParamsShape::Overloaded {
                        flat: Arc::<[Ty]>::from(&chosen.params[..]),
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
                    (*chosen.ret).clone()
                } else {
                    Ty::union(sigs.iter().map(|s| (*s.ret).clone()).collect())
                };
                // Pin the bare callee's cached type so the second pass
                // in `infer_all` doesn't re-enter `Expr::Path` on the
                // function-name token. Mirrors the `Expr::Field`
                // callee fix above.
                self.expr_types.insert(callee, Ty::Unknown);
                return ret;
            }
        }

        // Infer callee type for non-qualified calls
        let callee_ty = self.infer_expr(callee);

        // Infer argument types
        for arg in args {
            self.infer_expr(*arg);
        }

        // Check if callee is a function type
        match callee_ty {
            Ty::Function { ref params, ref defaults, max_args, ref ret } => {
                // Arity check honours per-parameter defaults and the
                // documented `max_args` cap. The lower bound is the count
                // of required (non-default) leading parameters. The upper
                // bound is `max_args` (e.g. `Some(11)` for `СтрШаблон`,
                // `Some(2)` for `НСтр`, `None` for genuinely unbounded
                // variadics like the `ОписаниеТипов` fallback).
                let total = params.len();
                let required =
                    defaults.iter().rposition(|has_default| !*has_default).map_or(0, |i| i + 1);
                let too_few = args.len() < required;
                let too_many = max_args.is_some_and(|m| args.len() > m as usize);
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
                    ParamsShape::Single(Arc::<[Ty]>::from(&params[..])),
                );

                // Return function's return type
                (**ret).clone()
            }
            Ty::Unknown => {
                // Phase 2: Resolve built-in functions
                // Phase 3: Resolve user-defined functions via SymbolTree
                Ty::Unknown
            }
            _ => {
                // Callee is not a function type
                // Phase 2+: Could emit diagnostic here
                Ty::Unknown
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
    ) -> Ty {
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
                    ParamsShape::Single(Arc::<[Ty]>::from(&resolution.signature.params[..])),
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
                        if let Some(method) = bsl_platform::PlatformDataInner::instance()
                            .resolve_global_member(module_name.as_str(), method_name.as_str())
                        {
                            let return_ty = method
                                .return_type
                                .as_ref()
                                .map(|s| {
                                    let lowering = crate::lower::TyLoweringContext::new();
                                    lowering.lower_bare_name(&hir_def::Name::new(s.as_str()))
                                })
                                .unwrap_or(Ty::Unknown);
                            self.expr_types.insert(call_expr, Ty::Unknown);
                            return return_ty;
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

                Ty::Unknown
            }
        }
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
    ) -> Ty {
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
                    ParamsShape::Single(Arc::<[Ty]>::from(&resolution.signature.params[..])),
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
                        self.refine_constant_method(
                            mdo_name,
                            method_name,
                            &mut res.return_ty,
                            &mut res.signature.params,
                        );
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
                        ParamsShape::Single(Arc::<[Ty]>::from(&res.signature.params[..])),
                    );
                    return res.return_ty;
                }

                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind: UnresolvedMethodKind::MethodNotFound,
                });
                Ty::Unknown
            }
            Err(kind) => {
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind,
                });
                Ty::Unknown
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
    fn resolve_constant_value_type(&self, mdo_name: &Name) -> Option<Ty> {
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
                TyLoweringContext::new().lower_type_ref(&type_ref)
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
        return_ty: &mut Ty,
        params: &mut [Ty],
    ) {
        let lc = method_name.as_str().to_lowercase();
        let is_get = lc == "получить" || lc == "get";
        let is_set = lc == "установить" || lc == "set";
        if !is_get && !is_set {
            return;
        }
        let needs_override = (is_get && matches!(return_ty, Ty::Unknown))
            || (is_set && matches!(params.first(), Some(Ty::Unknown)));
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
/// agree.
///
/// The user-visible form matches the 3-segment path's `<Plural>.<MDO>`
/// convention rendered by `unresolved_method_call::from_hir`.
fn receiver_display_name(receiver_ty: &Ty) -> Option<hir_def::Name> {
    match receiver_ty {
        Ty::MetadataRef { kind, name } => {
            let plural = mdo_kind_to_plural(*kind)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, name.as_str())))
        }
        Ty::ThisObject { owner: (mdo_type, name) } => {
            let plural = mdo_type_to_plural(*mdo_type)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, name.as_str())))
        }
        Ty::ObjectManager { kind: mdo_type, name } => {
            let plural = mdo_type_to_plural(*mdo_type)?;
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
        MetadataKind::TaskRef => bsl_metadata::MdoType::Task,
        MetadataKind::BusinessProcessRef => bsl_metadata::MdoType::BusinessProcess,
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
        MetadataKind::InformationRegisterRecordManager => {
            bsl_metadata::MdoType::InformationRegister
        }
        MetadataKind::AccumulationRegisterRecordSet => bsl_metadata::MdoType::AccumulationRegister,
        // `*RegisterRef` value kinds: no module-level call surface
        // (no `RecordSetModule.bsl` for the *Ref form), no platform
        // surface. Silence is the honest answer.
        MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef => return None,
        // Tabular-section kinds: `lookup_method` resolves their methods
        // through `PlatformData["Tabular section"]` and field_lookup
        // resolves their row properties through
        // `PlatformData["Line of a tabular section"]`, so a miss is
        // authoritative — surface it as `<Plural>.<MdoName>.<Section>`
        // (the section name is already encoded in `MetadataRef::name`).
        MetadataKind::TabularSection { parent } | MetadataKind::TabularSectionRow { parent } => {
            parent
        }
        // Register-part kinds carry a parent payload but no manager-style
        // call surface. Returning `None` keeps the diagnostic silent on
        // these — `lookup_method` itself returns `None`, so we never
        // construct a misleading `Регистры…/<Раздел>` receiver name.
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. } => return None,
    };
    mdo_type_to_plural(mdo)
}

/// Salsa query: Infer types for all expressions in a file.
///
/// This is the main entry point for type inference. It:
/// 1. Gets the HIR bodies for the file via module_bodies query
/// 2. Creates an InferenceContext for each body
/// 3. Runs inference on all expressions
/// 4. Returns the cached result
///
/// # Caching
///
/// Results are cached by Salsa. The query is invalidated when:
/// - The file content changes (via parse query)
/// - Dependencies change (via module_bodies query)
pub fn infer_query(db: &dyn HirDatabase, file_id: FileId) -> Arc<InferenceResult> {
    let _p = tracing::info_span!("infer_query", ?file_id).entered();

    // Get HIR bodies from DefDatabase
    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

    let mut result = InferenceResult::default();

    let fold_body = |result: &mut InferenceResult, body_result: BodyInferenceResult| {
        // Preserve per-body expr_types so `Semantics::type_of_expr`
        // (M3 Task 9) can look them up via `BodySourceMap`. Before this,
        // the merge dropped `expr_types` entirely and every syntax-node
        // lookup returned `Ty::Unknown`.
        result.expr_types_by_body.insert(body_result.owner, body_result.expr_types);
        // `var_types` stays file-global: completion matches variables by name
        // across bodies. `diagnostics` is flat file-wide but each entry is
        // paired with its `DefWithBodyId` owner so ide-diagnostics can resolve
        // the body-local `ExprId` through the correct `BodySourceMap`.
        result.var_types.extend(body_result.var_types);
        let owner = body_result.owner;
        result.diagnostics.extend(body_result.diagnostics.into_iter().map(|d| (owner, d)));
        // Call-site arg bindings carry their own owner field, so we just
        // append. `arg_diagnostics_query` consumes the file-wide list.
        result.call_arg_bindings.extend(body_result.call_arg_bindings);
    };

    // Infer module-level code (statements outside procedures/functions)
    if let Some(body) = module_bodies.module_code() {
        let mut ctx =
            InferenceContext::new(db, file_id, DefWithBodyId::ModuleCode, &Arc::new(body.clone()));
        ctx.infer_all();
        fold_body(&mut result, ctx.finish());
    }

    // Infer all method bodies (procedures and functions)
    for (local_id, body) in module_bodies.iter_bodies() {
        let mut ctx = InferenceContext::new(
            db,
            file_id,
            DefWithBodyId::Method(local_id),
            &Arc::new(body.clone()),
        );
        ctx.infer_all();
        fold_body(&mut result, ctx.finish());
    }

    info!(
        "type inference complete: {} bodies, {} var types, {} diagnostics",
        result.expr_types_by_body.len(),
        result.var_types.len(),
        result.diagnostics.len()
    );

    Arc::new(result)
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
/// - The inferred type for `(owner, expr)` if present.
/// - `Ty::Unknown` if inference produced no entry for that pair.
pub fn type_of_expr_query(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    expr: ExprId,
) -> Ty {
    let infer = db.infer(file_id);
    infer.type_of_expr_in(owner, expr).cloned().unwrap_or(Ty::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let expected_ty = Ty::Number;
        let actual_ty = Ty::String;

        let diag = InferenceDiagnostic::TypeMismatch {
            expr: expr_id,
            expected: expected_ty.clone(),
            actual: actual_ty.clone(),
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
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let receiver_ty = Ty::MetadataRef {
            kind: hir_def::ty::MetadataKind::CatalogRef,
            name: Name::new("Номенклатура"),
        };
        let field_name = Name::new("НесуществующееПоле");

        let diag = InferenceDiagnostic::UnresolvedField {
            expr: expr_id,
            receiver_ty: receiver_ty.clone(),
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
    fn first_sig<'a>(
        builtins: &'a builtin::BuiltinFunctions,
        name: &str,
    ) -> &'a hir_def::ty::FunctionSignature {
        let sigs = builtins.get(name).unwrap_or_else(|| panic!("{name} should exist"));
        &sigs[0]
    }

    #[test]
    fn test_builtin_function_lookup() {
        // Verify builtin functions are accessible for inference.
        // The actual integration happens in infer_expr() for Expr::Path.
        let builtins = builtin::builtin_functions();

        // Test that СтрДлина returns Number
        let strlen_sig = first_sig(builtins, "стрдлина");
        assert_eq!(*strlen_sig.ret, Ty::Number);
        assert_eq!(strlen_sig.params.len(), 1);
        assert_eq!(strlen_sig.params[0], Ty::String);

        // Test English variant
        let strlen_en = first_sig(builtins, "strlen");
        assert_eq!(*strlen_en.ret, Ty::Number);

        // Test case-insensitive lookup
        let upper_case = builtins.get("СТРДЛИНА");
        assert!(upper_case.is_some(), "Lookup should be case-insensitive");

        // Test that the resolved type would be correct
        // When Expr::Path("СтрДлина") is inferred, it should return:
        // Ty::Function { params: [Ty::String], ret: Ty::Number }
        let sig = first_sig(builtins, "стрдлина");
        let ty = Ty::Function {
            params: sig.params.clone(),
            defaults: sig.defaults.clone(),
            max_args: sig.max_args,
            ret: sig.ret.clone(),
        };
        match ty {
            Ty::Function { params, ret, .. } => {
                assert_eq!(params.len(), 1);
                assert_eq!(*ret, Ty::Number);
            }
            _ => panic!("Expected Function type"),
        }
    }

    #[test]
    fn test_builtin_date_function() {
        let builtins = builtin::builtin_functions();

        // ТекущаяДата() -> Дата
        let current_date = first_sig(builtins, "текущаядата");
        assert_eq!(*current_date.ret, Ty::Date);
        assert!(current_date.params.is_empty());

        // Год(Дата) -> Число
        let year = first_sig(builtins, "год");
        assert_eq!(*year.ret, Ty::Number);
        assert_eq!(year.params.len(), 1);
        assert_eq!(year.params[0], Ty::Date);
    }

    #[test]
    fn test_builtin_type_function() {
        let builtins = builtin::builtin_functions();

        // ТипЗнч(Any) -> Type
        let type_of = first_sig(builtins, "типзнч");
        assert_eq!(*type_of.ret, Ty::Type);
    }
}
