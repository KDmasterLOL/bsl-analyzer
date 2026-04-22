//! Salsa-backed workspace feature flags.
//!
//! Single-source-of-truth for opt-in analysis features (currently only
//! narrowing overlay; Task 6.7). The flags live behind a Salsa input so
//! that any future `#[salsa::tracked]` query that reads them participates
//! in Salsa's invalidation graph automatically — today's consumers are
//! plain Rust helpers (`narrow_or_base` in hir) that simply observe the
//! latest value on their next call.
//!
//! A single instance per database is used as a singleton; the handle is
//! eagerly created by `RootDatabaseImpl::new` and stored on the database
//! alongside `WorkspaceConfigsInput`.

/// Salsa input carrying workspace-wide feature flags.
///
/// Defaults are set by `RootDatabaseImpl::new` (all flags on). Consumers
/// update the input via generated setters through `RootDatabaseImpl`
/// helpers.
#[salsa::input(debug)]
pub struct FeaturesInput {
    /// Type narrowing overlay (ADR-01 Option A). When `false`,
    /// `Semantics::type_of_expr` skips the narrowing analysis entirely
    /// and returns the base inferred type.
    pub type_narrowing: bool,
}
