//! Infrastructure layer for SDBL completion.
//!
//! This layer contains implementations of provider traits:
//! - DbMetadataProvider: gets Configuration from RootDatabase
//! - DbScopeProvider: builds SDBL Scope from RootDatabase
//!
//! Infrastructure depends on Domain (traits) but Domain doesn't depend on Infrastructure.

pub mod metadata_provider;
pub mod scope_provider;

pub use metadata_provider::DbMetadataProvider;
pub use scope_provider::DbScopeProvider;
