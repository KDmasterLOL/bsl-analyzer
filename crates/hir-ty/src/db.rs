//! Salsa database trait for type inference queries.

use hir_def::{ConfigsDatabase, DefWithBodyId, ExprId, MethodIdInput};
use std::sync::Arc;
use vfs::FileId;

use crate::infer::{InferenceDiagnostic, InferenceResult};
use crate::narrow::NarrowState;
use crate::proc_signature::ProcSignature;
use crate::Ty;

/// Database trait for HIR type inference.
///
/// This trait extends DefDatabase with type-related queries.
/// All queries are cached by Salsa for incremental computation.
///
/// # Implementation Pattern
///
/// Implementations delegate to tracked query functions in the `infer` module:
///
/// ```ignore
/// impl HirDatabase for MyDatabase {
///     fn infer(&self, file_id: FileId) -> Arc<InferenceResult> {
///         crate::infer::infer_query(self, file_id)
///     }
/// }
/// ```
#[salsa::db]
pub trait HirDatabase: ConfigsDatabase {
    /// Infer types for all expressions in a file.
    ///
    /// This is the main entry point for type inference. It runs type inference
    /// on all methods/functions in the module and returns the complete inference result.
    ///
    /// # Caching
    ///
    /// Results are cached by Salsa and invalidated when the file or its dependencies change.
    ///
    /// # Performance
    /// - **LRU cache:** 256 files
    /// - **Depends on:** [`module_bodies`](hir_def::DefDatabase::module_bodies)
    /// - **Typical time:** ~10-50ms for medium files
    ///
    /// # Implementation
    /// Should delegate to [`crate::infer::infer_query`].
    fn infer(&self, file_id: FileId) -> Arc<InferenceResult>;

    /// Get type of a specific expression in a specific body.
    ///
    /// `ExprId` is only unique within a single `Body`, so callers must
    /// disambiguate with `DefWithBodyId` — `Method(local_id)` for a
    /// procedure / function body, `ModuleCode` for module-level code.
    /// The IDE-facing `Semantics::type_of_expr(SyntaxNode)` derives the
    /// owner automatically via `BodySourceMap`.
    ///
    /// # Returns
    ///
    /// - The inferred type for `(owner, expr)`.
    /// - `Ty::Unknown` if inference produced no entry for that pair.
    ///
    /// # Implementation
    /// Should delegate to [`crate::infer::type_of_expr_query`].
    fn type_of_expr(&self, file_id: FileId, owner: DefWithBodyId, expr: ExprId) -> Ty;

    /// Run narrowing analysis on a single body (ADR-01 Option A).
    ///
    /// Returns the `Arc`-shared [`dataflow::DataflowResult`] produced by the
    /// [`NarrowState`] lattice over the body's CFG. [`Semantics::type_of_expr`]
    /// consumes it to overlay narrowed types on `Expr::Path` references — the
    /// Task 6.5 invariant that hovers on a guard's receiver see the pre-narrow
    /// type while hovers inside the then / else body see the narrowed one
    /// falls out structurally from CFG vertex placement.
    ///
    /// Returns `None` when `owner` does not resolve to a body in this file,
    /// or (impossibly) when the solver fails to converge.
    ///
    /// # Implementation
    /// Should delegate to [`crate::narrow::narrow_query`].
    ///
    /// [`Semantics::type_of_expr`]: https://docs.rs/hir/latest/hir/struct.Semantics.html#method.type_of_expr
    fn narrow(
        &self,
        file_id: FileId,
        owner: DefWithBodyId,
    ) -> Option<Arc<dataflow::DataflowResult<NarrowState>>>;

    /// Narrowing-aware argument type-mismatch diagnostics for `file_id`.
    ///
    /// Consumes [`HirDatabase::infer`] (for the per-call-site
    /// `(args, params)` shape recorded during inference) and
    /// [`HirDatabase::narrow`] (for the per-program-point overlay).
    /// Each emitted [`InferenceDiagnostic::TypeMismatch`] is paired
    /// with its owning [`DefWithBodyId`] so ide-diagnostics can resolve
    /// the body-local `ExprId` through the right `BodySourceMap`.
    ///
    /// Inference itself no longer emits argument-`TypeMismatch`
    /// diagnostics — moving them out lets this query consult the
    /// narrowing overlay before deciding, so guards like
    /// `If X <> Undefined Then …` correctly suppress false positives.
    /// `MismatchedArgCount` stays inside `infer_query` (no narrowing
    /// dependency).
    ///
    /// # Implementation
    /// Should delegate to [`crate::arg_diagnostics::arg_diagnostics_query`].
    fn arg_diagnostics(&self, file_id: FileId) -> Arc<Vec<(DefWithBodyId, InferenceDiagnostic)>>;

    /// Whether the type narrowing overlay (ADR-01 Option A) is enabled.
    ///
    /// Implementations read the current value from a Salsa input hosted
    /// by `ide_db`, so the method always observes the latest value set
    /// through the corresponding setter — but note: the only current
    /// consumer, [`narrow_or_base`] in `hir`, is a plain Rust helper
    /// called from `Semantics::type_of_expr`, not a Salsa-tracked query.
    /// Flipping the flag therefore takes effect on the next call, not
    /// by Salsa invalidation. If a future query is added that reads
    /// this flag from a `#[salsa::tracked]` function, Salsa revision
    /// tracking *will* kick in for that query.
    ///
    /// Consumers inside hir use it as a short-circuit before calling
    /// [`HirDatabase::narrow`].
    ///
    /// [`narrow_or_base`]: ../../../hir/fn.narrow_or_base.html
    fn type_narrowing_enabled(&self) -> bool;

    /// Lower a workspace-defined method's `(params, return_ty)` signature
    /// from its docstring.
    ///
    /// Returns gradual `Ty::Unknown` for any parameter / return slot the
    /// docstring omits — call sites consuming this signature must accept
    /// `Unknown` actuals via the existing `is_assignable` rule.
    ///
    /// # Implementation
    /// Should delegate to [`crate::proc_signature::proc_signature_query`].
    fn proc_signature(&self, method_input: MethodIdInput<'_>) -> Arc<ProcSignature>;
}
