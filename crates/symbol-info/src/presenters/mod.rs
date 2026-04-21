//! Presenters render a [`SymbolSignature`](crate::SymbolSignature) into a
//! consumer-specific view-model.
//!
//! No LSP types here; the LSP server crate maps view-models to wire types.

pub mod completion;
pub mod hover;
pub mod signature_help;

pub use completion::{render_completion_detail, CompletionDetail};
pub use hover::render_hover_markdown;
pub use signature_help::{render_signature_help, ParameterInfoView, SignatureHelpView};
