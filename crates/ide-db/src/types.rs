//! Shared types for ide-db.

/// Symbol kind (procedure, function, variable, etc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Procedure,
    Function,
    Variable,
    Region,
}

// `SdblHirEntries` lives in `hir_def::sdbl_cache` so the SDBL ↔ Ty
// bridge in `hir-ty` can consume the cache from below. Re-exported
// through `crate::SdblHirEntries` for back-compat with existing
// `ide_db::SdblHirEntries` callers.
