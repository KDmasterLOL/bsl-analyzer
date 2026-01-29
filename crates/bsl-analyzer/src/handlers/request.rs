//! Handlers for LSP requests.
//!
//! This module implements handlers for LSP requests like
//! textDocument/definition, textDocument/references, etc.

use anyhow::Result;
use base_db::{DiagnosticsConfigId, FileIdInput, SourceDatabase};
use ide::Location as IdeLocation;
use ide_diagnostics::file_diagnostics_query;
use line_index::LineIndex;
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionResponse, CompletionItem, CompletionItemKind,
    CompletionParams, CompletionResponse, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location,
    MarkupContent, MarkupKind, ReferenceParams, SemanticTokens, SemanticTokensParams,
    SemanticTokensResult, SignatureHelpParams,
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
            tracing::debug!(
                target_file_id = nav_target.file_id.0,
                ?nav_target.range,
                "goto_definition: found target"
            );
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
        None => {
            tracing::debug!(
                file_id = file_id.0,
                offset = u32::from(offset),
                "goto_definition: no target found"
            );
            Ok(None)
        }
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
    let start = std::time::Instant::now();
    let _p = tracing::info_span!(
        "handle_semantic_tokens_full",
        uri = %params.text_document.uri
    )
    .entered();

    // Don't block on metadata loading if VFS isn't done yet.
    // Client will re-request when ready.
    if !snap.vfs_done {
        tracing::debug!("VFS not ready, returning empty semantic tokens");
        return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })));
    }

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
    let highlight_start = std::time::Instant::now();
    let highlight_result = snap.analysis.highlight(file_id);
    let highlight_elapsed = highlight_start.elapsed();
    tracing::warn!(
        file_id = file_id.0,
        highlight_count = highlight_result.highlights.len(),
        resolved_external_files = highlight_result.resolved_external_files.len(),
        elapsed_ms = highlight_elapsed.as_millis() as u64,
        "semantic_tokens: analysis.highlight() completed"
    );

    // Convert to LSP semantic tokens (pass text for UTF-16 length calculation)
    let tokens = crate::lsp::semantic_tokens(&line_index, &text, &highlight_result.highlights);
    let total_elapsed = start.elapsed();
    tracing::warn!(
        file_id = file_id.0,
        token_count = tokens.len(),
        total_ms = total_elapsed.as_millis() as u64,
        %uri,
        "semantic_tokens: completed"
    );

    // Request preloading of external files for faster goto_definition
    if !highlight_result.resolved_external_files.is_empty() {
        use crate::global_state::Task;
        let _ = snap
            .task_sender
            .send(Task::PreloadExternalFiles { files: highlight_result.resolved_external_files });
    }

    Ok(Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: None, data: tokens })))
}

/// Handles textDocument/documentSymbol request.
pub fn handle_document_symbol(
    snap: GlobalStateSnapshot,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let _p = tracing::info_span!(
        "handle_document_symbol",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;

    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    let symbols = snap.analysis.document_symbols(file_id);

    if symbols.is_empty() {
        return Ok(None);
    }

    let lsp_symbols: Vec<lsp_types::DocumentSymbol> = symbols
        .into_iter()
        .filter_map(|s| convert_document_symbol(&line_index, &text, s))
        .collect();

    Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
}

/// Handles textDocument/signatureHelp request.
///
/// Returns signature help (parameter hints) at the cursor position.
pub fn handle_signature_help(
    snap: GlobalStateSnapshot,
    params: SignatureHelpParams,
) -> Result<Option<lsp_types::SignatureHelp>> {
    let _p = tracing::info_span!(
        "handle_signature_help",
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
    let sig_help = snap.analysis.signature_help(file_id, offset.into());

    // Convert result
    Ok(sig_help.map(to_lsp_signature_help))
}

/// Convert IDE SignatureHelp to LSP SignatureHelp.
fn to_lsp_signature_help(sh: ide::SignatureHelp) -> lsp_types::SignatureHelp {
    let parameters: Vec<_> = sh
        .parameters
        .iter()
        .map(|p| lsp_types::ParameterInformation {
            label: lsp_types::ParameterLabel::Simple(p.label.clone()),
            documentation: p.documentation.as_ref().map(|d| {
                lsp_types::Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d.clone(),
                })
            }),
        })
        .collect();

    lsp_types::SignatureHelp {
        signatures: vec![lsp_types::SignatureInformation {
            label: sh.signature,
            documentation: sh.doc.map(|d| {
                lsp_types::Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d,
                })
            }),
            parameters: Some(parameters),
            active_parameter: sh.active_parameter.map(|i| i as u32),
        }],
        active_signature: Some(0),
        active_parameter: sh.active_parameter.map(|i| i as u32),
    }
}

