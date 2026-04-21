//! Single source of truth for method/function signature rendering across LSP features.
//!
//! Layered per Clean Architecture:
//! - [`domain`] — pure entities (`SymbolSignature`, `SignatureParam`, `CalleeKind`).
//! - [`use_cases`] — `resolve_callee_at` (CST → `CalleeKind`).
//! - [`adapters`] — one per data source (platform / global / common module / manager module / local).
//! - presenters — one per LSP feature (signature_help / hover / completion). Coming in later phases.
//!
//! Crate has no `lsp-types` dependency; presenters return view-models that the
//! LSP server crate maps to wire types.

pub mod adapters;
pub mod domain;
pub mod presenters;
pub mod use_cases;

pub use adapters::{build_signature, from_global_function, from_platform_method};
pub use domain::{
    CalleeKind, CodeExample, Lang, MethodKind, SignatureParam, SignatureSource, SymbolSignature,
    TypeRef,
};
pub use presenters::{
    render_completion_detail, render_hover_markdown, render_signature_help, CompletionDetail,
    ParameterInfoView, SignatureHelpView,
};
pub use use_cases::{resolve_callee_at, ActiveParam};
