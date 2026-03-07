//! SDBL semantic highlighting within BSL string literals.
//!
//! This module provides semantic highlighting for SDBL (1C Query Language) queries
//! embedded in BSL string literals. It leverages the SDBL HIR infrastructure that
//! collects token positions during lowering.
//!
//! ## Architecture
//!
//! 1. Detect SDBL in string literals using `looks_like_sdbl()` (length ≥ 15 + contains SELECT/ВЫБРАТЬ)
//! 2. Query cached SDBL HIR via `sdbl_hir_in_file_query()` (Salsa-cached)
//! 3. Extract `SdblSourceMap` which categorizes tokens by type (keywords, operators, functions)
//! 4. Map SDBL positions → BSL positions via `SdblPositionMapper`
//! 5. Convert token categories → HlTag types
//! 6. Merge with BSL highlights
//!
//! ## Token Mapping
//!
//! | SDBL TokenCategory | BSL HlTag | Examples |
//! |-------------------|-----------|----------|
//! | ClauseKeyword | Keyword | SELECT, FROM, WHERE, GROUP BY |
//! | Operator | Operator | =, <>, AND, OR, +, - |
//! | SpecialKeyword | Keyword | IN, BETWEEN, LIKE, CASE |
//! | JoinKeyword | Keyword | JOIN, INNER, LEFT, RIGHT |
//! | Modifier | Keyword | DISTINCT, TOP, UNION |
//! | AggregateFunction | Function | SUM, AVG, COUNT, MIN, MAX |

use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxNode, TextRange};

use crate::syntax_highlighting::{HighlightContext, HlMod, HlRange, HlTag};

/// Main entry point for SDBL highlighting in a string literal.
///
/// Returns `Some(Vec<HlRange>)` if the literal contains SDBL and highlighting succeeded,
/// `None` otherwise (not SDBL, or concatenated string, or other issues).
pub(super) fn highlight_sdbl_in_literal<DB: RootDatabase>(
    ctx: &HighlightContext<DB>,
    literal_node: &SyntaxNode,
) -> Option<Vec<HlRange>> {
    // 1. Extract string content
    let string_content = extract_string_content(literal_node)?;

    // 2. Check if looks like SDBL (length ≥ 15 + contains SELECT/ВЫБРАТЬ)
    if !looks_like_sdbl(&string_content) {
        return None;
    }

    // 3. Check for string concatenation (unsupported - position mapping would be incorrect)
    if has_string_concatenation(literal_node) {
        tracing::debug!("Skipping SDBL highlighting: string concatenation detected");
        return None;
    }

    // 4. Query cached SDBL HIR entries (Salsa LRU = 64)
    let sdbl_hir_entries = ctx.db.sdbl_hir_in_file(ctx.file_id);

    // 5. Also query SDBL query infos for position mapping
    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    // Early return if no SDBL queries found
    if sdbl_hir_entries.is_empty() || sdbl_queries.is_empty() {
        return None;
    }

    // 6. Find matching SDBL entry by comparing literal ranges
    // Both vectors are sorted by position, so we can zip them
    let literal_range = literal_node.text_range();
    let sdbl_entry = find_sdbl_entry_by_range(&sdbl_hir_entries, &sdbl_queries, literal_range)?;

    let ((_expr_id, sdbl_package), (_query_expr_id, query_info)) = sdbl_entry;

    // 7. Create position mapper with shared line index (optimization)
    let input = ctx.db.file_text_input(ctx.file_id);
    let bsl_source = input.text(ctx.db);
    let mapper = if let Some(ref line_starts) = ctx.line_index {
        ide_diagnostics::sdbl_utils::SdblPositionMapper::from_query_info(
            query_info,
            &bsl_source,
            line_starts,
        )
    } else {
        ide_diagnostics::sdbl_utils::SdblPositionMapper::new_from_range(
            query_info.bsl_literal_range,
            &bsl_source,
            query_info.quote_corrections.clone(),
        )
    };

    // 8. Convert tokens to highlights
    Some(convert_sdbl_tokens_to_highlights(
        &sdbl_package.source_map,
        &mapper,
        &query_info.query_text,
    ))
}

