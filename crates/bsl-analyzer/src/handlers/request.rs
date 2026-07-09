use anyhow::Result;
use ide::{
    DocumentHighlightKind as IdeDocumentHighlightKind, FoldingRangeKind as IdeFoldingRangeKind,
    Location as IdeLocation, RenameError,
};
use line_index::{LineIndex, TextSize};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams,
    CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse, CompletionItem,
    CompletionItemKind, CompletionParams, CompletionResponse, DocumentChanges,
    DocumentHighlight as LspDocumentHighlight, DocumentHighlightKind as LspDocumentHighlightKind,
    DocumentHighlightParams, DocumentSymbolParams, DocumentSymbolResponse,
    FoldingRange as LspFoldingRange, FoldingRangeKind as LspFoldingRangeKind, FoldingRangeParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location,
    MarkupContent, MarkupKind, OneOf, OptionalVersionedTextDocumentIdentifier,
    PrepareRenameResponse, Range, ReferenceParams, RenameParams, SemanticTokens,
    SemanticTokensParams, SemanticTokensResult, SignatureHelpParams, SymbolKind, TextDocumentEdit,
    TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};
use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::frozen_context::LatencyRequestContext;
use crate::global_state::GlobalStateSnapshot;

pub fn handle_goto_definition(
    ctx: LatencyRequestContext,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let _p = tracing::info_span!(
        "handle_goto_definition",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let source_doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = source_doc.text();
    let line_index = source_doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let target = ctx.analysis.goto_definition(file_id, offset.into());

    match target {
        Some(nav_target) => {
            tracing::debug!(
                target_file_id = nav_target.file_id.0,
                ?nav_target.range,
                "goto_definition: found target"
            );
            let target_url = ctx.url_for_file_id(nav_target.file_id)?;

            let target_text: String = if nav_target.file_id == file_id {
                text.to_string()
            } else if let Some(doc) = ctx.mem_docs.get(&target_url) {
                doc.text().to_string()
            } else {
                ctx.analysis.file_text(nav_target.file_id)
            };

            let target_line_index = LineIndex::new(&target_text);

            let target_range = crate::lsp::range_with_encoding(
                &target_line_index,
                &target_text,
                nav_target.range,
                ctx.position_encoding,
            )
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

pub fn handle_find_references(
    ctx: LatencyRequestContext,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let _p = tracing::info_span!(
        "handle_find_references",
        uri = %params.text_document_position.text_document.uri
    )
    .entered();

    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let locations = ctx.analysis.find_references(file_id, offset.into());

    if locations.is_empty() {
        return Ok(None);
    }

    let mut converter = ReferenceLocationConverter::new(&ctx, file_id, text);
    let lsp_locations: Vec<Location> =
        locations.into_iter().map(|loc| converter.convert(loc)).collect::<Result<Vec<_>>>()?;

    if lsp_locations.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_locations))
    }
}

pub fn handle_prepare_rename(
    ctx: LatencyRequestContext,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let _p =
        tracing::info_span!("handle_prepare_rename", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;
    let position = params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let Some(target) = ctx.analysis.prepare_rename(file_id, offset.into()) else {
        return Ok(None);
    };

    let range =
        crate::lsp::range_with_encoding(line_index, text, target.range, ctx.position_encoding)
            .ok_or_else(|| anyhow::anyhow!("Failed to convert rename range"))?;

    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
        range,
        placeholder: target.current_name,
    }))
}

pub fn handle_rename(
    ctx: LatencyRequestContext,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    let _p = tracing::info_span!(
        "handle_rename",
        uri = %params.text_document_position.text_document.uri
    )
    .entered();

    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;
    let new_name = params.new_name;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let locations = match ctx.analysis.rename(file_id, offset.into(), &new_name) {
        Ok(locations) => locations,
        Err(RenameError::NotRenameable) => {
            anyhow::bail!("This symbol cannot be renamed")
        }
        Err(RenameError::InvalidIdentifier(name)) => {
            anyhow::bail!("'{name}' is not a valid identifier")
        }
    };

    let mut converter = ReferenceLocationConverter::new(&ctx, file_id, text);
    let mut grouped: FxHashMap<lsp_types::Url, Vec<TextEdit>> = FxHashMap::default();
    for location in locations {
        let lsp_location = converter.convert(location)?;
        grouped
            .entry(lsp_location.uri)
            .or_default()
            .push(TextEdit { range: lsp_location.range, new_text: new_name.clone() });
    }

    let edit = if ctx.supports_workspace_edit_document_changes {
        // Versioned edits let the client discard a rename computed against a buffer it
        // has since changed. An unopened file carries `None` — its on-disk content is
        // authoritative, so there is no editor version to match.
        let document_changes = grouped
            .into_iter()
            .map(|(uri, edits)| {
                let version = ctx.mem_docs.get(&uri).map(|doc| doc.version());
                TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version },
                    edits: edits.into_iter().map(OneOf::Left).collect(),
                }
            })
            .collect();
        WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(document_changes)),
            change_annotations: None,
        }
    } else {
        WorkspaceEdit {
            changes: Some(grouped.into_iter().collect()),
            document_changes: None,
            change_annotations: None,
        }
    };

    Ok(Some(edit))
}

pub fn handle_prepare_call_hierarchy(
    ctx: LatencyRequestContext,
    params: CallHierarchyPrepareParams,
) -> Result<Option<Vec<CallHierarchyItem>>> {
    let _p = tracing::info_span!(
        "handle_prepare_call_hierarchy",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let Some(ide_item) = ctx.analysis.prepare_call_hierarchy(file_id, offset.into()) else {
        return Ok(None);
    };

    let mut converter = ReferenceLocationConverter::new(&ctx, file_id, text);
    let item = to_lsp_call_hierarchy_item(&mut converter, ide_item)?;
    Ok(Some(vec![item]))
}

pub fn handle_call_hierarchy_incoming(
    ctx: LatencyRequestContext,
    params: CallHierarchyIncomingCallsParams,
) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
    let _p =
        tracing::info_span!("handle_call_hierarchy_incoming", uri = %params.item.uri).entered();

    let item = params.item;
    let file_id = ctx.file_id_for_url(&item.uri)?;
    let text = call_hierarchy_anchor_text(&ctx, &item.uri, file_id);
    let line_index = LineIndex::new(&text);

    let offset = crate::lsp::offset_with_encoding(
        &line_index,
        &text,
        item.selection_range.start,
        ctx.position_encoding,
    )?;

    let calls = ctx.analysis.call_hierarchy_incoming(file_id, offset.into());
    if calls.is_empty() {
        return Ok(None);
    }

    let mut converter = ReferenceLocationConverter::new(&ctx, file_id, &text);
    let mut result = Vec::with_capacity(calls.len());
    for call in calls {
        // Incoming call sites live in the caller's own file.
        let caller_file = call.item.file_id;
        let from = to_lsp_call_hierarchy_item(&mut converter, call.item)?;
        let from_ranges = convert_call_ranges(&mut converter, caller_file, call.ranges)?;
        result.push(CallHierarchyIncomingCall { from, from_ranges });
    }
    Ok(Some(result))
}

pub fn handle_call_hierarchy_outgoing(
    ctx: LatencyRequestContext,
    params: CallHierarchyOutgoingCallsParams,
) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
    let _p =
        tracing::info_span!("handle_call_hierarchy_outgoing", uri = %params.item.uri).entered();

    let item = params.item;
    let file_id = ctx.file_id_for_url(&item.uri)?;
    let text = call_hierarchy_anchor_text(&ctx, &item.uri, file_id);
    let line_index = LineIndex::new(&text);

    let offset = crate::lsp::offset_with_encoding(
        &line_index,
        &text,
        item.selection_range.start,
        ctx.position_encoding,
    )?;

    let calls = ctx.analysis.call_hierarchy_outgoing(file_id, offset.into());
    if calls.is_empty() {
        return Ok(None);
    }

    let mut converter = ReferenceLocationConverter::new(&ctx, file_id, &text);
    let mut result = Vec::with_capacity(calls.len());
    for call in calls {
        let to = to_lsp_call_hierarchy_item(&mut converter, call.item)?;
        // Outgoing call sites live in the anchor method's file, not the callee's.
        let from_ranges = convert_call_ranges(&mut converter, file_id, call.ranges)?;
        result.push(CallHierarchyOutgoingCall { to, from_ranges });
    }
    Ok(Some(result))
}

