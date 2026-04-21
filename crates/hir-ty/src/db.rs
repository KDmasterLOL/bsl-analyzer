//! Salsa database trait for type inference queries.

use hir_def::ExprId;
use std::sync::Arc;
use vfs::FileId;

use crate::infer::InferenceResult;
use crate::type_db::TypeDatabase;
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
pub trait HirDatabase: TypeDatabase {
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

    /// Get type of a specific expression.
    ///
    /// This is a convenience query derived from `infer()`. It returns the type
    /// of a single expression without exposing the entire inference result.
    ///
    /// # Returns
    ///
    /// - The inferred type of the expression
    /// - `Ty::Unknown` if the expression was not found in the inference result
    ///
    /// # Implementation
    /// Should delegate to [`crate::infer::type_of_expr_query`].
    fn type_of_expr(&self, file_id: FileId, expr: ExprId) -> Ty;
}
