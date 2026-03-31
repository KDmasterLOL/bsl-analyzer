//! Shared types for ide-db.

use std::sync::Arc;

/// Symbol kind (procedure, function, variable, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    Region,
}

/// Type alias for SDBL HIR entries in a file.
///
/// Maps SdblExprId (unique across all bodies in file) to the corresponding SDBL package.
pub type SdblHirEntries = Arc<Vec<(hir::SdblExprId, Arc<sdbl_hir::SdblPackage>)>>;