/// Converts SDBL token category to BSL highlight tag.
fn sdbl_category_to_tag(category: sdbl_hir::TokenCategory) -> HlTag {
    use sdbl_hir::TokenCategory;
    match category {
        TokenCategory::ClauseKeyword => HlTag::Keyword,
        TokenCategory::Operator => HlTag::Operator,
        TokenCategory::SpecialKeyword => HlTag::Keyword,
        TokenCategory::JoinKeyword => HlTag::Keyword,
        TokenCategory::Modifier => HlTag::Keyword,
        TokenCategory::AggregateFunction => HlTag::Function,
        TokenCategory::BuiltinFunction => HlTag::Function,
        // Identifiers
        TokenCategory::MdoType => HlTag::Class, // MDO types (Справочник, Документ)
        TokenCategory::TableName => HlTag::Type, // Object names (Валюты, Продажи)
        TokenCategory::UnresolvedTableName => HlTag::UnresolvedReference,
        TokenCategory::TableAlias => HlTag::Namespace, // Table aliases (Валюты в Валюты.Код)
        TokenCategory::FieldName => HlTag::Property,   // Field names (Код, Наименование)
        TokenCategory::UnresolvedFieldName => HlTag::UnresolvedReference,
        TokenCategory::FieldAlias => HlTag::EnumMember, // Field aliases (AS ИмяПоля)
    }
}

/// Converts SDBL source map tokens to BSL highlight ranges.
///
/// Iterates over all tokens in the source map, maps their positions from SDBL to BSL,
/// and creates HlRange for each token.
fn convert_sdbl_tokens_to_highlights(
    source_map: &sdbl_hir::SdblSourceMap,
    mapper: &ide_diagnostics::sdbl_utils::SdblPositionMapper,
    sdbl_text: &str,
) -> Vec<HlRange> {
    let mut highlights = Vec::new();

    for (token_info, category) in source_map.all_tokens() {
        // Map SDBL range → BSL range
        let bsl_range = mapper.map_range(token_info.range, sdbl_text);

        // Convert category → tag
        let tag = sdbl_category_to_tag(category);

        highlights.push(HlRange { range: bsl_range, tag, modifiers: HlMod::new() });
    }

    highlights
}

/// Finds the SDBL entry that corresponds to the given literal range.
///
/// Both `sdbl_hir_entries` and `sdbl_queries` are sorted by position in file,
/// so we can zip them and find the matching entry.
#[allow(clippy::type_complexity)]
fn find_sdbl_entry_by_range<'a>(
    sdbl_hir_entries: &'a std::sync::Arc<
        Vec<(hir::SdblExprId, std::sync::Arc<sdbl_hir::SdblPackage>)>,
    >,
    sdbl_queries: &'a std::sync::Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>>,
    literal_range: TextRange,
) -> Option<(
    &'a (hir::SdblExprId, std::sync::Arc<sdbl_hir::SdblPackage>),
    &'a (hir::SdblExprId, syntax::SdblQueryInfo),
)> {
    for (hir_entry, query_entry) in sdbl_hir_entries.iter().zip(sdbl_queries.iter()) {
        let (_query_sdbl_expr_id, query_info) = query_entry;
        if query_info.bsl_literal_range == literal_range {
            return Some((hir_entry, query_entry));
        }
    }
    None
}

/// Checks if a string looks like a SDBL query.
///
/// Criteria:
/// - Length ≥ 15 characters
/// - Contains "SELECT" or "ВЫБРАТЬ" (case-insensitive)
fn looks_like_sdbl(text: &str) -> bool {
    text.len() >= 15
        && (text.to_uppercase().contains("SELECT") || text.to_uppercase().contains("ВЫБРАТЬ"))
}

/// Extracts string content from a LITERAL node.
///
/// Handles both simple strings and multiline strings with `|` prefix.
/// Returns `None` if extraction fails (e.g., not a string literal).
fn extract_string_content(literal_node: &SyntaxNode) -> Option<String> {
    ide_diagnostics::sdbl_utils::extract_string_content(literal_node)
}

/// Checks if a string literal involves concatenation.
///
/// String concatenation makes position mapping ambiguous, so we skip highlighting.
fn has_string_concatenation(literal_node: &SyntaxNode) -> bool {
    // Check if parent is BINARY_EXPR with + operator
    if let Some(parent) = literal_node.parent() {
        if parent.kind() == SyntaxKind::BINARY_EXPR {
            // Check if operator is +
            for child in parent.children_with_tokens() {
                if child.kind() == SyntaxKind::PLUS {
                    return true;
                }
            }
        }
    }
    false
}