fn call_hierarchy_anchor_text(
    ctx: &LatencyRequestContext,
    uri: &lsp_types::Url,
    file_id: FileId,
) -> String {
    match ctx.mem_docs.get(uri) {
        Some(doc) => doc.text().to_string(),
        None => ctx.analysis.file_text(file_id),
    }
}

fn to_lsp_call_hierarchy_item(
    converter: &mut ReferenceLocationConverter,
    item: ide::CallHierarchyItem,
) -> Result<CallHierarchyItem> {
    let range = converter.convert(IdeLocation { file_id: item.file_id, range: item.range })?;
    let selection =
        converter.convert(IdeLocation { file_id: item.file_id, range: item.selection_range })?;
    Ok(CallHierarchyItem {
        name: item.name,
        kind: if item.is_function { SymbolKind::FUNCTION } else { SymbolKind::METHOD },
        tags: None,
        detail: item.detail,
        uri: range.uri,
        range: range.range,
        selection_range: selection.range,
        data: None,
    })
}

fn convert_call_ranges(
    converter: &mut ReferenceLocationConverter,
    file_id: FileId,
    ranges: Vec<ide::TextRange>,
) -> Result<Vec<Range>> {
    ranges
        .into_iter()
        .map(|range| Ok(converter.convert(IdeLocation { file_id, range })?.range))
        .collect()
}

pub fn handle_document_highlight(
    ctx: LatencyRequestContext,
    params: DocumentHighlightParams,
) -> Result<Option<Vec<LspDocumentHighlight>>> {
    let _p = tracing::info_span!(
        "handle_document_highlight",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let highlights = ctx.analysis.document_highlights(file_id, offset.into());
    if highlights.is_empty() {
        return Ok(None);
    }

    let lsp_highlights: Vec<LspDocumentHighlight> = highlights
        .into_iter()
        .filter_map(|highlight| {
            let range = crate::lsp::range_with_encoding(
                line_index,
                text,
                highlight.range,
                ctx.position_encoding,
            )?;
            Some(LspDocumentHighlight {
                range,
                kind: Some(convert_document_highlight_kind(highlight.kind)),
            })
        })
        .collect();

    if lsp_highlights.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_highlights))
    }
}

pub fn handle_folding_range(
    ctx: LatencyRequestContext,
    params: FoldingRangeParams,
) -> Result<Option<Vec<LspFoldingRange>>> {
    let _p = tracing::info_span!("handle_folding_range", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;
    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let line_index = doc.line_index();

    let ranges = ctx.analysis.folding_ranges(file_id);
    if ranges.is_empty() {
        return Ok(None);
    }

    let lsp_ranges: Vec<LspFoldingRange> = ranges
        .into_iter()
        .filter_map(|folding_range| {
            let (start_line, end_line) = folding_range_lines(line_index, folding_range.range)?;
            Some(LspFoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: folding_range.kind.map(convert_folding_range_kind),
                collapsed_text: None,
            })
        })
        .collect();

    if lsp_ranges.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_ranges))
    }
}

pub fn handle_hover(ctx: LatencyRequestContext, params: HoverParams) -> Result<Option<Hover>> {
    let _p = tracing::info_span!(
        "handle_hover",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding)?;

    let hover_result = ctx.analysis.hover(file_id, offset.into(), ctx.diagnostics_config.locale);

    match hover_result {
        Some(result) => {
            let range = result.range.and_then(|r| {
                crate::lsp::range_with_encoding(line_index, text, r, ctx.position_encoding)
            });

            let contents = HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: result.markup,
            });

            Ok(Some(Hover { contents, range }))
        }
        None => Ok(None),
    }
}

pub fn handle_completion(
    ctx: LatencyRequestContext,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let _p = tracing::info_span!(
        "handle_completion",
        uri = %params.text_document_position.text_document.uri
    )
    .entered();

    tracing::debug!(
        "completion request at line={} char={}",
        params.text_document_position.position.line,
        params.text_document_position.position.character
    );

    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let line_num = position.line as usize;
    let lines: Vec<&str> = text.lines().collect();
    if line_num < lines.len() {
        let line_text = lines[line_num];
        tracing::debug!(
            "Line {} content (first 100 chars): {:?}",
            line_num,
            &line_text.chars().take(100).collect::<String>()
        );
        tracing::debug!(
            "Position.character={} (UTF-16 code units from line start)",
            position.character
        );
    }

    let offset =
        match crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding) {
            Ok(o) => o,
            Err(_) => {
                tracing::debug!(
                    "Position out of bounds, likely race with didChange - returning empty"
                );
                return Ok(None);
            }
        };
    tracing::debug!("Converted position to offset: {:?}", offset);

    let items = ctx.analysis.completions(
        file_id,
        offset.into(),
        ctx.workspace_root.clone(),
        ctx.diagnostics_config.locale,
    );
    tracing::debug!("IDE API returned {} completion items", items.len());

    if items.is_empty() {
        tracing::debug!("No completion items, returning None");
        return Ok(None);
    }

    // Multi-line snippet bodies carry only relative (tab) nesting; ask clients
    // that honor `InsertTextMode` to indent the continuation lines to the cursor
    // column. Clients without the capability re-indent by their own default.
    let adjust_indentation = ctx.supports_insert_text_mode_adjust_indentation;
    let lsp_items: Vec<CompletionItem> =
        items.into_iter().map(|item| convert_completion_item(item, adjust_indentation)).collect();
    tracing::debug!("Converted to {} LSP items, returning CompletionResponse", lsp_items.len());

    Ok(Some(CompletionResponse::Array(lsp_items)))
}

pub fn handle_semantic_tokens_full(
    ctx: LatencyRequestContext,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    let start = std::time::Instant::now();
    let _p = tracing::info_span!(
        "handle_semantic_tokens_full",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let highlight_start = std::time::Instant::now();
    let highlight_result = ctx.analysis.highlight(file_id);
    let highlight_elapsed = highlight_start.elapsed();
    tracing::debug!(
        file_id = file_id.0,
        highlight_count = highlight_result.highlights.len(),
        resolved_external_files = highlight_result.resolved_external_files.len(),
        elapsed_ms = highlight_elapsed.as_millis() as u64,
        "semantic_tokens: analysis.highlight() completed"
    );

    let tokens = crate::lsp::semantic_tokens_with_encoding(
        line_index,
        text,
        &highlight_result.highlights,
        ctx.position_encoding,
    );
    let total_elapsed = start.elapsed();
    tracing::debug!(
        file_id = file_id.0,
        token_count = tokens.len(),
        total_ms = total_elapsed.as_millis() as u64,
        %uri,
        "semantic_tokens: completed"
    );

    if !highlight_result.resolved_external_files.is_empty() {
        use crate::global_state::Task;
        let _ = ctx
            .task_sender
            .send(Task::PreloadExternalFiles { files: highlight_result.resolved_external_files });
    }

    Ok(Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: None, data: tokens })))
}

