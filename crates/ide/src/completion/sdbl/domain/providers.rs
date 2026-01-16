//! Provider traits for SDBL completion.
//!
//! These traits define abstractions for external dependencies:
//! - MetadataProvider: access to 1C metadata (Configuration)
//! - ScopeProvider: access to SDBL scope (tables, fields from HIR)
//!
//! Implementations are in infrastructure layer.

use bsl_metadata::Configuration;
use sdbl_hir::Scope;
use std::sync::Arc;
use syntax::TextSize;
use vfs::FileId;

/// Provider for 1C metadata (Configuration).
///
/// Allows use cases to access metadata without depending on RootDatabase directly.
pub trait MetadataProvider {
    /// Get configuration for the current file/position.
    ///
    /// Returns None if no metadata is available.
    fn get_configuration(&self) -> Option<Arc<Configuration>>;
}

/// Provider for SDBL scope.
///
/// Allows use cases to access SDBL scope (tables, fields, HIR) without
/// depending on RootDatabase directly.
pub trait ScopeProvider {
    /// Get SDBL scope at given position.
    ///
    /// ## Parameters
    /// - `file_id`: FileId containing the SDBL query
    /// - `bsl_literal_range`: TextRange of the BSL string literal containing SDBL query
    /// - `sdbl_offset`: Offset within the SDBL query text (NOT BSL offset)
    ///
    /// Returns None if position is not inside SDBL query or scope cannot be built.
    fn get_scope(
        &self,
        file_id: FileId,
        bsl_literal_range: syntax::TextRange,
        sdbl_offset: TextSize,
    ) -> Option<Scope>;
}
