//! Salsa database trait for type inference queries.

use hir_def::{ConfigsDatabase, DefWithBodyId, ExprId};
use std::sync::Arc;
use vfs::FileId;

use crate::infer::InferenceResult;
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
}
