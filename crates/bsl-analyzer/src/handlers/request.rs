//! Handlers for LSP requests.
//!
//! This module implements handlers for LSP requests like
//! textDocument/definition, textDocument/references, etc.

use anyhow::Result;
use base_db::SourceDatabase;
use ide::Location as IdeLocation;
use line_index::LineIndex;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location, MarkupContent, MarkupKind,
    ReferenceParams, SemanticTokens, SemanticTokensParams, SemanticTokensResult,
};

use crate::global_state::GlobalStateSnapshot;

/// Handles textDocument/definition request.
///
/// Goes to the definition of the symbol at the cursor position.
pub fn handle_goto_definition(
    snap: GlobalStateSnapshot,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let _p = tracing::info_span!(
        "handle_goto_definition",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    // Get FileId
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    // Get text for line index
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Convert position to offset
    let offset = crate::lsp::offset(&line_index, &text, position)?;

    // Call IDE API
    let target = snap.analysis.goto_definition(file_id, offset.into());

    // Convert result
    match target {
        Some(nav_target) => {
            // Get URL for target file (may be different from source file)
            let target_url = snap.url_for_file_id(nav_target.file_id)?;

            // Get text and line index for target file
            let target_text = if nav_target.file_id == file_id {
                // Same file - reuse current text
                text.clone()
            } else {
                // Different file - read from MemDocs or database
                snap.mem_docs.get(&target_url).unwrap_or_else(|| {
                    // File not in MemDocs - read from database
                    let db = snap.analysis.database();
                    let file_text_input = db.file_text_input(nav_target.file_id);
                    file_text_input.text(db).clone()
                })
            };

            let target_line_index = LineIndex::new(&target_text);

            // Convert range
            let target_range =
                crate::lsp::range(&target_line_index, &target_text, nav_target.range)
                    .ok_or_else(|| anyhow::anyhow!("Failed to convert range"))?;

            let location = Location { uri: target_url, range: target_range };

            Ok(Some(GotoDefinitionResponse::Scalar(location)))
        }
        None => Ok(None),
    }
}

/// Handles textDocument/references request.
///
/// Finds all references to the symbol at the cursor position.
pub fn handle_find_references(
    snap: GlobalStateSnapshot,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let _p = tracing::info_span!(
        "handle_find_references",
        uri = %params.text_document_position.text_document.uri
    )
    .entered();

    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    // Get FileId
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    // Get text for line index
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Convert position to offset
    let offset = crate::lsp::offset(&line_index, &text, position)?;

    // Call IDE API
    let locations = snap.analysis.find_references(file_id, offset.into());

    // Convert results
    if locations.is_empty() {
        return Ok(None);
    }

    let lsp_locations: Vec<Location> = locations
        .into_iter()
        .filter_map(|loc| convert_location(&line_index, &text, &uri, loc))
        .collect();

    if lsp_locations.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_locations))
    }
}

/// Handles textDocument/hover request.
///
/// Returns hover information for the symbol at the cursor position.
pub fn handle_hover(snap: GlobalStateSnapshot, params: HoverParams) -> Result<Option<Hover>> {
    let _p = tracing::info_span!(
        "handle_hover",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    // Get FileId
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    // Get text for line index
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Convert position to offset
    let offset = crate::lsp::offset(&line_index, &text, position)?;

    // Call IDE API
    let hover_result = snap.analysis.hover(file_id, offset.into());

    // Convert result
    match hover_result {
        Some(result) => {
            let range = result.range.and_then(|r| crate::lsp::range(&line_index, &text, r));

            let contents = HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: result.markup,
            });

            Ok(Some(Hover { contents, range }))
        }
        None => Ok(None),
    }
}