pub fn handle_document_symbol(
    ctx: LatencyRequestContext,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    let _p = tracing::info_span!(
        "handle_document_symbol",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let symbols = ctx.analysis.document_symbols(file_id);

    if symbols.is_empty() {
        return Ok(None);
    }

    let lsp_symbols: Vec<lsp_types::DocumentSymbol> = symbols
        .into_iter()
        .filter_map(|s| convert_document_symbol(line_index, text, s, ctx.position_encoding))
        .collect();

    Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
}

pub fn handle_signature_help(
    ctx: LatencyRequestContext,
    params: SignatureHelpParams,
) -> Result<Option<lsp_types::SignatureHelp>> {
    let _p = tracing::info_span!(
        "handle_signature_help",
        uri = %params.text_document_position_params.text_document.uri
    )
    .entered();

    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

    let offset =
        match crate::lsp::offset_with_encoding(line_index, text, position, ctx.position_encoding) {
            Ok(o) => o,
            Err(_) => {
                tracing::debug!(
                    "Position out of bounds, likely race with didChange - returning empty"
                );
                return Ok(None);
            }
        };

    let sig_help = ctx.analysis.signature_help(file_id, offset.into());

    Ok(sig_help.map(to_lsp_signature_help))
}

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

pub fn handle_code_action(
    ctx: LatencyRequestContext,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let _p = tracing::info_span!(
        "handle_code_action",
        uri = %params.text_document.uri
    )
    .entered();

    let uri = params.text_document.uri;
    let file_id = ctx.file_id_for_url(&uri)?;
    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();
    let range = crate::lsp::text_range_with_encoding(
        line_index,
        text,
        params.range,
        ctx.position_encoding,
    )?;

    let diagnostics = ctx.analysis.file_diagnostics_cached(file_id, ctx.diagnostics_config.clone());
    let only = &params.context.only;

    let mut actions = Vec::new();

    // Individual quick fixes for the diagnostics touching the requested range, plus a
    // "fix all occurrences of code X" batch (once per code) for codes with ≥2 safe fixes.
    if kind_requested(CodeActionKind::QUICKFIX.as_str(), only) {
        for diag in diagnostics.iter() {
            if diag.fixes.is_empty() || diag.range.intersect(range).is_none() {
                continue;
            }
            for fix in &diag.fixes {
                if let Some(action) = crate::lsp::to_proto::code_action_with_encoding(
                    line_index,
                    text,
                    &uri,
                    diag,
                    fix,
                    ctx.position_encoding,
                ) {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }
            }
        }

        // "Fix all occurrences of code X" — offered only for a code visible in the
        // requested range (like a normal quick fix), but merging every occurrence
        // of it in the file.
        let mut seen_codes = rustc_hash::FxHashSet::default();
        for diag in diagnostics.iter() {
            if diag.range.intersect(range).is_none() || !seen_codes.insert(diag.code) {
                continue;
            }
            if let Some(action) = fix_all_occurrences_action(
                line_index,
                text,
                &uri,
                &diagnostics,
                diag.code,
                ctx.position_encoding,
            ) {
                actions.push(CodeActionOrCommand::CodeAction(action));
            }
        }
    }

    // Whole-file `source.fixAll`: every safe fix in the file, merged without overlaps.
    // Scoped to the whole file (not the request range) as the action title promises.
    if kind_requested(crate::lsp::to_proto::FIX_ALL_BSL, only) {
        let merged = ide::batch_fixes::merge_fixes(
            diagnostics
                .iter()
                .flat_map(|diag| diag.fixes.iter())
                .filter(|fix| fix.safe_for_fix_all),
        );
        if let Some(action) = crate::lsp::to_proto::aggregate_code_action(
            line_index,
            text,
            &uri,
            "Исправить все безопасные замечания в файле".to_string(),
            CodeActionKind::new(crate::lsp::to_proto::FIX_ALL_BSL),
            &merged,
            ctx.position_encoding,
        ) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    if actions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(actions))
    }
}

/// An action kind is requested when the client sends no `only` filter, or lists a kind
/// equal to or a super-kind (dotted prefix) of it — so `source.fixAll` matches the
/// server's `source.fixAll.bsl-analyzer`.
fn kind_requested(action_kind: &str, only: &Option<Vec<CodeActionKind>>) -> bool {
    match only {
        None => true,
        Some(list) => list.iter().any(|requested| {
            let requested = requested.as_str();
            action_kind == requested || action_kind.starts_with(&format!("{requested}."))
        }),
    }
}

/// A batched quick fix that applies every safe fix of one diagnostic code in the file.
/// Emitted only when at least two such fixes exist.
fn fix_all_occurrences_action(
    line_index: &LineIndex,
    text: &str,
    uri: &lsp_types::Url,
    diagnostics: &[ide::Diagnostic],
    code: ide::DiagnosticCode,
    encoding: crate::lsp::PositionEncoding,
) -> Option<lsp_types::CodeAction> {
    let safe_fixes = || {
        diagnostics
            .iter()
            .filter(|diag| diag.code == code)
            .flat_map(|diag| diag.fixes.iter())
            .filter(|fix| fix.safe_for_fix_all && !fix.edits.is_empty())
    };
    if safe_fixes().count() < 2 {
        return None;
    }

    let merged = ide::batch_fixes::merge_fixes(safe_fixes());
    crate::lsp::to_proto::aggregate_code_action(
        line_index,
        text,
        uri,
        format!("Исправить все «{}» в файле", code.as_str()),
        CodeActionKind::QUICKFIX,
        &merged,
        encoding,
    )
}

/// Pull-model single-document diagnostics (`textDocument/diagnostic`).
///
/// Uses the same `file_diagnostics_query` the push path uses, so a pulled report is
/// identical to what the editor already shows — and unlike push it also serves closed
/// files, reading their text from the disk-backed database. The `previous_result_id`
/// fast-path returns a tiny `Unchanged` report when the diagnostics have not changed.
///
/// This is intentionally scope-independent: `WorkspaceDiagnosticsScope::Extensions`
/// governs which files the bulk `workspace/diagnostic` sweep reports, not an explicit
/// single-file request. A user who opens a base-configuration file expects its
/// diagnostics, and computing them for that one file on demand is cheap (it does not
/// trigger whole-base analysis).
pub fn handle_document_diagnostic(
    ctx: LatencyRequestContext,
    params: lsp_types::DocumentDiagnosticParams,
) -> Result<lsp_types::DocumentDiagnosticReportResult> {
    use lsp_types::{
        DocumentDiagnosticReport, DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
        RelatedFullDocumentDiagnosticReport, RelatedUnchangedDocumentDiagnosticReport,
        UnchangedDocumentDiagnosticReport,
    };

    let uri = params.text_document.uri;
    let _p = tracing::info_span!("handle_document_diagnostic", uri = %uri).entered();

    let file_id = ctx.file_id_for_url(&uri)?;

    let ide_diagnostics =
        ctx.analysis.file_diagnostics_cached(file_id, ctx.diagnostics_config.clone());
    let result_id = crate::lsp::diagnostics_result_id(&ide_diagnostics);

    if params.previous_result_id.as_deref() == Some(result_id.as_str()) {
        return Ok(DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(
            RelatedUnchangedDocumentDiagnosticReport {
                related_documents: None,
                unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                    result_id,
                },
            },
        )));
    }

    // An open buffer's overlay text (and its cached line index) reflects unsaved edits;
    // a closed file has neither, so fall back to the database's disk-backed text.
    let items = match ctx.mem_docs.get(&uri) {
        Some(doc) => crate::lsp::to_proto::diagnostics_with_encoding(
            doc.line_index(),
            doc.text(),
            &ide_diagnostics,
            ctx.position_encoding,
        ),
        None => {
            let text = ctx.analysis.file_text(file_id);
            let line_index = LineIndex::new(&text);
            crate::lsp::to_proto::diagnostics_with_encoding(
                &line_index,
                &text,
                &ide_diagnostics,
                ctx.position_encoding,
            )
        }
    };

    Ok(DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
        RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: Some(result_id),
                items,
            },
        },
    )))
}