/// Handles textDocument/codeAction request.
///
/// Returns quick-fix code actions for diagnostics in the requested range.
pub fn handle_code_action(
    snap: GlobalStateSnapshot,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let _p = tracing::info_span!(
        "handle_code_action",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let line_index = LineIndex::new(&text);
    let range = crate::lsp::text_range(&line_index, &text, params.range)?;

    let db = snap.analysis.database();
    let file_id_input = FileIdInput::new(db, file_id);
    let config_id = DiagnosticsConfigId::new(db, snap.diagnostics_config.clone());
    let diagnostics = file_diagnostics_query(db, file_id_input, config_id);

    let mut actions = Vec::new();
    for diag in diagnostics.iter() {
        if diag.fixes.is_empty() {
            continue;
        }
        if diag.range.intersect(range).is_none() {
            continue;
        }
        for fix in &diag.fixes {
            if let Some(action) =
                crate::lsp::to_proto::code_action(&line_index, &text, &uri, diag, fix)
            {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
    }

    if actions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(actions))
    }
}

fn convert_document_symbol(
    line_index: &LineIndex,
    text: &str,
    sym: ide::DocumentSymbol,
) -> Option<lsp_types::DocumentSymbol> {
    let range = crate::lsp::range(line_index, text, sym.range)?;
    let selection_range = crate::lsp::range(line_index, text, sym.selection_range)?;

    let kind = match sym.kind {
        ide::SymbolKind::Procedure | ide::SymbolKind::Function => lsp_types::SymbolKind::FUNCTION,
        ide::SymbolKind::Variable => lsp_types::SymbolKind::VARIABLE,
        ide::SymbolKind::Region => lsp_types::SymbolKind::NAMESPACE,
    };

    let children = if sym.children.is_empty() {
        None
    } else {
        let converted: Vec<_> = sym
            .children
            .into_iter()
            .filter_map(|c| convert_document_symbol(line_index, text, c))
            .collect();
        if converted.is_empty() {
            None
        } else {
            Some(converted)
        }
    };

    #[allow(deprecated)]
    Some(lsp_types::DocumentSymbol {
        name: sym.name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    })
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
        ide::CompletionItemKind::Constant => CompletionItemKind::CONSTANT,
        ide::CompletionItemKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
    }
}

/// Handles textDocument/formatting request.
///
/// Formats the entire document.
pub fn handle_formatting(
    snap: GlobalStateSnapshot,
    params: lsp_types::DocumentFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    let _p = tracing::info_span!(
        "handle_formatting",
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

    // Get formatting config from LSP options
    let config = formatting_config_from_options(&params.options);

    // Call IDE API
    let result = snap.analysis.format_file(file_id, &config);

    // Convert edits
    if result.edits.is_empty() {
        return Ok(None);
    }

    let lsp_edits: Vec<lsp_types::TextEdit> = result
        .edits
        .into_iter()
        .filter_map(|edit| {
            let range = crate::lsp::range(&line_index, &text, edit.range)?;
            Some(lsp_types::TextEdit { range, new_text: edit.new_text })
        })
        .collect();

    if lsp_edits.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_edits))
    }
}

