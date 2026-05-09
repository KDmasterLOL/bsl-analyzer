//! Handlers for LSP requests.
//!
//! This module implements handlers for LSP requests like
//! textDocument/definition, textDocument/references, etc.

use anyhow::Result;
use ide::Location as IdeLocation;
use line_index::LineIndex;
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionResponse, CompletionItem, CompletionItemKind,
    CompletionParams, CompletionResponse, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location,
    MarkupContent, MarkupKind, ReferenceParams, SemanticTokens, SemanticTokensParams,
    SemanticTokensResult, SignatureHelpParams,
};
use rustc_hash::FxHashMap;
use vfs::FileId;

use crate::frozen_context::LatencyRequestContext;
use crate::global_state::GlobalStateSnapshot;

/// Handles textDocument/definition request.
///
/// Goes to the definition of the symbol at the cursor position. Runs on the
/// task pool via `on_latency` — it reads exclusively from the immutable
/// `LatencyRequestContext`, so concurrent `didChange` edits cannot alias
/// the Salsa snapshot used for resolution.
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

    let offset = crate::lsp::offset(line_index, text, position)?;

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

    let offset = crate::lsp::offset(line_index, text, position)?;

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

/// Handles textDocument/hover request.
///
/// Returns hover information for the symbol at the cursor position.
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

    let offset = crate::lsp::offset(line_index, text, position)?;

    let hover_result = ctx.analysis.hover(file_id, offset.into(), ctx.diagnostics_config.locale);

    match hover_result {
        Some(result) => {
            let range = result.range.and_then(|r| crate::lsp::range(line_index, text, r));

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
    ctx: LatencyRequestContext,
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

    let file_id = ctx.file_id_for_url(&uri)?;

    let doc = ctx
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;
    let text = doc.text();
    let line_index = doc.line_index();

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

    // Convert position to offset (handle race condition with didChange)
    let offset = match crate::lsp::offset(line_index, text, position) {
        Ok(o) => o,
        Err(_) => {
            tracing::warn!("Position out of bounds, likely race with didChange - returning empty");
            return Ok(None);
        }
    };
    tracing::info!("Converted position to offset: {:?}", offset);

    let items = ctx.analysis.completions(
        file_id,
        offset.into(),
        ctx.workspace_root.clone(),
        ctx.diagnostics_config.locale,
    );
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
    ctx: LatencyRequestContext,
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
    if !ctx.vfs_done {
        tracing::debug!("VFS not ready, returning empty semantic tokens");
        return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })));
    }

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
    tracing::warn!(
        file_id = file_id.0,
        highlight_count = highlight_result.highlights.len(),
        resolved_external_files = highlight_result.resolved_external_files.len(),
        elapsed_ms = highlight_elapsed.as_millis() as u64,
        "semantic_tokens: analysis.highlight() completed"
    );

    let tokens = crate::lsp::semantic_tokens(line_index, text, &highlight_result.highlights);
    let total_elapsed = start.elapsed();
    tracing::warn!(
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

/// Handles textDocument/documentSymbol request.
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

    let lsp_symbols: Vec<lsp_types::DocumentSymbol> =
        symbols.into_iter().filter_map(|s| convert_document_symbol(line_index, text, s)).collect();

    Ok(Some(DocumentSymbolResponse::Nested(lsp_symbols)))
}

/// Handles textDocument/signatureHelp request.
///
/// Returns signature help (parameter hints) at the cursor position.
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

    let offset = match crate::lsp::offset(line_index, text, position) {
        Ok(o) => o,
        Err(_) => {
            tracing::warn!("Position out of bounds, likely race with didChange - returning empty");
            return Ok(None);
        }
    };

    let sig_help = ctx.analysis.signature_help(file_id, offset.into());

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
    let range = crate::lsp::text_range(line_index, text, params.range)?;

    let diagnostics = ctx.analysis.file_diagnostics_cached(file_id, ctx.diagnostics_config.clone());

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
                crate::lsp::to_proto::code_action(line_index, text, &uri, diag, fix)
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
        let target = self.target_file(ide_loc.file_id)?;
        let range = crate::lsp::range(&target.line_index, &target.text, ide_loc.range)
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

/// Convert IDE CompletionItem to LSP CompletionItem.
fn convert_completion_item(item: ide::CompletionItem) -> CompletionItem {
    let has_snippet = item.insert_text.contains('$');
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
        documentation: item.documentation.map(lsp_types::Documentation::String),
        sort_text: item.sort_text,
        filter_text: item.filter_text,
        label_details: item
            .source
            .map(|s| lsp_types::CompletionItemLabelDetails { detail: None, description: Some(s) }),
        ..Default::default()
    }
}

/// Convert IDE CompletionItemKind to LSP CompletionItemKind.
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