/// Emits a `WorkDoneProgress` `End` for a started progress token on drop, so the client's progress
/// indicator is always closed — including when the sweep unwinds on a Salsa cancellation.
struct WorkDoneProgressGuard<'a> {
    ctx: &'a LatencyRequestContext,
    token: lsp_types::ProgressToken,
}

impl Drop for WorkDoneProgressGuard<'_> {
    fn drop(&mut self) {
        self.ctx.send_progress(
            &self.token,
            serde_json::to_value(lsp_types::WorkDoneProgress::End(
                lsp_types::WorkDoneProgressEnd { message: None },
            ))
            .unwrap_or_default(),
        );
    }
}

/// Pull-model whole-workspace diagnostics (`workspace/diagnostic`).
///
/// Reports diagnostics for every in-scope BSL file in one sweep. Scope comes from
/// `[features] workspaceDiagnostics`: `Extensions` reports only files under a
/// configuration-extension root (the base configuration stays loaded so cross-references
/// resolve, but base files are not reported); `All` reports the whole configuration.
///
/// Runs on the task pool via `on_latency`, which also provides the boot gate (declined
/// with `ContentModified` until the workspace has loaded) and cancellation: a concurrent
/// edit bumps the Salsa revision and the sweep unwinds, so the client simply re-pulls.
/// Each file's `previousResultId` drives a tiny `Unchanged` report when nothing changed.
///
/// When the client supplies a `partialResultToken`, reports are streamed in chunks via
/// `$/progress` as they are computed (so the Problems panel fills progressively instead of
/// waiting for a minutes-long `all` sweep), and the final response is an empty report. A
/// `workDoneToken` drives a work-done progress indicator over the same sweep.
pub fn handle_workspace_diagnostic(
    ctx: LatencyRequestContext,
    params: lsp_types::WorkspaceDiagnosticParams,
) -> Result<lsp_types::WorkspaceDiagnosticReportResult> {
    use lsp_types::{
        WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressReport, WorkspaceDiagnosticReport,
        WorkspaceDiagnosticReportResult,
    };

    let _p = tracing::info_span!("handle_workspace_diagnostic").entered();

    let scope =
        ctx.project.as_ref().map(|p| p.config.features.workspace_diagnostics).unwrap_or_default();
    if !scope.is_enabled() {
        return Ok(WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport::default()));
    }

    // Extension roots for scope filtering. For `Extensions`, only files under one of these
    // are reported; `All` reports every BSL file.
    let ext_roots: Vec<&std::path::Path> = ctx
        .project
        .as_ref()
        .map(|p| p.extension_paths().iter().map(|(_, path)| path.as_path()).collect())
        .unwrap_or_default();

    let mut file_ids: Vec<vfs::FileId> = ctx
        .file_paths
        .iter()
        .filter(|(_, path)| path_in_workspace_scope(path, scope, &ext_roots))
        .map(|(file_id, _)| file_id)
        .collect();
    // Stable report order across pulls (protocol-agnostic, but keeps diffs/logs sane).
    file_ids.sort_by_key(|f| f.0);

    let previous: FxHashMap<lsp_types::Url, String> =
        params.previous_result_ids.into_iter().map(|p| (p.uri, p.value)).collect();

    let total = file_ids.len();
    tracing::info!(scope = ?scope, files = total, "workspace diagnostics sweep");

    let partial_token = params.partial_result_params.partial_result_token;
    let work_done_token = params.work_done_progress_params.work_done_token;

    // The client created the work-done token, so the server drives it directly (no create
    // round-trip). The guard sends `End` on drop — including a Salsa-cancellation unwind — so the
    // client's progress indicator never stays stuck if the sweep is aborted mid-flight.
    let _work_done_guard = work_done_token.as_ref().map(|token| {
        ctx.send_progress(
            token,
            serde_json::to_value(WorkDoneProgress::Begin(WorkDoneProgressBegin {
                title: "Workspace diagnostics".to_string(),
                cancellable: Some(false),
                message: Some(format!("0/{total}")),
                percentage: Some(0),
            }))
            .unwrap_or_default(),
        );
        WorkDoneProgressGuard { ctx: &ctx, token: token.clone() }
    });

    // Compute in chunks so streaming clients see progress and the heavy per-file memos fall out
    // of the Salsa LRU between chunks instead of all piling up at once.
    const CHUNK: usize = 64;
    let mut collected = Vec::new();
    let mut done = 0usize;
    for chunk in file_ids.chunks(CHUNK) {
        let computed = ctx.analysis.workspace_diagnostics(chunk, ctx.diagnostics_config.clone());
        let chunk_items: Vec<_> = computed
            .into_iter()
            .filter_map(|(file_id, diagnostics)| {
                workspace_report_item(&ctx, file_id, &diagnostics, &previous)
            })
            .collect();
        done += chunk.len();

        if let Some(token) = &partial_token {
            ctx.send_progress(token, serde_json::json!({ "items": chunk_items }));
        } else {
            collected.extend(chunk_items);
        }
        if let Some(token) = &work_done_token {
            let percentage = ((done as f64 / total.max(1) as f64) * 100.0) as u32;
            ctx.send_progress(
                token,
                serde_json::to_value(WorkDoneProgress::Report(WorkDoneProgressReport {
                    cancellable: Some(false),
                    message: Some(format!("{done}/{total}")),
                    percentage: Some(percentage),
                }))
                .unwrap_or_default(),
            );
        }
    }

    // `End` is emitted by `_work_done_guard` on drop (here and on a cancellation unwind alike).

    // Streamed reports were already delivered via `$/progress`; the response is then an empty
    // report. Without a partial-result token everything is returned in the response instead.
    let items = if partial_token.is_some() { Vec::new() } else { collected };
    Ok(WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items }))
}

/// Builds one `workspace/diagnostic` report entry for a computed file: an `Unchanged` stub when
/// the client's `previousResultId` still matches, otherwise a `Full` report. Returns `None` only
/// when the file id has no resolvable URL (it was removed from the frozen snapshot).
fn workspace_report_item(
    ctx: &LatencyRequestContext,
    file_id: vfs::FileId,
    diagnostics: &[ide::Diagnostic],
    previous: &FxHashMap<lsp_types::Url, String>,
) -> Option<lsp_types::WorkspaceDocumentDiagnosticReport> {
    use lsp_types::{
        FullDocumentDiagnosticReport, UnchangedDocumentDiagnosticReport,
        WorkspaceDocumentDiagnosticReport, WorkspaceFullDocumentDiagnosticReport,
        WorkspaceUnchangedDocumentDiagnosticReport,
    };

    let url = ctx.url_for_file_id(file_id).ok()?;
    let result_id = crate::lsp::diagnostics_result_id(diagnostics);
    // The LSP spec wants the buffer version for an open document (and `None` only for a closed
    // one), so a client can associate or discard the report against its buffer.
    let open_doc = ctx.mem_docs.get(&url);
    let version = open_doc.map(|doc| doc.version() as i64);

    if previous.get(&url).map(String::as_str) == Some(result_id.as_str()) {
        return Some(WorkspaceDocumentDiagnosticReport::Unchanged(
            WorkspaceUnchangedDocumentDiagnosticReport {
                uri: url,
                version,
                unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                    result_id,
                },
            },
        ));
    }

    // An open buffer's overlay text (and cached line index) reflects unsaved edits; a closed
    // file has neither, so fall back to the database's disk-backed text.
    let lsp_items = match open_doc {
        Some(doc) => crate::lsp::to_proto::diagnostics_with_encoding(
            doc.line_index(),
            doc.text(),
            diagnostics,
            ctx.position_encoding,
        ),
        None => {
            // A closed file's disk-backed text read (and conversion) can panic if the file was
            // deleted/rewritten mid-sweep. Catch it so one racing file does not fail the whole
            // request (and, when streaming, discard already-sent chunks); a `salsa::Cancelled`
            // is a real abort and must keep unwinding.
            let converted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let text = ctx.analysis.file_text(file_id);
                let line_index = LineIndex::new(&text);
                crate::lsp::to_proto::diagnostics_with_encoding(
                    &line_index,
                    &text,
                    diagnostics,
                    ctx.position_encoding,
                )
            }));
            match converted {
                Ok(lsp_items) => lsp_items,
                Err(payload) if payload.is::<salsa::Cancelled>() => {
                    std::panic::resume_unwind(payload)
                }
                Err(_) => {
                    tracing::warn!(
                        file_id = file_id.0,
                        "workspace diagnostics: skipping file after a text-read panic"
                    );
                    return None;
                }
            }
        }
    };

    Some(WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
        uri: url,
        version,
        full_document_diagnostic_report: FullDocumentDiagnosticReport {
            result_id: Some(result_id),
            items: lsp_items,
        },
    }))
}

