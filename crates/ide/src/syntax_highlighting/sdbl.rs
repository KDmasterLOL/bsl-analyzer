use ide_db::RootDatabase;
use syntax::{SyntaxKind, SyntaxNode, TextRange};

use crate::syntax_highlighting::{HighlightContext, HlMod, HlRange, HlTag};

pub(super) fn highlight_sdbl_in_literal<DB: RootDatabase>(
    ctx: &HighlightContext<DB>,
    literal_node: &SyntaxNode,
) -> Option<Vec<HlRange>> {
    let string_content = extract_string_content(literal_node)?;

    if !looks_like_sdbl(&string_content) {
        return None;
    }

    if has_string_concatenation(literal_node) {
        tracing::debug!("Skipping SDBL highlighting: string concatenation detected");
        return None;
    }

    let sdbl_hir_entries = ctx.db.sdbl_hir_in_file(ctx.file_id);

    let sdbl_queries = ctx.db.all_sdbl_in_file(ctx.file_id);

    if sdbl_hir_entries.is_empty() || sdbl_queries.is_empty() {
        return None;
    }

    let literal_range = literal_node.text_range();
    let sdbl_entry = find_sdbl_entry_by_range(&sdbl_hir_entries, &sdbl_queries, literal_range)?;

    let ((_expr_id, sdbl_package), (_query_expr_id, query_info)) = sdbl_entry;

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

    Some(convert_sdbl_tokens_to_highlights(
        &sdbl_package.source_map,
        &mapper,
        &query_info.query_text,
    ))
}

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
        TokenCategory::MdoType => HlTag::Class,
        TokenCategory::TableName => HlTag::Type,
        TokenCategory::UnresolvedTableName => HlTag::UnresolvedReference,
        TokenCategory::TableAlias => HlTag::Namespace,
        TokenCategory::FieldName => HlTag::Property,
        TokenCategory::UnresolvedFieldName => HlTag::UnresolvedReference,
        TokenCategory::FieldAlias => HlTag::EnumMember,
    }
}

fn convert_sdbl_tokens_to_highlights(
    source_map: &sdbl_hir::SdblSourceMap,
    mapper: &ide_diagnostics::sdbl_utils::SdblPositionMapper,
    sdbl_text: &str,
) -> Vec<HlRange> {
    let mut highlights = Vec::new();

    for (token_info, category) in source_map.all_tokens() {
        let bsl_range = mapper.map_range(token_info.range, sdbl_text);

        let tag = sdbl_category_to_tag(category);

        highlights.push(HlRange { range: bsl_range, tag, modifiers: HlMod::new() });
    }

    highlights
}

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

fn looks_like_sdbl(text: &str) -> bool {
    text.len() >= 15
        && (text.to_uppercase().contains("SELECT") || text.to_uppercase().contains("ВЫБРАТЬ"))
}

fn extract_string_content(literal_node: &SyntaxNode) -> Option<String> {
    ide_diagnostics::sdbl_utils::extract_string_content(literal_node)
}

fn has_string_concatenation(literal_node: &SyntaxNode) -> bool {
    if let Some(parent) = literal_node.parent() {
        if parent.kind() == SyntaxKind::BINARY_EXPR {
            for child in parent.children_with_tokens() {
                if child.kind() == SyntaxKind::PLUS {
                    return true;
                }
            }
        }
    }
    false
}
