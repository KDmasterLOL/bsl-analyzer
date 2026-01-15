//! Common types shared between cfg and hir-def.
//!
//! This crate breaks the circular dependency by extracting index types
//! that both crates need, without depending on concrete HIR types.
//!
//! ## Problem
//!
//! Before this crate:
//! ```text
//! cfg → hir-def (depends for ExprId = Idx<Expr>, StmtId = Idx<Stmt>)
//! hir-def → cfg (BLOCKED: would create cycle)
//! ```
//!
//! ## Solution
//!
//! After this crate:
//! ```text
//! cfg → cfg-types (opaque ExprId(u32), StmtId(u32))
//! hir-def → cfg-types (opaque IDs)
//! hir-def → cfg (✅ NO CYCLE!)
//! ```
//!
//! ## Usage
//!
//! **In cfg crate:**
//! ```ignore
//! use cfg_types::{ExprId, StmtId, BindingId};
//!
//! pub enum Vertex {
//!     Expr(ExprId),  // Opaque, no dependency on hir_def::Expr
//!     Stmt(StmtId),
//! }
//! ```
//!
//! **In hir-def crate:**
//! ```ignore
//! use cfg_types::{ExprId, StmtId, IdConversion};
//!
//! // Re-export for backward compatibility
//! pub use cfg_types::{ExprId, StmtId, BindingId};
//!
//! // Convert when passing to cfg
//! let opaque_id = ExprId::from_idx(typed_id);
//!
//! // Convert back when reading from arena
//! let typed_id: Idx<Expr> = opaque_id.to_idx();
//! let expr = body.exprs[typed_id];
//! ```

pub use indices::{BindingId, ExprId, IdConversion, StmtId};

pub mod indices;