/// Handles textDocument/rangeFormatting request.
///
/// Formats a selected range in the document.
pub fn handle_range_formatting(
    snap: GlobalStateSnapshot,
    params: lsp_types::DocumentRangeFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    let _p = tracing::info_span!(
        "handle_range_formatting",
        uri = %params.text_document.uri
    )
    .entered();

    let total_start = std::time::Instant::now();

    let uri = params.text_document.uri;

    // Get FileId
    let start = std::time::Instant::now();
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;
    tracing::debug!("file_id_snapshot: {:?}", start.elapsed());

    // Get text for line index
    let start = std::time::Instant::now();
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    tracing::debug!("mem_docs.get: {:?}, text len: {}", start.elapsed(), text.len());

    let start = std::time::Instant::now();
    let line_index = LineIndex::new(&text);
    tracing::debug!("LineIndex::new: {:?}", start.elapsed());

    // Convert LSP range to TextRange
    let start = std::time::Instant::now();
    let range = crate::lsp::text_range(&line_index, &text, params.range)?;
    tracing::debug!("text_range conversion: {:?}, range: {:?}", start.elapsed(), range);

    // Get formatting config
    let config = formatting_config_from_options(&params.options);

    // Call IDE API
    let start = std::time::Instant::now();
    let result = snap.analysis.format_range(file_id, range, &config);
    tracing::debug!("format_range: {:?}, edits: {}", start.elapsed(), result.edits.len());

    // Convert edits
    if result.edits.is_empty() {
        tracing::debug!("total time (no edits): {:?}", total_start.elapsed());
        return Ok(None);
    }

    let start = std::time::Instant::now();
    let lsp_edits: Vec<lsp_types::TextEdit> = result
        .edits
        .into_iter()
        .filter_map(|edit| {
            let range = crate::lsp::range(&line_index, &text, edit.range)?;
            Some(lsp_types::TextEdit { range, new_text: edit.new_text })
        })
        .collect();
    tracing::debug!("convert edits: {:?}", start.elapsed());

    tracing::info!("range_formatting total: {:?}", total_start.elapsed());

    if lsp_edits.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_edits))
    }
}

/// Handles textDocument/onTypeFormatting request.
///
/// Formats when a trigger character is typed (e.g., `;`, `\n`).
pub fn handle_on_type_formatting(
    snap: GlobalStateSnapshot,
    params: lsp_types::DocumentOnTypeFormattingParams,
) -> Result<Option<Vec<lsp_types::TextEdit>>> {
    let _p = tracing::info_span!(
        "handle_on_type_formatting",
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

    // Get the typed character
    let char_typed = params.ch.chars().next().unwrap_or('\0');

    // Get formatting config
    let config = formatting_config_from_options(&params.options);

    // Call IDE API
    let edits = snap.analysis.on_type_formatting(file_id, offset.into(), char_typed, &config);

    match edits {
        Some(ide_edits) => {
            let lsp_edits: Vec<lsp_types::TextEdit> = ide_edits
                .into_iter()
                .filter_map(|edit| {
                    let range = crate::lsp::range(&line_index, &text, edit.range)?;
                    Some(lsp_types::TextEdit { range, new_text: edit.new_text })
                })
                .collect();

            if lsp_edits.is_empty() {
                Ok(None)
            } else {
                Ok(Some(lsp_edits))
            }
        }
        None => Ok(None),
    }
}

/// Creates a FormattingConfig from LSP FormattingOptions.
fn formatting_config_from_options(options: &lsp_types::FormattingOptions) -> ide::FormattingConfig {
    ide::FormattingConfig {
        use_tabs: !options.insert_spaces,
        indent_size: if options.insert_spaces { options.tab_size } else { 1 },
        continuation_indent: 1,
        space_after_comma: true,
        space_around_assignment: true,
        space_around_binary_ops: true,
        trim_trailing_whitespace: options.trim_trailing_whitespace.unwrap_or(true),
        insert_final_newline: options.insert_final_newline.unwrap_or(true),
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
