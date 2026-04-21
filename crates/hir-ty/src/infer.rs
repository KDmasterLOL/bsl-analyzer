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
use hir_def::{ExprId, Name};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
use crate::method_resolution;

/// Result of type inference for a file/module.
///
/// Contains inferred types for all expressions and collected diagnostics.
/// This structure is cached by Salsa.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferenceResult {
    /// Type of each expression, keyed by ExprId.
    pub expr_types: FxHashMap<ExprId, Ty>,

    /// Variable types inferred from assignments.
    ///
    /// Maps lowercase variable name to its last inferred type.
    /// Populated by tracking `Stmt::Assign { target: Path(name), value }` during inference.
    /// Used by completion to resolve receiver types for method lookup.
    pub var_types: FxHashMap<String, Ty>,

    /// Diagnostics collected during type inference.
    ///
    /// Diagnostics are collected as a byproduct of type inference, not emitted immediately.
    pub diagnostics: Vec<InferenceDiagnostic>,
}

impl InferenceResult {
    /// Get the type of an expression.
    pub fn type_of_expr(&self, expr: ExprId) -> Option<&Ty> {
        self.expr_types.get(&expr)
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
    /// Unresolved method call (CommonModule.Method not found).
    ///
    /// Emitted when:
    /// - CommonModule doesn't exist in workspace
    /// - Method doesn't exist in CommonModule
    /// - Method exists but is not exported
    /// - CommonModule source file is missing
    UnresolvedMethodCall {
        expr: ExprId,
        receiver_name: Name,
        method_name: Name,
        kind: UnresolvedMethodKind,
    },

    /// Mismatched argument count in function call.
    ///
    /// Emitted when calling a function with wrong number of arguments.
    MismatchedArgCount { call_expr: ExprId, expected: usize, found: usize },

    /// Type mismatch between expected and actual type.
    ///
    /// Emitted when expression type doesn't match expected type
    /// (e.g., assigning String to Number variable).
    TypeMismatch { expr: ExprId, expected: Ty, actual: Ty },
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

/// Context for type inference.
///
/// Performs type inference for a single file/module, building up an InferenceResult.
pub struct InferenceContext<'db> {
    /// Database for queries.
    ///
    /// Used for method resolution, metadata queries, workspace symbols.
    db: &'db dyn HirDatabase,

    /// File being inferred.
    ///
    /// Used for diagnostics reporting and workspace file collection.
    file_id: FileId,

    /// HIR body for the file.
    body: Arc<Body>,

    /// Variable types tracked from assignments (lowercase name → Ty).
    var_types: FxHashMap<String, Ty>,

    /// Accumulated inference results.
    result: InferenceResult,
}

impl<'db> InferenceContext<'db> {
    /// Create a new inference context for a file.
    pub fn new(db: &'db dyn HirDatabase, file_id: FileId, body: &Arc<Body>) -> Self {
        Self {
            db,
            file_id,
            body: Arc::clone(body),
            var_types: FxHashMap::default(),
            result: InferenceResult::default(),
        }
    }

