//! SDBL completion module (refactored with Clean Architecture).
//!
//! Architecture:
//! - `domain/` - Domain models and traits (no external dependencies)
//! - `use_cases/` - Business logic (use cases for different completion scenarios)
//! - `infrastructure/` - External dependencies (DB access, metadata, scope providers)
//! - `tests/` - Test fixtures and test suites

pub mod domain;
pub mod infrastructure;
pub mod use_cases;

use super::{CompletionItem, CompletionPosition};
use domain::ScopeProvider;
use ide_db::RootDatabase;
use infrastructure::{DbMetadataProvider, DbScopeProvider};
use sdbl_hir::{detect_context, detect_sdbl_at_position, SdblCompletionContext};
use use_cases::{
    CompleteAliasesUseCase, CompleteFieldsUseCase, CompleteKeywordsUseCase, CompleteMdoUseCase,
};

/// Main SDBL completion entry point (Facade).
///
/// Returns completion suggestions if cursor is inside an SDBL query string.
pub(super) fn sdbl_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let file_id = position.file_id;
    let offset = position.offset;

    tracing::info!("sdbl_completions called: file_id={:?}, offset={:?}", file_id, offset);

    // Get parsed file
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Check if position is inside SDBL query string
    let query_info = detect_sdbl_at_position(&root, offset)?;

    tracing::info!(
        query_len = query_info.query_text.len(),
        offset_in_query = u32::from(query_info.offset_in_query),
        "detected SDBL query"
    );

    // Create providers
    let metadata_provider =
        DbMetadataProvider::with_workspace(db, file_id, position.workspace_root.as_deref());
    let scope_provider = DbScopeProvider::new(db);

    // Try to get Scope for the query at cursor position
    // Pass BSL literal range and SDBL offset (already converted from BSL offset)
    let scope =
        scope_provider.get_scope(file_id, query_info.bsl_literal_range, query_info.offset_in_query);
    if scope.is_some() {
        tracing::debug!("successfully built Scope from query HIR");
    } else {
        tracing::debug!("failed to build Scope (no HIR or no tables)");
    }

    // Determine completion context
    let context = detect_context(&query_info.query_text, query_info.offset_in_query);

    // Match on context and call appropriate use case
    let items = match (context, scope.as_ref()) {
        // Alias field completion - requires scope
        (SdblCompletionContext::AfterTableAlias { alias, prefix }, Some(scope)) => {
            tracing::info!(
                alias = %alias,
                prefix = %prefix,
                "completion context: AfterTableAlias (with scope)"
            );
            CompleteFieldsUseCase::execute(scope, &alias, &prefix)
        }
        (SdblCompletionContext::AfterTableAlias { alias, prefix }, None) => {
            tracing::warn!(
                alias = %alias,
                prefix = %prefix,
                "completion context: AfterTableAlias but no scope available (HIR failed?)"
            );
            // Fallback to keywords if no scope
            CompleteKeywordsUseCase::execute(&prefix)
        }

        // Alias suggestion after AS/КАК
        (SdblCompletionContext::AfterAsKeyword { context: as_context, suggestion }, _) => {
            tracing::info!(
                ?as_context,
                suggestion = ?suggestion,
                "completion context: AfterAsKeyword"
            );
            CompleteAliasesUseCase::execute_alias_suggestion(suggestion)
        }

        // JOIN type keywords (delegates to old implementation for now)
        (SdblCompletionContext::JoinTypeKeyword { prefix }, _) => {
            tracing::info!(prefix = %prefix, "completion context: JoinTypeKeyword");
            // TODO: Create use case for JOIN type keywords
            return super::sdbl_completion::sdbl_completions(db, position);
        }

        // Table aliases after ON - requires scope
        (SdblCompletionContext::AfterOnKeyword { prefix }, Some(scope)) => {
            tracing::info!(
                prefix = %prefix,
                "completion context: AfterOnKeyword (with scope)"
            );
            CompleteAliasesUseCase::execute_table_aliases(scope, &prefix)
        }
        (SdblCompletionContext::AfterOnKeyword { prefix }, None) => {
            tracing::warn!(
                prefix = %prefix,
                "completion context: AfterOnKeyword but no scope available"
            );
            // Fallback to keywords if no scope
            CompleteKeywordsUseCase::execute(&prefix)
        }

        // AfterFromKeyword - suggest MDO types
        (SdblCompletionContext::AfterFromKeyword, _) => {
            tracing::info!("completion context: AfterFromKeyword");
            CompleteMdoUseCase::execute_types("")
        }

        // InsideMdoType - suggest MDO objects
        (SdblCompletionContext::InsideMdoType { mdo_type, prefix }, _) => {
            tracing::info!(
                ?mdo_type,
                prefix = %prefix,
                "completion context: InsideMdoType"
            );
            CompleteMdoUseCase::execute_objects(&metadata_provider, mdo_type, &prefix)
        }

        // AfterMdoObject - suggest nested elements (delegates to old implementation for now)
        (SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix }, _) => {
            tracing::info!(
                ?mdo_type,
                object_name = %object_name,
                prefix = %prefix,
                "completion context: AfterMdoObject"
            );
            // TODO: Create use case for nested elements (tabular sections, virtual tables)
            return super::sdbl_completion::sdbl_completions(db, position);
        }

        // SdblKeywords - suggest SDBL keywords
        (SdblCompletionContext::SdblKeywords { prefix }, _) => {
            tracing::info!(prefix = %prefix, "completion context: SdblKeywords");
            CompleteKeywordsUseCase::execute(&prefix)
        }

        // None - no completion
        (SdblCompletionContext::None, _) => {
            tracing::info!("no completion context detected");
            return None;
        }
    };

    // Convert SdblCompletionItem to CompletionItem
    Some(items.into_iter().map(|item| item.into_completion_item()).collect())
}