/// Handles textDocument/completion request.
///
/// Returns code completion suggestions at the cursor position.
pub fn handle_completion(
    snap: GlobalStateSnapshot,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let _p = tracing::info_span!(
        "handle_completion",
        uri = %params.text_document_position.text_document.uri
    )
    .entered();

    tracing::info!(
        "COMPLETION REQUEST RECEIVED at line={} char={}",
        params.text_document_position.position.line,
        params.text_document_position.position.character
    );

    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    // Get FileId
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    // Get text for line index
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Log the actual line content for debugging
    let line_num = position.line as usize;
    let lines: Vec<&str> = text.lines().collect();
    if line_num < lines.len() {
        let line_text = lines[line_num];
        tracing::info!(
            "Line {} content (first 100 chars): {:?}",
            line_num,
            &line_text.chars().take(100).collect::<String>()
        );
        tracing::info!(
            "Position.character={} (UTF-16 code units from line start)",
            position.character
        );
    }

    // Convert position to offset
    let offset = crate::lsp::offset(&line_index, &text, position)?;
    tracing::info!("Converted position to offset: {:?}", offset);

    // Call IDE API with workspace root
    let items = snap.analysis.completions(file_id, offset.into(), snap.workspace_root.clone());
    tracing::info!("IDE API returned {} completion items", items.len());

    // Convert results
    if items.is_empty() {
        tracing::info!("No completion items, returning None");
        return Ok(None);
    }

    let lsp_items: Vec<CompletionItem> = items.into_iter().map(convert_completion_item).collect();
    tracing::info!("Converted to {} LSP items, returning CompletionResponse", lsp_items.len());

    Ok(Some(CompletionResponse::Array(lsp_items)))
}

/// Handles textDocument/semanticTokens/full request.
///
/// Returns semantic highlighting for the entire document.
pub fn handle_semantic_tokens_full(
    snap: GlobalStateSnapshot,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    let _p = tracing::info_span!(
        "handle_semantic_tokens_full",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;

    // Get FileId
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    // Get text for line index
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Get highlights from IDE
    let highlights = snap.analysis.highlight(file_id);

    // Convert to LSP semantic tokens (pass text for UTF-16 length calculation)
    let tokens = crate::lsp::semantic_tokens(&line_index, &text, &highlights);

    Ok(Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: None, data: tokens })))
}

/// Convert IDE Location to LSP Location.
///
/// For now, assumes all locations are in the same file.
/// TODO: Support cross-file references when we have FileId → URL mapping.
fn convert_location(
    line_index: &LineIndex,
    text: &str,
    uri: &lsp_types::Url,
    ide_loc: IdeLocation,
) -> Option<Location> {
    let range = crate::lsp::range(line_index, text, ide_loc.range)?;
    Some(Location { uri: uri.clone(), range })
}

/// Convert IDE CompletionItem to LSP CompletionItem.
fn convert_completion_item(item: ide::CompletionItem) -> CompletionItem {
    CompletionItem {
        label: item.label,
        detail: item.detail,
        kind: Some(convert_completion_kind(item.kind)),
        insert_text: Some(item.insert_text),
        documentation: item.documentation.map(lsp_types::Documentation::String),
        ..Default::default()
    }
}

/// Convert IDE CompletionItemKind to LSP CompletionItemKind.
fn convert_completion_kind(kind: ide::CompletionItemKind) -> CompletionItemKind {
    match kind {
        ide::CompletionItemKind::MdoType => CompletionItemKind::CLASS,
        ide::CompletionItemKind::MdoObject => CompletionItemKind::MODULE,
        ide::CompletionItemKind::Field => CompletionItemKind::FIELD,
        ide::CompletionItemKind::Function => CompletionItemKind::FUNCTION,
        ide::CompletionItemKind::Method => CompletionItemKind::METHOD,
        ide::CompletionItemKind::Keyword => CompletionItemKind::KEYWORD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use lsp_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams};

    use crate::global_state::GlobalState;

    fn create_test_state() -> GlobalState {
        let (sender, _receiver) = unbounded();
        GlobalState::new(sender)
    }

    #[test]
    fn test_goto_definition_not_found() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        // Insert document
        state.mem_docs.insert(uri.clone(), "Процедура Тест() КонецПроцедуры".to_string(), 1);

        let snap = state.snapshot();

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 0, character: 0 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        // Should handle gracefully even if file is not in VFS
        let result = handle_goto_definition(snap, params);
        // File not in VFS is expected to fail in tests
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[test]
    fn test_find_references_empty() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        // Insert document
        state.mem_docs.insert(uri.clone(), "Процедура Тест() КонецПроцедуры".to_string(), 1);

        let snap = state.snapshot();

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 0, character: 0 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext { include_declaration: true },
        };

        // Should handle gracefully even if file is not in VFS
        let result = handle_find_references(snap, params);
        // File not in VFS is expected to fail in tests
        assert!(result.is_err() || result.unwrap().is_none());
    }
}