/// Whether a file belongs to the `workspace/diagnostic` sweep for the given scope.
/// Only BSL source files are ever in scope; `Extensions` additionally requires the file
/// to live under one of the configuration-extension roots.
pub(crate) fn path_in_workspace_scope(
    path: &std::path::Path,
    scope: project_model::WorkspaceDiagnosticsScope,
    ext_roots: &[&std::path::Path],
) -> bool {
    use project_model::WorkspaceDiagnosticsScope as S;
    if !project_model::is_bsl_source_path(path) {
        return false;
    }
    match scope {
        S::All => true,
        S::Extensions => ext_roots.iter().any(|root| path.starts_with(root)),
        S::Off => false,
    }
}

fn convert_document_symbol(
    line_index: &LineIndex,
    text: &str,
    sym: ide::DocumentSymbol,
    encoding: crate::lsp::PositionEncoding,
) -> Option<lsp_types::DocumentSymbol> {
    let range = crate::lsp::range_with_encoding(line_index, text, sym.range, encoding)?;
    let selection_range =
        crate::lsp::range_with_encoding(line_index, text, sym.selection_range, encoding)?;

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
            .filter_map(|c| convert_document_symbol(line_index, text, c, encoding))
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

struct ReferenceLocationConverter<'ctx> {
    ctx: &'ctx LatencyRequestContext,
    source_file_id: FileId,
    source_text: String,
    target_files: FxHashMap<FileId, ReferenceTargetFile>,
}

struct ReferenceTargetFile {
    uri: lsp_types::Url,
    text: String,
    line_index: LineIndex,
}

impl<'ctx> ReferenceLocationConverter<'ctx> {
    fn new(ctx: &'ctx LatencyRequestContext, source_file_id: FileId, source_text: &str) -> Self {
        Self {
            ctx,
            source_file_id,
            source_text: source_text.to_string(),
            target_files: FxHashMap::default(),
        }
    }

    fn convert(&mut self, ide_loc: IdeLocation) -> Result<Location> {
        let encoding = self.ctx.position_encoding;
        let target = self.target_file(ide_loc.file_id)?;
        let range = crate::lsp::range_with_encoding(
            &target.line_index,
            &target.text,
            ide_loc.range,
            encoding,
        )
        .ok_or_else(|| anyhow::anyhow!("Failed to convert reference range"))?;
        Ok(Location { uri: target.uri.clone(), range })
    }

    fn target_file(&mut self, file_id: FileId) -> Result<&ReferenceTargetFile> {
        if !self.target_files.contains_key(&file_id) {
            let uri = self.ctx.url_for_file_id(file_id)?;
            let text = if file_id == self.source_file_id {
                self.source_text.clone()
            } else if let Some(doc) = self.ctx.mem_docs.get(&uri) {
                doc.text().to_string()
            } else {
                self.ctx.analysis.file_text(file_id)
            };
            let line_index = LineIndex::new(&text);
            self.target_files.insert(file_id, ReferenceTargetFile { uri, text, line_index });
        }

        Ok(self
            .target_files
            .get(&file_id)
            .expect("reference target file must be cached after insertion"))
    }
}

fn convert_document_highlight_kind(kind: IdeDocumentHighlightKind) -> LspDocumentHighlightKind {
    match kind {
        IdeDocumentHighlightKind::Text => LspDocumentHighlightKind::TEXT,
        IdeDocumentHighlightKind::Read => LspDocumentHighlightKind::READ,
        IdeDocumentHighlightKind::Write => LspDocumentHighlightKind::WRITE,
    }
}

fn convert_folding_range_kind(kind: IdeFoldingRangeKind) -> LspFoldingRangeKind {
    match kind {
        IdeFoldingRangeKind::Region => LspFoldingRangeKind::Region,
    }
}

fn folding_range_lines(line_index: &LineIndex, range: ide::TextRange) -> Option<(u32, u32)> {
    if range.is_empty() {
        return None;
    }

    let start_line = line_index.try_line_col(range.start())?.line;
    let end_offset = range.end() - TextSize::from(1);
    let end_line = line_index.try_line_col(end_offset)?.line;
    (end_line > start_line).then_some((start_line, end_line))
}

fn convert_completion_item(item: ide::CompletionItem, adjust_indentation: bool) -> CompletionItem {
    let has_snippet = item.insert_text.contains('$');
    // Multi-line snippet bodies carry only relative (tab) nesting. Ask capable
    // clients to adjust the continuation lines' leading whitespace to the cursor
    // column so the block isn't flushed to column 0. Single-line snippets and
    // plain text need no adjustment.
    let insert_text_mode = if adjust_indentation && has_snippet && item.insert_text.contains('\n') {
        Some(lsp_types::InsertTextMode::ADJUST_INDENTATION)
    } else {
        None
    };
    CompletionItem {
        label: item.label,
        detail: item.detail,
        kind: Some(convert_completion_kind(item.kind)),
        insert_text: Some(item.insert_text),
        insert_text_format: if has_snippet {
            Some(lsp_types::InsertTextFormat::SNIPPET)
        } else {
            None
        },
        insert_text_mode,
        documentation: item.documentation.map(lsp_types::Documentation::String),
        sort_text: item.sort_text,
        filter_text: item.filter_text,
        label_details: item
            .source
            .map(|s| lsp_types::CompletionItemLabelDetails { detail: None, description: Some(s) }),
        ..Default::default()
    }
}

fn convert_completion_kind(kind: ide::CompletionItemKind) -> CompletionItemKind {
    match kind {
        ide::CompletionItemKind::MdoType => CompletionItemKind::CLASS,
        ide::CompletionItemKind::MdoObject => CompletionItemKind::MODULE,
        ide::CompletionItemKind::Field => CompletionItemKind::FIELD,
        ide::CompletionItemKind::Property => CompletionItemKind::PROPERTY,
        ide::CompletionItemKind::Function => CompletionItemKind::FUNCTION,
        ide::CompletionItemKind::Method => CompletionItemKind::METHOD,
        ide::CompletionItemKind::Keyword => CompletionItemKind::KEYWORD,
        ide::CompletionItemKind::Constant => CompletionItemKind::CONSTANT,
        ide::CompletionItemKind::EnumMember => CompletionItemKind::ENUM_MEMBER,
        ide::CompletionItemKind::Constructor => CompletionItemKind::CONSTRUCTOR,
        ide::CompletionItemKind::Snippet => CompletionItemKind::SNIPPET,
    }
}

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

    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    let config = formatting_config_from_options(&params.options);

    let result = snap.analysis.format_file(file_id, &config);
    tracing::debug!("format_file: {} edits", result.edits.len());

    if result.edits.is_empty() {
        return Ok(None);
    }

    let lsp_edits: Vec<lsp_types::TextEdit> = result
        .edits
        .into_iter()
        .filter_map(|edit| {
            let range = crate::lsp::range_with_encoding(
                &line_index,
                &text,
                edit.range,
                snap.position_encoding,
            )?;
            tracing::trace!("edit {:?} → {:?}", edit.range, truncate_edit_preview(&edit.new_text));
            Some(lsp_types::TextEdit { range, new_text: edit.new_text })
        })
        .collect();

    if lsp_edits.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_edits))
    }
}

