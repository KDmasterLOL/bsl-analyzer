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
//! use hir_def::DefWithBodyId;
//!
//! // Get inference result for a file
//! let inference = db.infer(file_id);
//!
//! // Get type of a specific expression. `ExprId` is only unique inside
//! // a single `Body`, so callers must identify the body with
//! // `DefWithBodyId::Method(local_id)` or `DefWithBodyId::ModuleCode`.
//! let owner = DefWithBodyId::Method(local_id);
//! let ty = db.type_of_expr(file_id, owner, expr_id);
//! ```

pub mod arg_diagnostics;
pub mod builtin;
pub mod db;
pub mod field_enum;
pub mod field_lookup;
pub mod form_attr;
pub mod form_self;
pub mod infer;
pub mod lower;
pub mod manager_lookup;
pub mod method_lookup;
pub mod method_resolution;
pub mod narrow;
pub mod platform_manager_lookup;
pub mod platform_property_lookup;
pub mod subtype;
pub mod this_object;

// Re-export main types for convenience
pub use field_enum::{enumerate_fields, FieldInfo, FieldOrigin};
pub use field_lookup::lookup_field;
pub use hir_def::ty::{FormDataKind, FunctionSignature, MetadataKind, Ty};
pub use hir_def::type_ref::{BuiltinTypeRef, TypeRef};
pub use hir_def::{ConfigsDatabase, VisibleConfig};
pub use infer::{
    CallArgBinding, InferenceContext, InferenceDiagnostic, InferenceResult, ParamsShape,
    UnresolvedMethodKind,
};
pub use lower::TyLoweringContext;
pub use manager_lookup::{lookup_manager_field, ManagerMemberInfo};
pub use method_lookup::{lookup_method, MethodInfo};
pub use method_resolution::{resolve_qualified_call, MethodResolution};
pub use platform_manager_lookup::{
    resolve_platform_manager_method, resolve_platform_metadata_ref_method, PlatformMethodResolution,
};
pub use platform_property_lookup::{lookup_platform_property, PlatformPropertyResolution};
pub use subtype::{is_assignable, is_ref_ty};
pub use this_object::coerce_to_metadata_ref as coerce_this_object_to_metadata_ref;
