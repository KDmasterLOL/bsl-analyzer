//! SDBL HIR - semantic representation of SDBL queries.
//!
//! This crate provides High-level Intermediate Representation (HIR) for SDBL
//! (Structured Data Base Language) - the query language used in 1C:Enterprise.
//!
//! # Features
//!
//! - **Type inference**: Infer field types from 1C metadata
//! - **Name resolution**: Resolve tables to metadata objects, fields to definitions
//! - **Semantic diagnostics**: Collected during lowering (QueryToMissingMetadata, etc.)
//! - **LSP support**: Foundation for completion, hover, go-to-definition
//!
//! # Architecture
//!
//! ```text
//! SDBL AST (from parser)
//!     ↓
//! lower_sdbl_to_hir() with metadata context
//!     ↓
//! SdblHir (typed, resolved)
//!     ↓
//! Diagnostics + LSP features
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use sdbl_hir::{lower_sdbl_to_hir, SdblHir};
//!
//! let sdbl_hir = lower_sdbl_to_hir(&sdbl_ast, &metadata);
//!
//! // Check for semantic errors
//! for diag in &sdbl_hir.diagnostics {
//!     println!("Error: {:?}", diag);
//! }
//!
//! // Access typed fields
//! for field in &sdbl_hir.select.fields {
//!     println!("Field: {} (type: {:?})", field.alias_or_name(), field.ty);
//! }
//! ```

mod diagnostics;
mod hir;
mod lower;
mod scope;
mod standard_fields;
mod types;

pub use diagnostics::SdblDiagnostic;
pub use hir::{
    ExprHir, FieldDef, FieldHir, GroupByHir, JoinHir, JoinType, OrderByHir, ResolvedTable, SdblHir,
    SelectHir, TableRef, UnionHir,
};
pub use lower::lower_sdbl_to_hir;
pub use scope::Scope;
pub use types::{MdoRef, SdblType};