fn truncate_edit_preview(s: &str) -> String {
    let escaped: String = s
        .chars()
        .map(|c| match c {
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            c => c.to_string(),
        })
        .collect();
    if escaped.chars().count() > 60 {
        let head: String = escaped.chars().take(57).collect();
        format!("{}...", head)
    } else {
        escaped
    }
}

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

    let start = std::time::Instant::now();
    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;
    tracing::debug!("file_id_snapshot: {:?}", start.elapsed());

    let start = std::time::Instant::now();
    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    tracing::debug!("mem_docs.get: {:?}, text len: {}", start.elapsed(), text.len());

    let start = std::time::Instant::now();
    let line_index = LineIndex::new(&text);
    tracing::debug!("LineIndex::new: {:?}", start.elapsed());

    let start = std::time::Instant::now();
    let range = crate::lsp::text_range_with_encoding(
        &line_index,
        &text,
        params.range,
        snap.position_encoding,
    )?;
    tracing::debug!("text_range conversion: {:?}, range: {:?}", start.elapsed(), range);

    let config = formatting_config_from_options(&params.options);

    let start = std::time::Instant::now();
    let result = snap.analysis.format_range(file_id, range, &config);
    tracing::debug!("format_range: {:?}, edits: {}", start.elapsed(), result.edits.len());

    if result.edits.is_empty() {
        tracing::debug!("total time (no edits): {:?}", total_start.elapsed());
        return Ok(None);
    }

    let start = std::time::Instant::now();
    let lsp_edits: Vec<lsp_types::TextEdit> = result
        .edits
        .into_iter()
        .filter_map(|edit| {
            let range = crate::lsp::range_with_encoding(
                &line_index,
                &text,
                edit.range,
                snap.position_encoding,
            )?;
            tracing::trace!("edit {:?} → {:?}", edit.range, truncate_edit_preview(&edit.new_text));
            Some(lsp_types::TextEdit { range, new_text: edit.new_text })
        })
        .collect();
    tracing::debug!("convert edits: {:?}", start.elapsed());

    tracing::debug!("range_formatting total: {:?}", total_start.elapsed());

    if lsp_edits.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lsp_edits))
    }
}

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

    let file_id = crate::lsp::file_id_snapshot(&snap, &uri)?;

    let text = snap
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    let offset =
        crate::lsp::offset_with_encoding(&line_index, &text, position, snap.position_encoding)?;

    let char_typed = params.ch.chars().next().unwrap_or('\0');

    let config = formatting_config_from_options(&params.options);

    let edits = snap.analysis.on_type_formatting(file_id, offset.into(), char_typed, &config);

    match edits {
        Some(ide_edits) => {
            let lsp_edits: Vec<lsp_types::TextEdit> = ide_edits
                .into_iter()
                .filter_map(|edit| {
                    let range = crate::lsp::range_with_encoding(
                        &line_index,
                        &text,
                        edit.range,
                        snap.position_encoding,
                    )?;
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
    use std::sync::Arc;
    use vfs::VfsPath;

    use crate::frozen_context::{FrozenFilePaths, LatencyRequestContext};
    use crate::global_state::GlobalState;

    #[test]
    fn workspace_scope_filters_by_extension_root() {
        use project_model::WorkspaceDiagnosticsScope::{All, Extensions, Off};
        use std::path::Path;

        let ext_root = Path::new("/proj/src/cfe/ExtA");
        let ext_roots = [ext_root];
        let ext_file = Path::new("/proj/src/cfe/ExtA/CommonModules/M/Ext/Module.bsl");
        let base_file = Path::new("/proj/src/cf/CommonModules/M/Ext/Module.bsl");
        let xml_file = Path::new("/proj/src/cfe/ExtA/Configuration.xml");

        // Extensions: only BSL files under an extension root.
        assert!(path_in_workspace_scope(ext_file, Extensions, &ext_roots));
        assert!(!path_in_workspace_scope(base_file, Extensions, &ext_roots));
        assert!(!path_in_workspace_scope(xml_file, Extensions, &ext_roots), "non-BSL excluded");

        // All: every BSL file, extension or base.
        assert!(path_in_workspace_scope(ext_file, All, &ext_roots));
        assert!(path_in_workspace_scope(base_file, All, &ext_roots));
        assert!(!path_in_workspace_scope(xml_file, All, &ext_roots), "non-BSL still excluded");

        // Off: nothing (defensive — the handler returns early before this).
        assert!(!path_in_workspace_scope(ext_file, Off, &ext_roots));
    }

    fn snippet_item(insert_text: &str) -> ide::CompletionItem {
        ide::CompletionItem {
            label: "Если".into(),
            detail: None,
            kind: ide::CompletionItemKind::Snippet,
            insert_text: insert_text.into(),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        }
    }

    #[test]
    fn multiline_snippet_requests_adjust_indentation_when_supported() {
        let item =
            convert_completion_item(snippet_item("Если ${1:Усл} Тогда\n\t$0\nКонецЕсли;"), true);
        assert_eq!(item.insert_text_format, Some(lsp_types::InsertTextFormat::SNIPPET));
        assert_eq!(item.insert_text_mode, Some(lsp_types::InsertTextMode::ADJUST_INDENTATION));
    }

    #[test]
    fn multiline_snippet_omits_mode_when_unsupported() {
        let item =
            convert_completion_item(snippet_item("Если ${1:Усл} Тогда\n\t$0\nКонецЕсли;"), false);
        assert_eq!(item.insert_text_mode, None);
    }

    #[test]
    fn single_line_snippet_never_sets_mode() {
        let item = convert_completion_item(snippet_item("ВызватьИсключение;$0"), true);
        assert_eq!(item.insert_text_mode, None);
    }

    fn create_test_state() -> GlobalState {
        let (sender, _receiver) = unbounded();
        GlobalState::new(sender)
    }

    fn latency_ctx(state: &GlobalState) -> LatencyRequestContext {
        LatencyRequestContext {
            analysis: state.analysis_host.analysis(),
            workspace_root: state.workspace_root.clone(),
            project: state.project.clone(),
            diagnostics_config: state.diagnostics_config.clone(),
            position_encoding: state.position_encoding,
            supports_insert_text_mode_adjust_indentation: state
                .supports_insert_text_mode_adjust_indentation,
            supports_workspace_edit_document_changes: state
                .supports_workspace_edit_document_changes,
            task_sender: state.task_pool.pool.sender.clone(),
            client_sender: state.sender.clone(),
            mem_docs: state.mem_docs.freeze(),
            file_paths: FrozenFilePaths::freeze(&state.vfs.read()),
        }
    }

    #[test]
    fn test_goto_definition_not_found() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        state.mem_docs.insert(uri.clone(), "Процедура Тест() КонецПроцедуры".to_string(), 1);

        let ctx = latency_ctx(&state);

        let params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 0, character: 0 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = handle_goto_definition(ctx, params);
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[test]
    fn goto_definition_context_frozen_against_main_thread_mutation() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///frozen.bsl").unwrap();
        state.mem_docs.insert(uri.clone(), "original".to_string(), 1);

        let ctx = latency_ctx(&state);

        state.mem_docs.update(
            &uri,
            vec![lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "rewritten".to_string(),
            }],
        );

        let doc = ctx.mem_docs.get(&uri).expect("document must be in frozen view");
        assert_eq!(doc.text(), "original");
        assert_eq!(doc.version(), 1);

        assert_eq!(state.mem_docs.get(&uri).as_deref(), Some("rewritten"));
    }

    #[test]
    fn test_find_references_empty() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        state.mem_docs.insert(uri.clone(), "Процедура Тест() КонецПроцедуры".to_string(), 1);

        let ctx = latency_ctx(&state);

        let params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 0, character: 0 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext { include_declaration: true },
        };

        let result = handle_find_references(ctx, params);
        assert!(result.is_err() || result.unwrap().is_none());
    }

    fn open_source(state: &mut GlobalState, uri: &lsp_types::Url, source: &str) {
        state.mem_docs.insert(uri.clone(), source.to_string(), 1);
        let open_file_id = state.vfs_file_for_url(uri).unwrap();
        state.open_files.insert(open_file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(uri.to_file_path().unwrap()),
                Some(Arc::from(source)),
            );
        }
        state.process_changes(false);
    }

    #[test]
    fn prepare_rename_reports_placeholder_for_local_variable() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///rename.bsl").unwrap();
        let source =
            "Процедура Тест()\n    Перем МояПеременная;\n    МояПеременная = 10;\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let ctx = latency_ctx(&state);
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position: Position { line: 1, character: 10 },
        };

        let result = handle_prepare_rename(ctx, params).unwrap().unwrap();
        match result {
            PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } => {
                assert_eq!(placeholder, "МояПеременная");
            }
            other => panic!("expected RangeWithPlaceholder, got {other:?}"),
        }
    }

    #[test]
    fn rename_local_variable_edits_every_occurrence() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///rename.bsl").unwrap();
        let source = "Процедура Тест()\n    Перем МояПеременная;\n    МояПеременная = 10;\n    Результат = МояПеременная;\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let ctx = latency_ctx(&state);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line: 1, character: 10 },
            },
            new_name: "Итог".to_string(),
            work_done_progress_params: Default::default(),
        };

        let edit = handle_rename(ctx, params).unwrap().unwrap();
        let changes = edit.changes.expect("workspace edit must carry changes");
        let edits = changes.get(&uri).expect("edits for the renamed file");
        assert_eq!(edits.len(), 3, "declaration + 2 usages");
        assert!(edits.iter().all(|e| e.new_text == "Итог"));
    }

    #[test]
    fn rename_uses_versioned_document_changes_when_supported() {
        let mut state = create_test_state();
        state.init_empty_source_root();
        state.supports_workspace_edit_document_changes = true;

        let uri = lsp_types::Url::parse("file:///rename.bsl").unwrap();
        let source =
            "Процедура Тест()\n    Перем МояПеременная;\n    МояПеременная = 10;\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let ctx = latency_ctx(&state);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position { line: 1, character: 10 },
            },
            new_name: "Итог".to_string(),
            work_done_progress_params: Default::default(),
        };

        let edit = handle_rename(ctx, params).unwrap().unwrap();
        assert!(edit.changes.is_none(), "versioned path must not populate changes");
        match edit.document_changes.expect("document_changes must be present") {
            DocumentChanges::Edits(edits) => {
                assert_eq!(edits.len(), 1);
                assert_eq!(edits[0].text_document.uri, uri);
                assert_eq!(edits[0].text_document.version, Some(1));
                assert_eq!(edits[0].edits.len(), 2, "declaration + 1 usage");
            }
            other => panic!("expected edits, got {other:?}"),
        }
    }

    #[test]
    fn rename_rejects_invalid_identifier() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///rename.bsl").unwrap();
        let source =
            "Процедура Тест()\n    Перем МояПеременная;\n    МояПеременная = 10;\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let ctx = latency_ctx(&state);
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 1, character: 10 },
            },
            new_name: "Если".to_string(),
            work_done_progress_params: Default::default(),
        };

        assert!(handle_rename(ctx, params).is_err());
    }

    fn call_hierarchy_item_at(
        state: &GlobalState,
        uri: &lsp_types::Url,
        position: Position,
    ) -> CallHierarchyItem {
        let ctx = latency_ctx(state);
        let params = CallHierarchyPrepareParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position,
            },
            work_done_progress_params: Default::default(),
        };
        let mut items = handle_prepare_call_hierarchy(ctx, params).unwrap().unwrap();
        assert_eq!(items.len(), 1);
        items.pop().unwrap()
    }

    #[test]
    fn prepare_call_hierarchy_reports_method_item() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///ch.bsl").unwrap();
        let source = "Процедура Помощник()\nКонецПроцедуры\n\nПроцедура Первый()\n    Помощник();\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let item = call_hierarchy_item_at(&state, &uri, Position { line: 0, character: 10 });
        assert_eq!(item.name, "Помощник");
        assert_eq!(item.kind, SymbolKind::METHOD);
        assert_eq!(item.selection_range.start, Position { line: 0, character: 10 });
    }

    #[test]
    fn call_hierarchy_outgoing_reports_callee_and_ranges() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///ch.bsl").unwrap();
        let source = "Процедура Помощник()\nКонецПроцедуры\n\nПроцедура Первый()\n    Помощник();\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        // Anchor on Первый (line 3), which calls Помощник once.
        let item = call_hierarchy_item_at(&state, &uri, Position { line: 3, character: 10 });

        let ctx = latency_ctx(&state);
        let params = CallHierarchyOutgoingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let calls = handle_call_hierarchy_outgoing(ctx, params).unwrap().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].to.name, "Помощник");
        assert_eq!(calls[0].from_ranges.len(), 1);
        // The call site sits on line 4 inside Первый's body.
        assert_eq!(calls[0].from_ranges[0].start.line, 4);
    }

    #[test]
    fn call_hierarchy_incoming_reports_callers() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///ch.bsl").unwrap();
        let source = "Процедура Помощник()\nКонецПроцедуры\n\nПроцедура Первый()\n    Помощник();\nКонецПроцедуры\n";
        open_source(&mut state, &uri, source);

        let item = call_hierarchy_item_at(&state, &uri, Position { line: 0, character: 10 });

        let ctx = latency_ctx(&state);
        let params = CallHierarchyIncomingCallsParams {
            item,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };
        let calls = handle_call_hierarchy_incoming(ctx, params).unwrap().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].from.name, "Первый");
        assert_eq!(calls[0].from_ranges.len(), 1);
    }

    #[test]
    fn document_highlight_returns_ranges_and_kinds() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///highlight.bsl").unwrap();
        let source = r#"
