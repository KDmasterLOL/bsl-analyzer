//! # HIR Type Inference
//!
//! This crate implements type inference for BSL (1C:Enterprise) language.
//! It uses a separate hir-ty crate for type system and inference.
//!
//! ## Architecture
//!
//! - **Type Representation**: `Ty` enum (re-exported from hir-def)
//! - **Inference**: `InferenceContext` performs type inference on HIR
//! - **Salsa Integration**: All type queries are cached via Salsa
//! - **Diagnostics**: Type-related diagnostics collected during inference
//!
//! ## Main Entry Points
//!
//! - `infer_query()` - Main Salsa query for type inference
//! - `type_of_expr_query()` - Get type of specific expression
//!
//! ## Example
//!
//! ```rust,ignore
//! use hir_ty::db::HirDatabase;
//!
//! // Get inference result for a file
//! let inference = db.infer(file_id);
//!
//! // Get type of specific expression
//! let ty = db.type_of_expr(file_id, expr_id);
//! ```

pub mod builtin;
pub mod db;
pub mod infer;
pub mod method_resolution;
pub mod type_db;

// Re-export main types for convenience
pub use hir_def::ty::{FunctionSignature, MetadataKind, Ty};
pub use infer::{InferenceContext, InferenceDiagnostic, InferenceResult, UnresolvedMethodKind};
pub use method_resolution::{resolve_qualified_call, MethodResolution};
pub use type_db::{TypeDatabase, VisibleConfig};
