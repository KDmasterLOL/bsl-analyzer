//! SDBL completion module (refactored with Clean Architecture).
//!
//! Architecture:
//! - `domain/` - Domain models and traits (no external dependencies)
//! - `use_cases/` - Business logic (use cases for different completion scenarios)
//! - `infrastructure/` - External dependencies (DB access, metadata, scope providers)
//! - `tests/` - Test fixtures and test suites

// TODO: Remove these allows once code is integrated
#![allow(dead_code)]
#![allow(unused_imports)]

pub mod domain;
pub mod infrastructure;
pub mod use_cases;

use super::{CompletionItem, CompletionPosition};
use ide_db::RootDatabase;

/// Main SDBL completion entry point (Facade).
///
/// Returns completion suggestions if cursor is inside an SDBL query string.
pub(super) fn sdbl_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    // TODO: Implement facade after migrating use cases
    // For now, delegate to old implementation
    super::sdbl_completion::sdbl_completions(db, position)
}