Процедура Тест()
    Перем МояПеременная;

    МояПеременная = 10;
    Сообщить(МояПеременная);
КонецПроцедуры
"#;

        state.mem_docs.insert(uri.clone(), source.to_string(), 1);
        let open_file_id = state.vfs_file_for_url(&uri).unwrap();
        state.open_files.insert(open_file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(uri.to_file_path().unwrap()),
                Some(Arc::from(source)),
            );
        }
        state.process_changes(false);

        let ctx = latency_ctx(&state);
        let params = DocumentHighlightParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 2, character: 10 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = handle_document_highlight(ctx, params).unwrap().unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].kind, Some(LspDocumentHighlightKind::TEXT));
        assert_eq!(result[0].range.start, Position { line: 2, character: 10 });
        assert_eq!(result[1].kind, Some(LspDocumentHighlightKind::WRITE));
        assert_eq!(result[1].range.start, Position { line: 4, character: 4 });
        assert_eq!(result[2].kind, Some(LspDocumentHighlightKind::READ));
        assert_eq!(result[2].range.start, Position { line: 5, character: 13 });
    }

    #[test]
    fn folding_range_returns_lines_and_region_kind() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///folding.bsl").unwrap();
        let source = "#Область Public\nПроцедура Тест()\n    Если Истина Тогда\n        Сообщить(1);\n    КонецЕсли;\nКонецПроцедуры\n#КонецОбласти";

        state.mem_docs.insert(uri.clone(), source.to_string(), 1);
        let open_file_id = state.vfs_file_for_url(&uri).unwrap();
        state.open_files.insert(open_file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(uri.to_file_path().unwrap()),
                Some(Arc::from(source)),
            );
        }
        state.process_changes(false);

        let ctx = latency_ctx(&state);
        let params = FoldingRangeParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = handle_folding_range(ctx, params).unwrap().unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].start_line, 0);
        assert_eq!(result[0].end_line, 6);
        assert_eq!(result[0].kind, Some(LspFoldingRangeKind::Region));
        assert_eq!(result[1].start_line, 1);
        assert_eq!(result[1].end_line, 5);
        assert_eq!(result[1].kind, None);
        assert_eq!(result[2].start_line, 2);
        assert_eq!(result[2].end_line, 4);
        assert_eq!(result[2].kind, None);
    }

    fn setup_code_action_doc(source: &str) -> (GlobalState, lsp_types::Url) {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///fixall.bsl").unwrap();
        state.mem_docs.insert(uri.clone(), source.to_string(), 1);
        let open_file_id = state.vfs_file_for_url(&uri).unwrap();
        state.open_files.insert(open_file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(uri.to_file_path().unwrap()),
                Some(Arc::from(source)),
            );
        }
        state.process_changes(false);
        (state, uri)
    }

    fn code_action_params(
        uri: lsp_types::Url,
        only: Option<Vec<CodeActionKind>>,
    ) -> CodeActionParams {
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: lsp_types::Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 4, character: 0 },
            },
            context: lsp_types::CodeActionContext { diagnostics: vec![], only, trigger_kind: None },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    fn actions_of_kind<'a>(
        actions: &'a [CodeActionOrCommand],
        kind: &str,
    ) -> Vec<&'a lsp_types::CodeAction> {
        actions
            .iter()
            .filter_map(|action| match action {
                CodeActionOrCommand::CodeAction(ca)
                    if ca.kind.as_ref().is_some_and(|k| k.as_str() == kind) =>
                {
                    Some(ca)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn code_action_offers_source_fix_all_over_the_whole_file() {
        let source =
            "Процедура Тест()\n    ЭтаФорма.Закрыть();\n    ЭтаФорма.Открыть();\nКонецПроцедуры\n";
        let (state, uri) = setup_code_action_doc(source);
        let ctx = latency_ctx(&state);

        let result = handle_code_action(ctx, code_action_params(uri, None)).unwrap().unwrap();

        // Two individual quick fixes (one per `ЭтаФорма`).
        let quickfixes = actions_of_kind(&result, "quickfix");
        assert!(
            quickfixes.iter().filter(|ca| ca.diagnostics.is_some()).count() >= 2,
            "expected a quick fix per occurrence, got {result:#?}"
        );

        // One aggregate `source.fixAll` action covering both occurrences.
        let fix_all = actions_of_kind(&result, crate::lsp::to_proto::FIX_ALL_BSL);
        assert_eq!(fix_all.len(), 1, "expected exactly one source.fixAll action");
        let edits = fix_all[0]
            .edit
            .as_ref()
            .and_then(|we| we.changes.as_ref())
            .and_then(|c| c.values().next())
            .expect("fixAll must carry edits");
        assert_eq!(edits.len(), 2, "fixAll must merge both occurrences");
        assert!(edits.iter().all(|e| e.new_text == "ЭтотОбъект"));

        // One "fix all occurrences of this code" batched quick fix (kind quickfix, no diag).
        let batched = quickfixes.iter().filter(|ca| ca.diagnostics.is_none()).count();
        assert_eq!(batched, 1, "expected one 'fix all occurrences' batch");
    }

    #[test]
    fn code_action_only_source_fix_all_suppresses_quick_fixes() {
        let source =
            "Процедура Тест()\n    ЭтаФорма.Закрыть();\n    ЭтаФорма.Открыть();\nКонецПроцедуры\n";
        let (state, uri) = setup_code_action_doc(source);
        let ctx = latency_ctx(&state);

        let only = Some(vec![CodeActionKind::SOURCE_FIX_ALL]);
        let result = handle_code_action(ctx, code_action_params(uri, only)).unwrap().unwrap();

        assert!(
            actions_of_kind(&result, "quickfix").is_empty(),
            "quick fixes must be filtered out"
        );
        assert_eq!(
            actions_of_kind(&result, crate::lsp::to_proto::FIX_ALL_BSL).len(),
            1,
            "requesting source.fixAll must still match the bsl-analyzer subkind"
        );
    }

    #[test]
    fn code_action_only_quickfix_suppresses_source_fix_all() {
        let source =
            "Процедура Тест()\n    ЭтаФорма.Закрыть();\n    ЭтаФорма.Открыть();\nКонецПроцедуры\n";
        let (state, uri) = setup_code_action_doc(source);
        let ctx = latency_ctx(&state);

        let only = Some(vec![CodeActionKind::QUICKFIX]);
        let result = handle_code_action(ctx, code_action_params(uri, only)).unwrap().unwrap();

        assert!(
            actions_of_kind(&result, crate::lsp::to_proto::FIX_ALL_BSL).is_empty(),
            "source.fixAll must be filtered out when only quickfix is requested"
        );
        assert!(!actions_of_kind(&result, "quickfix").is_empty(), "quick fixes must remain");
    }

    #[test]
    fn reference_location_converter_uses_target_file_uri_and_text() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let source_uri = lsp_types::Url::parse("file:///source.bsl").unwrap();
        let target_uri = lsp_types::Url::parse("file:///target.bsl").unwrap();
        let source_text = "Процедура Источник()\nКонецПроцедуры";
        let target_text = "ПерваяСтрока\n    Цель();\n";

        let source_file_id = state.vfs_file_for_url(&source_uri).unwrap();
        let target_file_id = state.vfs_file_for_url(&target_uri).unwrap();
        // Both open: text lives in the resident overlay (these synthetic files
        // have no disk path for the disk-backed path to read).
        state.mem_docs.insert(source_uri.clone(), source_text.to_string(), 1);
        state.open_files.insert(source_file_id);
        state.open_files.insert(target_file_id);

        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(source_uri.to_file_path().unwrap()),
                Some(Arc::from(source_text)),
            );
            vfs.set_file_contents(
                VfsPath::new(target_uri.to_file_path().unwrap()),
                Some(Arc::from(target_text)),
            );
        }

        state.process_changes(false);
        let ctx = latency_ctx(&state);

        let start = target_text.find("Цель").unwrap() as u32;
        let end = start + "Цель".len() as u32;
        let ide_loc = IdeLocation {
            file_id: target_file_id,
            range: ide::TextRange::new(start.into(), end.into()),
        };

        let mut converter = ReferenceLocationConverter::new(&ctx, source_file_id, source_text);
        let lsp_loc = converter.convert(ide_loc).unwrap();

        assert_eq!(lsp_loc.uri, target_uri);
        assert_eq!(lsp_loc.range.start, Position { line: 1, character: 4 });
        assert_eq!(lsp_loc.range.end, Position { line: 1, character: 8 });
    }

    #[test]
    fn completion_returns_none_on_out_of_bounds_position() {
        let mut state = create_test_state();
        state.init_empty_source_root();

        let uri = lsp_types::Url::parse("file:///short.bsl").unwrap();
        let source = "short";

        state.mem_docs.insert(uri.clone(), source.to_string(), 1);
        let open_file_id = state.vfs_file_for_url(&uri).unwrap();
        state.open_files.insert(open_file_id);
        {
            let mut vfs = state.vfs.write();
            vfs.set_file_contents(
                VfsPath::new(uri.to_file_path().unwrap()),
                Some(Arc::from(source)),
            );
        }
        state.process_changes(false);

        let ctx = latency_ctx(&state);
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line: 999, character: 999 },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        let result = handle_completion(ctx, params);
        assert!(result.unwrap().is_none());
    }
}