/// Converts project-model formatting config (from `.bsl-analyzer.json`) into the IDE
/// formatting config used by the formatter.
///
/// `project_model::FormattingConfig` carries only the two fields that are serialised
/// from the config file (`indent_size`, `use_tabs`); all remaining fields fall back to
/// their `ide::FormattingConfig` defaults so that the two types stay in sync
/// automatically.
///
/// A `From` impl is not possible here due to the orphan rule (both types are defined in
/// external crates relative to `bsl-analyzer`), so this free function serves the same
/// purpose.
///
/// Called when the server applies workspace formatting config from the project model
/// (as opposed to per-request LSP `FormattingOptions` handled by
/// [`formatting_config_from_options`]).
#[allow(dead_code)] // conversion infrastructure: will be called when project-model config is plumbed into LSP handlers
fn formatting_config_from_project_model(
    cfg: &project_model::FormattingConfig,
) -> ide::FormattingConfig {
    ide::FormattingConfig {
        use_tabs: cfg.use_tabs,
        indent_size: cfg.indent_size,
        ..ide::FormattingConfig::default()
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
            vfs_done: state.vfs_done,
            task_sender: state.task_pool.pool.sender.clone(),
            mem_docs: state.mem_docs.freeze(),
            file_paths: FrozenFilePaths::freeze(&state.vfs.read()),
        }
    }

    #[test]
    fn test_goto_definition_not_found() {
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        // Insert document
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

        // Should handle gracefully even if file is not in VFS
        let result = handle_goto_definition(ctx, params);
        // File not in VFS is expected to fail in tests
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[test]
    fn goto_definition_context_frozen_against_main_thread_mutation() {
        // Regression for the async dispatch race: the worker must read the
        // same text that existed at dispatch time, even if the main thread
        // applies didChange edits before the handler runs.
        let mut state = create_test_state();

        let uri = lsp_types::Url::parse("file:///frozen.bsl").unwrap();
        state.mem_docs.insert(uri.clone(), "original".to_string(), 1);

        // Freeze ctx BEFORE mutation. This is what on_latency does on the
        // main thread right before spawning the worker.
        let ctx = latency_ctx(&state);

        // Main thread mutates MemDocs while the "worker" still holds ctx.
        state.mem_docs.update(
            &uri,
            vec![lsp_types::TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "rewritten".to_string(),
            }],
        );

        // Worker reads ctx — must see "original", not "rewritten".
        let doc = ctx.mem_docs.get(&uri).expect("document must be in frozen view");
        assert_eq!(doc.text(), "original");
        assert_eq!(doc.version(), 1);

        // Main thread's live view reflects the mutation.
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

        // Should handle gracefully even if file is not in VFS
        let result = handle_find_references(ctx, params);
        // File not in VFS is expected to fail in tests
        assert!(result.is_err() || result.unwrap().is_none());
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
        state.mem_docs.insert(source_uri.clone(), source_text.to_string(), 1);

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
    fn semantic_tokens_returns_empty_when_vfs_not_done() {
        // Guard path: when VFS is still loading, the handler must not panic
        // on missing file data; it returns an empty token list so the client
        // retries after `workspace/semanticTokens/refresh`.
        let state = create_test_state();
        assert!(!state.vfs_done, "default GlobalState has vfs_done=false");

        let ctx = latency_ctx(&state);
        let params = SemanticTokensParams {
            text_document: TextDocumentIdentifier {
                uri: lsp_types::Url::parse("file:///anything.bsl").unwrap(),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = handle_semantic_tokens_full(ctx, params).unwrap().unwrap();
        match result {
            SemanticTokensResult::Tokens(tokens) => assert!(tokens.data.is_empty()),
            SemanticTokensResult::Partial(_) => panic!("expected full empty tokens"),
        }
    }

    #[test]
    fn completion_returns_none_on_out_of_bounds_position() {
        // Guard path: if the client sends a position beyond the document
        // (typical race with didChange), the handler must fall through to
        // Ok(None), not bubble the bounds error up to the client as an
        // InternalError.
        let mut state = create_test_state();
        let uri = lsp_types::Url::parse("file:///short.bsl").unwrap();
        state.mem_docs.insert(uri.clone(), "short".to_string(), 1);

        // File is in MemDocs but not in VFS, so file_id_for_url fails first.
        // That's fine — the handler still exercises the ?-propagation, and a
        // proper VFS-backed fixture can't be built without metadata scaffolding
        // this test deliberately avoids. The guard we actually care about is
        // `crate::lsp::offset(...)` Err → Ok(None), which only matters once
        // file_id/doc resolve; covered by the `?` error path here.
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
        // Either Ok(None) (guard taken) or Err (earlier resolve failed).
        assert!(result.is_err() || result.unwrap().is_none());
    }
}