    /// Finish inference and return the result.
    pub fn finish(mut self) -> InferenceResult {
        self.result.var_types = self.var_types;
        self.result
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
            self.result.expr_types.len(),
            self.var_types.len(),
            self.result.diagnostics.len()
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
                let target_expr = self.body.expr_idx(*target);
                if let Expr::Path(name) = target_expr {
                    if !value_ty.is_unknown() {
                        self.var_types.insert(name.as_str().to_lowercase(), value_ty);
                    }
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
        if let Some(ty) = self.result.expr_types.get(&expr_id) {
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

            Expr::Field { base, field: _ } => {
                self.infer_expr(ExprId::from_idx(*base));

                // Phase 1: Return Unknown
                // Phase 2+: Resolve field type from base type
                Ty::Unknown
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
        self.result.expr_types.insert(expr_id, ty.clone());
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
        let resolved = resolver.resolve_name(self.db, name);

        // 1. Builtins — union of Resolver's platform-global view and the
        //    narrower hir-ty signature table. Either hit makes the name a
        //    builtin; only the hir-ty table supplies a typed signature.
        let resolver_says_builtin = matches!(resolved, Some(Resolution::Builtin(_)));
        let hir_sig = builtin::builtin_functions().get(name.as_str());
        if resolver_says_builtin || hir_sig.is_some() {
            if let Some(sig) = hir_sig {
                trace!("resolved {} as builtin via hir-ty signature table", name);
                return Ty::Function { params: sig.params.clone(), ret: sig.ret.clone() };
            }
            // Resolver classifies the name as a platform global but the
            // hir-ty signature table has no typed entry for it. Leave
            // `Ty::Unknown` until Task 2.x broadens signature coverage.
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
        if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(name.as_str()) {
            if let Some(ty) = Ty::manager_collection(mdo_type) {
                trace!("resolved {} as manager collection {:?}", name, mdo_type);
                return ty;
            }
        }

        // 4. Module-level methods / variables (Unknown today; Task 2.x
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

        // Infer callee type for non-qualified calls
        let callee_ty = self.infer_expr(callee);

        // Infer argument types
        for arg in args {
            self.infer_expr(*arg);
        }

        // Check if callee is a function type
        match callee_ty {
            Ty::Function { ref params, ref ret } => {
                // Phase 2: Check argument count
                if args.len() != params.len() {
                    self.result.diagnostics.push(InferenceDiagnostic::MismatchedArgCount {
                        call_expr: callee,
                        expected: params.len(),
                        found: args.len(),
                    });
                }

                // TODO Phase 2+: Check argument types
                // for (arg_id, param_ty) in args.iter().zip(params.iter()) {
                //     let arg_ty = self.result.expr_types.get(arg_id).cloned().unwrap_or(Ty::Unknown);
                //     if !self.is_compatible(&arg_ty, param_ty) {
                //         self.result.diagnostics.push(InferenceDiagnostic::TypeMismatch { ... });
                //     }
                // }

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
                    self.result.diagnostics.push(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: module_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                // Check argument count
                if args.len() != resolution.signature.params.len() {
                    self.result.diagnostics.push(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        expected: resolution.signature.params.len(),
                        found: args.len(),
                    });
                }

                // Return method's return type
                resolution.return_type
            }
            Err(kind) => {
                // Method not found - emit diagnostic
                self.result.diagnostics.push(InferenceDiagnostic::UnresolvedMethodCall {
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
                    self.result.diagnostics.push(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: receiver_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                if args.len() != resolution.signature.params.len() {
                    self.result.diagnostics.push(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        expected: resolution.signature.params.len(),
                        found: args.len(),
                    });
                }

                resolution.return_type
            }
            Err(kind) => {
                self.result.diagnostics.push(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind,
                });
                Ty::Unknown
            }
        }
    }
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

    // Infer module-level code (statements outside procedures/functions)
    if let Some(body) = module_bodies.module_code() {
        let mut ctx = InferenceContext::new(db, file_id, &Arc::new(body.clone()));
        ctx.infer_all();
        let module_result = ctx.finish();
        result.var_types.extend(module_result.var_types);
        result.diagnostics.extend(module_result.diagnostics);
    }

    // Infer all method bodies (procedures and functions)
    for (_local_id, body) in module_bodies.iter_bodies() {
        let mut ctx = InferenceContext::new(db, file_id, &Arc::new(body.clone()));
        ctx.infer_all();
        let method_result = ctx.finish();
        // Merge var_types from all methods (completion will match by variable name)
        result.var_types.extend(method_result.var_types);
        result.diagnostics.extend(method_result.diagnostics);
    }

    info!(
        "type inference complete: {} var types, {} diagnostics",
        result.var_types.len(),
        result.diagnostics.len()
    );

    Arc::new(result)
}

/// Salsa query: Get type of a specific expression.
///
/// This is a convenience query derived from `infer()`. It avoids
/// exposing the entire InferenceResult when only one type is needed.
///
/// # Returns
///
/// - The inferred type of the expression
/// - `Ty::Unknown` if the expression was not found
pub fn type_of_expr_query(db: &dyn HirDatabase, file_id: FileId, expr: ExprId) -> Ty {
    let infer = db.infer(file_id);
    infer.type_of_expr(expr).cloned().unwrap_or(Ty::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inference_result_default() {
        let result = InferenceResult::default();
        assert_eq!(result.expr_types.len(), 0);
        assert_eq!(result.diagnostics.len(), 0);
        assert!(!result.has_diagnostics());
    }

    #[test]
    fn test_mismatched_arg_count_diagnostic() {
        // Test that MismatchedArgCount diagnostic is created correctly
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let diag =
            InferenceDiagnostic::MismatchedArgCount { call_expr: expr_id, expected: 2, found: 1 };

        match diag {
            InferenceDiagnostic::MismatchedArgCount { call_expr, expected, found } => {
                assert_eq!(call_expr, expr_id);
                assert_eq!(expected, 2);
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
    fn test_inference_result_with_diagnostics() {
        let mut result = InferenceResult::default();
        assert!(!result.has_diagnostics());

        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        result.diagnostics.push(InferenceDiagnostic::MismatchedArgCount {
            call_expr: expr_id,
            expected: 2,
            found: 1,
        });

        assert!(result.has_diagnostics());
        assert_eq!(result.diagnostics.len(), 1);
    }

    #[test]
    fn test_builtin_function_lookup() {
        // Verify builtin functions are accessible for inference.
        // The actual integration happens in infer_expr() for Expr::Path.
        let builtins = builtin::builtin_functions();

        // Test that СтрДлина returns Number
        let strlen_sig = builtins.get("стрдлина").expect("СтрДлина should exist");
        assert_eq!(*strlen_sig.ret, Ty::Number);
        assert_eq!(strlen_sig.params.len(), 1);
        assert_eq!(strlen_sig.params[0], Ty::String);

        // Test English variant
        let strlen_en = builtins.get("strlen").expect("StrLen should exist");
        assert_eq!(*strlen_en.ret, Ty::Number);

        // Test case-insensitive lookup
        let upper_case = builtins.get("СТРДЛИНА");
        assert!(upper_case.is_some(), "Lookup should be case-insensitive");

        // Test that the resolved type would be correct
        // When Expr::Path("СтрДлина") is inferred, it should return:
        // Ty::Function { params: [Ty::String], ret: Ty::Number }
        if let Some(sig) = builtins.get("стрдлина") {
            let ty = Ty::Function { params: sig.params.clone(), ret: sig.ret.clone() };
            match ty {
                Ty::Function { params, ret } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(*ret, Ty::Number);
                }
                _ => panic!("Expected Function type"),
            }
        }
    }

    #[test]
    fn test_builtin_date_function() {
        let builtins = builtin::builtin_functions();

        // ТекущаяДата() -> Дата
        let current_date = builtins.get("текущаядата").expect("ТекущаяДата should exist");
        assert_eq!(*current_date.ret, Ty::Date);
        assert!(current_date.params.is_empty());

        // Год(Дата) -> Число
        let year = builtins.get("год").expect("Год should exist");
        assert_eq!(*year.ret, Ty::Number);
        assert_eq!(year.params.len(), 1);
        assert_eq!(year.params[0], Ty::Date);
    }

    #[test]
    fn test_builtin_type_function() {
        let builtins = builtin::builtin_functions();

        // ТипЗнч(Any) -> Type
        let type_of = builtins.get("типзнч").expect("ТипЗнч should exist");
        assert_eq!(*type_of.ret, Ty::Type);
    }
}
