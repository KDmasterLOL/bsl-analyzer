//! Domain layer for SDBL completion.
//!
//! This layer contains:
//! - Domain models (completion items, context)
//! - Provider traits (abstractions for external dependencies)
//!
//! Domain layer has NO dependencies on infrastructure (RootDatabase, VFS, etc.)
//! and can be tested in isolation with mocks.

pub mod completion_item;
pub mod providers;

pub use completion_item::SdblCompletionItem;
pub use providers::{MetadataProvider, ScopeProvider};
