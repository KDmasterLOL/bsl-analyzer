use anyhow::Result;
use ide::{
    DocumentHighlightKind as IdeDocumentHighlightKind, FoldingRangeKind as IdeFoldingRangeKind,
    Location as IdeLocation,
};
use line_index::{LineIndex, TextSize};
use lsp_types::{
    CodeActionOrCommand, CodeActionParams, CodeActionResponse, CompletionItem, CompletionItemKind,
    CompletionParams, CompletionResponse, DocumentHighlight as LspDocumentHighlight,
    DocumentHighlightKind as LspDocumentHighlightKind, DocumentHighlightParams,
    DocumentSymbolParams, DocumentSymbolResponse, FoldingRange as LspFoldingRange,
    FoldingRangeKind as LspFoldingRangeKind, FoldingRangeParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location, MarkupContent, MarkupKind,
    ReferenceParams, SemanticTokens, SemanticTokensParams, SemanticTokensResult,
    SignatureHelpParams,
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

    let mut actions = Vec::new();
    for diag in diagnostics.iter() {
        if diag.fixes.is_empty() {
            continue;
        }
        if diag.range.intersect(range).is_none() {
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

    if actions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(actions))
    }
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
pub fn handle_workspace_diagnostic(
    ctx: LatencyRequestContext,
    params: lsp_types::WorkspaceDiagnosticParams,
) -> Result<lsp_types::WorkspaceDiagnosticReportResult> {
    use lsp_types::{
        FullDocumentDiagnosticReport, UnchangedDocumentDiagnosticReport, WorkspaceDiagnosticReport,
        WorkspaceDiagnosticReportResult, WorkspaceDocumentDiagnosticReport,
        WorkspaceFullDocumentDiagnosticReport, WorkspaceUnchangedDocumentDiagnosticReport,
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

    tracing::info!(scope = ?scope, files = file_ids.len(), "workspace diagnostics sweep");

    let computed = ctx.analysis.workspace_diagnostics(&file_ids, ctx.diagnostics_config.clone());

    let mut items = Vec::with_capacity(computed.len());
    for (file_id, diagnostics) in computed {
        let url = match ctx.url_for_file_id(file_id) {
            Ok(url) => url,
            Err(_) => continue,
        };
        let result_id = crate::lsp::diagnostics_result_id(&diagnostics);
        // The LSP spec wants the buffer version for an open document (and `None` only for a
        // closed one), so a client can associate or discard the report against its buffer.
        let open_doc = ctx.mem_docs.get(&url);
        let version = open_doc.map(|doc| doc.version() as i64);

        if previous.get(&url).map(String::as_str) == Some(result_id.as_str()) {
            items.push(WorkspaceDocumentDiagnosticReport::Unchanged(
                WorkspaceUnchangedDocumentDiagnosticReport {
                    uri: url,
                    version,
                    unchanged_document_diagnostic_report: UnchangedDocumentDiagnosticReport {
                        result_id,
                    },
                },
            ));
            continue;
        }

        // An open buffer's overlay text (and cached line index) reflects unsaved edits; a closed
        // file has neither, so fall back to the database's disk-backed text.
        let lsp_items = match open_doc {
            Some(doc) => crate::lsp::to_proto::diagnostics_with_encoding(
                doc.line_index(),
                doc.text(),
                &diagnostics,
                ctx.position_encoding,
            ),
            None => {
                let text = ctx.analysis.file_text(file_id);
                let line_index = LineIndex::new(&text);
                crate::lsp::to_proto::diagnostics_with_encoding(
                    &line_index,
                    &text,
                    &diagnostics,
                    ctx.position_encoding,
                )
            }
        };

        items.push(WorkspaceDocumentDiagnosticReport::Full(
            WorkspaceFullDocumentDiagnosticReport {
                uri: url,
                version,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: Some(result_id),
                    items: lsp_items,
                },
            },
        ));
    }

    Ok(WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items }))
}

/// Whether a file belongs to the `workspace/diagnostic` sweep for the given scope.
/// Only BSL source files are ever in scope; `Extensions` additionally requires the file
/// to live under one of the configuration-extension roots.
fn path_in_workspace_scope(
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
            task_sender: state.task_pool.pool.sender.clone(),
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
