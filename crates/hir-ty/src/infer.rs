//! Type inference for BSL using HIR.
//!
//! This module implements type inference over HIR (Body/Expr/Stmt) instead of AST.
//! This allows:
//! - Diagnostics collection during inference (rust-analyzer pattern)
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
use hir_def::hir::{BinaryOp, Expr, Literal, UnaryOp};
use hir_def::resolver::Resolver;
use hir_def::ty::Ty;
use hir_def::{ExprId, Name};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::method_resolution;

/// Result of type inference for a file/module.
///
/// Contains inferred types for all expressions and collected diagnostics.
/// This structure is cached by Salsa.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferenceResult {
    /// Type of each expression, keyed by ExprId.
    pub expr_types: FxHashMap<ExprId, Ty>,

    /// Diagnostics collected during type inference.
    ///
    /// Following rust-analyzer pattern: diagnostics are collected as a byproduct
    /// of type inference, not emitted immediately.
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

    /// Accumulated inference results.
    result: InferenceResult,
}

impl<'db> InferenceContext<'db> {
    /// Create a new inference context for a file.
    pub fn new(db: &'db dyn HirDatabase, file_id: FileId, body: &Arc<Body>) -> Self {
        Self { db, file_id, body: Arc::clone(body), result: InferenceResult::default() }
    }

    /// Finish inference and return the result.
    pub fn finish(self) -> InferenceResult {
        self.result
    }

    /// Get source root ID for the current file.
    ///
    /// Used for workspace symbol resolution (Salsa-cached).
    fn get_source_root_id(&self) -> base_db::SourceRootId {
        let file_source_root_input = self.db.file_source_root_input(self.file_id);
        file_source_root_input.source_root_id(self.db)
    }

    /// Get resolver for the current module.
    fn get_resolver(&self) -> Resolver {
        let module_id = hir_def::ModuleId { file_id: self.file_id };
        Resolver::with_workspace_scope(module_id)
    }

    /// Infer types for all expressions in the body.
    ///
    /// This is called from infer_query after creating the context.
    pub fn infer_all(&mut self) {
        let _p = tracing::debug_span!("infer_all").entered();

        // Infer types for all expressions in the arena
        // We collect all ExprIds first to avoid borrowing issues
        let expr_ids: Vec<ExprId> = self.body.exprs_iter().map(|(id, _)| id).collect();

        for expr_id in expr_ids {
            self.infer_expr(expr_id);
        }

        debug!(
            "inferred {} expression types, {} diagnostics",
            self.result.expr_types.len(),
            self.result.diagnostics.len()
        );
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

            Expr::Path(name) => {
                // Try builtin functions first
                if let Some(sig) = builtin::builtin_functions().get(name.as_str()) {
                    trace!("resolved {} to builtin function", name);
                    return Ty::Function { params: sig.params.clone(), ret: sig.ret.clone() };
                }

                // TODO: Phase 2+: Resolve variable/parameter type
                Ty::Unknown
            }

            Expr::QualifiedPath(_path) => {
                // Phase 1: Return Unknown
                // Phase 3: Resolve CommonModule.Method or Metadata.Manager paths
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

            Expr::MethodCall { receiver, method: _, args } => {
                // Infer receiver and args
                self.infer_expr(ExprId::from_idx(*receiver));
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                // Phase 1: Return Unknown
                // Phase 3: Resolve method on receiver type
                Ty::Unknown
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

                // Infer type from constructor
                match type_name {
                    Some(name) => Ty::from_type_name(name.as_str()),
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
        // Phase 3: Check if callee is a qualified path (Module.Method)
        // If yes, try to resolve it
        let callee_expr = self.body.expr(callee);
        if let Expr::QualifiedPath(qualified_path) = callee_expr {
            // Phase 3: Handle two-level qualified calls (Module.Method)
            if qualified_path.segments().len() == 2 {
                // Clone the names to avoid borrow checker issues
                let module_name = qualified_path.first().clone();
                let method_name = qualified_path.last().clone();
                return self.infer_qualified_call(&module_name, &method_name, args, callee);
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

        // Get source root and resolver
        let source_root_id = self.get_source_root_id();
        let resolver = self.get_resolver();

        // Resolve the qualified call
        match method_resolution::resolve_qualified_call(
            self.db,
            module_name,
            method_name,
            &resolver,
            source_root_id,
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

    // For Phase 1 MVP: Only infer types for module-level code (code outside procedures/functions)
    // TODO Phase 2: Also infer types for all method bodies
    let mut result = InferenceResult::default();

    if let Some(body) = module_bodies.module_code() {
        // Create inference context for module-level code
        let mut ctx = InferenceContext::new(db, file_id, &Arc::new(body.clone()));

        // Run inference
        ctx.infer_all();

        // Merge results
        result = ctx.finish();
    }

    info!(
        "type inference complete: {} expr types, {} diagnostics",
        result.expr_types.len(),
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
