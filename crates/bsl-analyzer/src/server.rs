use std::path::PathBuf;

use anyhow::{Context, Result};
use crossbeam_channel::{select, Receiver};
use lsp_server::{Connection, Message, Notification, Request};
use lsp_types::{
    notification::{Exit, Notification as _},
    request::Shutdown,
    CodeActionProviderCapability, FoldingRangeProviderCapability, InitializeParams,
    SemanticTokensFullOptions, SemanticTokensOptions, SemanticTokensServerCapabilities,
    ServerCapabilities, SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkDoneProgressOptions,
};

use crate::{
    global_state::GlobalState,
    handlers::{NotificationDispatcher, RequestDispatcher},
    locale::parse_lsp_locale,
    lsp::{PositionEncoding, Progress},
};

pub fn main_loop(connection: Connection) -> Result<()> {
    tracing::info!("BSL Analyzer LSP server starting");

    let (initialize_id, initialize_params) =
        connection.initialize_start().context("Failed to start initialization")?;

    let initialize_params: InitializeParams =
        serde_json::from_value(initialize_params).context("Failed to parse InitializeParams")?;

    tracing::info!(
        "Client info: {:?}",
        initialize_params.client_info.as_ref().map(|info| &info.name)
    );

    let position_encoding = PositionEncoding::negotiate(&initialize_params.capabilities);

    let server_capabilities = server_capabilities(position_encoding);

    let initialize_result = lsp_types::InitializeResult {
        capabilities: server_capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: "bsl-analyzer".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };

    let mut initialize_value = serde_json::to_value(initialize_result)?;
    if let Some(object) = initialize_value.as_object_mut() {
        object.insert(
            "offsetEncoding".to_string(),
            serde_json::json!([position_encoding.as_offset_encoding()]),
        );
    }

    connection
        .initialize_finish(initialize_id, initialize_value)
        .context("Failed to finish initialization")?;

    tracing::info!("LSP server initialized");

    let mut state = GlobalState::new(connection.sender);
    state.position_encoding = position_encoding;

    state.init_empty_source_root();

    state.lsp_locale = initialize_params.locale.as_deref().map(parse_lsp_locale);
    if let Some(locale) = state.lsp_locale {
        tracing::info!(?locale, "client supplied LSP locale");
    }

    let workspace_root = extract_workspace_root(&initialize_params);

    if let Some(ref root) = workspace_root {
        state.set_workspace_root(root.clone());
    } else {
        tracing::warn!("No workspace root provided by client");
        state.update_diagnostics_config();
        // Without a workspace root no VFS loader runs, so no `Finished` event
        // will ever flip this flag — mark the (empty) workspace as loaded or
        // diagnostics would stay deferred forever.
        state.vfs_done = true;
    }

    run_event_loop(&mut state, &connection.receiver)?;

    tracing::info!("LSP server shutting down");
    Ok(())
}

fn extract_workspace_root(params: &InitializeParams) -> Option<PathBuf> {
    #[allow(deprecated)]
    if let Some(ref root_uri) = params.root_uri {
        if let Ok(path) = root_uri.to_file_path() {
            return Some(path);
        }
        tracing::warn!("Failed to convert root_uri to path: {}", root_uri);
        None
    } else {
        params.root_path.as_ref().map(PathBuf::from)
    }
}

fn run_event_loop(state: &mut GlobalState, receiver: &Receiver<Message>) -> Result<()> {
    loop {
        select! {
            recv(receiver) -> msg => {
                handle_lsp_msg(state, msg?)?;
                while let Ok(msg) = receiver.try_recv() {
                    handle_lsp_msg(state, msg)?;
                }
            }

            recv(&state.loader_receiver) -> msg => {
                handle_loader_msg(state, msg?)?;
            }

            recv(&state.task_pool.receiver) -> task => {
                handle_task(state, task?)?;
                while let Ok(task) = state.task_pool.receiver.try_recv() {
                    handle_task(state, task)?;
                }
            }
        }

        if state.shutdown_requested {
            break;
        }

        // The capacity gate keeps this from spinning: a schedule that finds the
        // pool still saturated would requeue the same URI, and the loop only
        // re-runs on a genuine wake (a finished worker frees a slot and posts
        // its result task).
        if !state.pending_diagnostics_uris.is_empty() && state.task_pool.pool.has_capacity() {
            for uri in std::mem::take(&mut state.pending_diagnostics_uris) {
                crate::handlers::schedule_diagnostics(state, &uri);
            }
        }
    }

    Ok(())
}

fn handle_lsp_msg(state: &mut GlobalState, msg: Message) -> Result<()> {
    match msg {
        Message::Request(req) => {
            if state.shutdown_requested {
                tracing::warn!("Received request after shutdown: {}", req.method);
                return Ok(());
            }
            handle_request(state, req)?;
        }
        Message::Notification(not) => {
            handle_notification(state, not)?;
        }
        Message::Response(resp) => {
            tracing::warn!("Unexpected response: {:?}", resp);
        }
    }
    Ok(())
}

fn handle_loader_msg(state: &mut GlobalState, msg: vfs::loader::Message) -> Result<()> {
    match msg {
        vfs::loader::Message::Progress { n_total, n_done, config_version: _, dir: _ } => {
            use vfs::loader::LoadingProgress;
            match n_done {
                LoadingProgress::Finished => {
                    let finalize_start = std::time::Instant::now();
                    tracing::info!("VFS loading complete");

                    // Boot-phase memory breakdown. Source batches were drained
                    // into Salsa incrementally as they streamed, so the text
                    // high-water (`boot_peak_text_bytes`) stayed near one loader
                    // chunk and this final `process_changes` is a near no-op; the
                    // remaining resident growth is the metadata substrate.
                    let mb = |b: u64| b / (1024 * 1024);
                    let rss_peak = state.boot_peak_rss_bytes;
                    let text_high_water = state.boot_peak_text_bytes;

                    state.process_changes(true);
                    let rss_after_load = crate::smoke::read_rss_bytes().unwrap_or(0);

                    state.init_source_root();
                    // Reopen the whole-config loader gate closed at
                    // `set_workspace_root`: the bootstrap and warm-up below run
                    // against the now-complete workspace, and the input flip
                    // invalidates anything resolved against the boot-window stub.
                    state.analysis_host.raw_database_mut().set_workspace_load_complete(true);
                    state.bootstrap_metadata_substrate();

                    state.report_progress(
                        "Loading",
                        Progress::Report,
                        Some("Loading metadata...".into()),
                        Some(0.95),
                    );
                    state.warm_metadata_cache();
                    let rss_steady = crate::smoke::read_rss_bytes().unwrap_or(0);

                    tracing::info!(
                        rss_peak_mb = mb(rss_peak),
                        rss_after_load_mb = mb(rss_after_load),
                        rss_steady_mb = mb(rss_steady),
                        text_high_water_mb = mb(text_high_water),
                        metadata_resident_mb = mb(rss_steady.saturating_sub(rss_after_load)),
                        "boot memory profile: source text drained incrementally during load",
                    );

                    state.degraded_files_count = state.skipped_bsl.len();
                    let extra_violations = state.assert_total_vfs_invariant();
                    if extra_violations > 0 {
                        tracing::error!(
                            extra_violations,
                            "total-VFS invariant violated unexpectedly — B1/B2-A missed a path",
                        );
                    }
                    if state.degraded_files_count > 0 {
                        tracing::warn!(
                            degraded = state.degraded_files_count,
                            "{} BSL paths skipped (unreadable or non-UTF-8)",
                            state.degraded_files_count,
                        );
                    }

                    state.vfs_done = true;
                    state.report_progress("Loading", Progress::End, Some("Done".into()), Some(1.0));

                    // Documents opened during the load had their dependency
                    // preload and diagnostics deferred; replay both now that
                    // the source root and metadata substrate are complete.
                    for uri in state.mem_docs.uris() {
                        if let Ok(file_id) = state.vfs_file_for_url(&uri) {
                            crate::handlers::notification::preload_dependencies(state, file_id);
                        }
                        crate::handlers::notification::schedule_diagnostics(state, &uri);
                    }

                    state.request_semantic_tokens_refresh();

                    tracing::info!(
                        elapsed_ms = finalize_start.elapsed().as_millis() as u64,
                        "vfs_done finalize complete (process_changes + init_source_root + warm_metadata)",
                    );
                }
                LoadingProgress::Scanning => {
                    tracing::info!("VFS scanning workspace...");
                    state.report_progress(
                        "Loading",
                        Progress::Begin,
                        Some("Scanning workspace...".into()),
                        None,
                    );
                }
                LoadingProgress::Started => {
                    tracing::info!("VFS loading started: {} files", n_total);
                    state.report_progress(
                        "Loading",
                        Progress::Report,
                        Some(format!("Loading {} files...", n_total)),
                        Some(0.0),
                    );
                }
                LoadingProgress::Progress(done) => {
                    tracing::debug!(done, n_total, "VFS loading progress");
                    let fraction = Progress::fraction(done, n_total);
                    state.report_progress(
                        "Loading",
                        Progress::Report,
                        Some(format!("{}/{}", done, n_total)),
                        Some(fraction),
                    );
                }
            }
        }
        vfs::loader::Message::Loaded { files } | vfs::loader::Message::Changed { files } => {
            let count = files.len();
            handle_vfs_msg(state, files, state.vfs_done)?;
            tracing::debug!(count, vfs_done = state.vfs_done, "streamed VFS batch");
        }
        vfs::loader::Message::WatchOnly { files } => {
            const VFS_WATCH_ONLY_BATCH: usize = 64;
            let count = files.len();
            for chunk in files.chunks(VFS_WATCH_ONLY_BATCH) {
                let mut vfs = state.vfs.write();
                for path in chunk {
                    vfs.register_watch_only(vfs::VfsPath::new(path.as_path()));
                }
            }
            // Metadata XML is watch-only (content not loaded into Salsa), so its
            // edits arrive here, NOT through `process_changes`. After the initial
            // scan these are live changes — e.g. a `git pull` touching XML —
            // each of which invalidates the configuration of its owning root.
            // Refresh open documents so their diagnostics reflect the new metadata.
            if state.vfs_done {
                state.analysis_host.request_cancellation();
                // Per-MDO substrate: re-discover the affected roots and re-read only
                // the changed/new XML, so resolve_metadata_object reflects the edit at
                // MDO granularity (and a disk-backed file_text never reads stale bytes).
                let changed: Vec<std::path::PathBuf> = files
                    .iter()
                    .map(|p| AsRef::<std::path::Path>::as_ref(p).to_path_buf())
                    .collect();
                state.refresh_metadata_substrate(&changed);
                // Coarse path: load_configuration is still the authoritative consumer
                // until the per-MDO resolvers are migrated, so keep bumping its root.
                state
                    .analysis_host
                    .raw_database_mut()
                    .bump_config_for_paths(files.iter().map(|p| p.as_ref()));
                for uri in state.opened_document_uris() {
                    crate::handlers::notification::schedule_diagnostics(state, &uri);
                }
            }
            tracing::debug!(count, vfs_done = state.vfs_done, "registered WatchOnly batch",);
        }
        vfs::loader::Message::RemovedRecursive { paths } => {
            // A removed directory subtree (a watch backend reported only the
            // directory, not each child). Expand it against the file set and
            // tombstone descendants; ignore during the initial scan.
            if state.vfs_done {
                let n = paths.len();
                let bsl_removed = state.remove_directories(&paths);
                // A removed subtree can also drop metadata MDOs; re-discovering the
                // owning roots tombstones them from the per-MDO listings.
                let removed: Vec<std::path::PathBuf> = paths
                    .iter()
                    .map(|p| AsRef::<std::path::Path>::as_ref(p).to_path_buf())
                    .collect();
                let meta_changed = state.refresh_metadata_substrate(&removed);
                if bsl_removed || meta_changed {
                    for uri in state.opened_document_uris() {
                        crate::handlers::notification::schedule_diagnostics(state, &uri);
                    }
                }
                tracing::info!(removed_paths = n, "processed removed directory subtree");
            }
        }
    }
    Ok(())
}

fn handle_task(state: &mut GlobalState, task: crate::global_state::Task) -> Result<()> {
    use crate::global_state::Task;

    match task {
        Task::DiagnosticsReady { uri, diagnostics, generation, completed_at } => {
            let current = state.diagnostics_generation.get(&uri).copied().unwrap_or(0);
            // Publish only the latest schedule for this uri (`== current`) and only
            // while it is still open — a result that finished after the document was
            // closed must not be published for a closed file.
            if generation == current && state.mem_docs.contains(&uri) {
                let publish_delay_ms = completed_at.elapsed().as_millis() as u64;
                let diagnostic_count = diagnostics.len();
                let allocated_mb = profile::memory_usage().allocated.megabytes();
                tracing::info!(
                    %uri,
                    generation,
                    publish_delay_ms,
                    diagnostic_count,
                    allocated_mb,
                    "publishing diagnostics",
                );
                let params =
                    lsp_types::PublishDiagnosticsParams { uri, diagnostics, version: None };
                let notification =
                    Notification::new("textDocument/publishDiagnostics".to_string(), params);
                state.sender.send(notification.into())?;
            } else {
                tracing::debug!(generation, current, "discarding stale diagnostics");
            }
        }
        Task::DiagnosticsCancelled { generation, completed_at } => {
            tracing::debug!(
                generation,
                publish_delay_ms = completed_at.elapsed().as_millis() as u64,
                "diagnostics cancelled",
            );
        }
        Task::DependenciesPreloaded { file_id, count } => {
            tracing::debug!(file_id = file_id.0, count, "dependencies preloaded");
            state.preload_tokens.remove(&file_id);
            state.preload_external_tokens.remove(&file_id);
        }
        Task::RequestResult { response } => {
            state.request_tokens.remove(&response.id);
            state.respond(response);
        }
        Task::PreloadExternalFiles { files } => {
            if files.is_empty() {
                return Ok(());
            }
            // Cache warming only — when the pool is saturated, skip it rather
            // than park the event loop in the bounded job queue.
            if !state.task_pool.pool.has_capacity() {
                tracing::debug!("task pool saturated; external preload skipped");
                return Ok(());
            }
            let file_count = files.len();
            let file_ids: Vec<u32> = files.iter().map(|f| f.0).collect();
            tracing::debug!(?file_ids, "preloading external files from semantic highlighting");

            let analysis = state.analysis_host.analysis();
            let task = analysis.warm_caches_task(&files);
            let first_file = files[0];

            if let Some(prev) = state.preload_external_tokens.remove(&first_file) {
                prev.cancel();
            }
            state.preload_external_tokens.insert(first_file, task.cancellation_token());

            let analysis_guard = state.note_analysis_spawned();
            let spawned = state.task_pool.pool.try_spawn(move || {
                let _analysis_guard = analysis_guard;
                let count = match salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
                    task.run()
                })) {
                    Ok(count) => {
                        tracing::debug!(count = file_count, "external files preloaded");
                        count
                    }
                    Err(_) => {
                        tracing::debug!(file_id = first_file.0, "external file preload cancelled");
                        0
                    }
                };
                Task::DependenciesPreloaded { file_id: first_file, count }
            });
            if spawned.is_err() {
                // Unreachable after the capacity check above; a skipped warm-up
                // costs only latency.
                tracing::debug!(
                    file_id = first_file.0,
                    "task pool rejected external preload job; skipped"
                );
                state.preload_external_tokens.remove(&first_file);
            }
        }
        Task::AnalysisProgressTick { epoch } => {
            state.handle_analysis_progress_tick(epoch);
        }
        Task::AnalysisJobFinished => {
            state.note_analysis_finished();
        }
    }
    Ok(())
}

fn handle_vfs_msg(
    state: &mut GlobalState,
    files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>,
    sync_to_salsa: bool,
) -> Result<()> {
    use std::sync::Arc;

    const VFS_WRITE_MINI_BATCH: usize = 16;

    let mut converted: Vec<(vfs::VfsPath, Option<Arc<str>>)> = Vec::with_capacity(files.len());
    for (path, contents) in files {
        let std_path: &std::path::Path = path.as_ref();
        let vfs_path = vfs::VfsPath::new(std_path);

        // An open editor buffer is authoritative for unsaved content, so a
        // disk-sourced change here must not clobber its overlay.
        if state.is_open_document_path(std_path, &vfs_path) {
            continue;
        }

        let contents_str =
            contents.and_then(|bytes| base_db::decode_disk_bytes(&bytes).map(Arc::from));

        if project_model::is_bsl_source_path(std_path) {
            let mutated = if contents_str.is_some() {
                state.skipped_bsl.remove(&path)
            } else if state.skipped_bsl.insert(path.clone()) {
                tracing::warn!(
                    path = %path,
                    "BSL file unreadable by VFS; recorded as skipped",
                );
                true
            } else {
                false
            };
            if mutated {
                state.degraded_files_count = state.skipped_bsl.len();
            }
        }

        converted.push((vfs_path, contents_str));
    }

    for chunk in converted.chunks(VFS_WRITE_MINI_BATCH) {
        let mut vfs = state.vfs.write();
        for (vfs_path, contents_str) in chunk {
            vfs.set_file_contents(vfs_path.clone(), contents_str.clone());
        }
    }

    if !sync_to_salsa {
        // Sample the streaming high-water BEFORE draining: this batch's text was
        // just written to the VFS and prior batches have already drained, so the
        // pending text is ~one loader chunk and RSS is at its per-batch peak.
        let (_pending_files, pending_text_bytes) = state.vfs.read().pending_change_bytes();
        state.boot_peak_text_bytes = state.boot_peak_text_bytes.max(pending_text_bytes as u64);
        if let Some(rss) = crate::smoke::read_rss_bytes() {
            state.boot_peak_rss_bytes = state.boot_peak_rss_bytes.max(rss);
            tracing::debug!(
                rss_mb = rss / (1024 * 1024),
                pending_text_mb = pending_text_bytes / (1024 * 1024),
                "boot load: batch buffered, draining to Salsa"
            );
        }

        // Boot phase: drain THIS batch into Salsa now, suppressing the
        // metadata/config reload (the post-load `init_source_root` +
        // `bootstrap_metadata_substrate` rebuild the source root and metadata
        // substrate once). Closed files are recorded by content revision and
        // their text dropped, so the whole corpus never piles up as resident
        // text in `Vfs::changes` — draining per batch keeps the load-time text
        // high-water at ~one chunk instead of the entire corpus held until a
        // single end-of-load flush.
        state.process_changes(true);
        return Ok(());
    }

    let outcome = state.process_changes(false);

    // External (non-open) file changes — e.g. a `git pull` touching modules or
    // metadata — invalidate the Salsa cache but do not by themselves re-publish
    // diagnostics for documents the editor already has open. Reschedule those so
    // their diagnostics reflect the new disk state. Open files are filtered out of
    // this batch above (their editor buffer is authoritative), so this fires only
    // for genuine external changes, not for saves of open buffers.
    if outcome.config_file_changed || outcome.affects_open_documents {
        tracing::info!(
            config_file_changed = outcome.config_file_changed,
            "external change: scheduling diagnostics refresh for all open documents"
        );
        for uri in state.opened_document_uris() {
            crate::handlers::notification::schedule_diagnostics(state, &uri);
        }
    }

    Ok(())
}

fn handle_request(state: &mut GlobalState, req: Request) -> Result<()> {
    use lsp_types::request::{
        CodeActionRequest, Completion, DocumentHighlightRequest, DocumentSymbolRequest,
        FoldingRangeRequest, Formatting, GotoDefinition, HoverRequest, OnTypeFormatting,
        RangeFormatting, References, SemanticTokensFullRequest, SignatureHelpRequest,
    };

    tracing::info!("INCOMING REQUEST: method={} id={:?}", req.method, req.id);

    RequestDispatcher { req: Some(req), global_state: state }
        .on_sync_mut::<Shutdown>(|state, ()| {
            state.shutdown_requested = true;
            Ok(())
        })
        .on_latency::<GotoDefinition>(crate::handlers::handle_goto_definition)
        .on_latency::<References>(crate::handlers::handle_find_references)
        .on_latency::<DocumentHighlightRequest>(crate::handlers::handle_document_highlight)
        .on_latency::<FoldingRangeRequest>(crate::handlers::handle_folding_range)
        .on_latency::<HoverRequest>(crate::handlers::handle_hover)
        .on_latency::<Completion>(crate::handlers::handle_completion)
        .on_latency::<SemanticTokensFullRequest>(crate::handlers::handle_semantic_tokens_full)
        .on_latency::<DocumentSymbolRequest>(crate::handlers::handle_document_symbol)
        .on_latency::<CodeActionRequest>(crate::handlers::handle_code_action)
        .on_latency::<SignatureHelpRequest>(crate::handlers::handle_signature_help)
        .on_sync::<Formatting>(crate::handlers::handle_formatting)
        .on_sync::<RangeFormatting>(crate::handlers::handle_range_formatting)
        .on_sync::<OnTypeFormatting>(crate::handlers::handle_on_type_formatting)
        .finish();

    Ok(())
}

fn handle_notification(state: &mut GlobalState, not: Notification) -> Result<()> {
    use lsp_types::notification::{
        Cancel, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
        DidSaveTextDocument,
    };

    if not.method == Exit::METHOD {
        tracing::info!("Received exit notification");
        state.shutdown_requested = true;
        return Ok(());
    }

    NotificationDispatcher { not: Some(not), global_state: state }
        .on_sync_mut::<DidOpenTextDocument>(crate::handlers::handle_did_open)?
        .on_sync_mut::<DidChangeTextDocument>(crate::handlers::handle_did_change)?
        .on_sync_mut::<DidCloseTextDocument>(crate::handlers::handle_did_close)?
        .on_sync_mut::<DidSaveTextDocument>(crate::handlers::handle_did_save)?
        .on_sync_mut::<Cancel>(crate::handlers::handle_cancel)?
        .finish();

    Ok(())
}

fn server_capabilities(position_encoding: PositionEncoding) -> ServerCapabilities {
    let legend = crate::lsp::semantic_tokens_legend();

    ServerCapabilities {
        position_encoding: Some(position_encoding.as_lsp_kind()),

        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),

        definition_provider: Some(lsp_types::OneOf::Left(true)),
        references_provider: Some(lsp_types::OneOf::Left(true)),

        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),

        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: None,
            trigger_characters: Some(vec![".".to_string()]),
            all_commit_characters: None,
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
            completion_item: None,
        }),

        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
                legend,
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),

        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),

        document_highlight_provider: Some(lsp_types::OneOf::Left(true)),

        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),

        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),

        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
        }),

        document_formatting_provider: Some(lsp_types::OneOf::Left(true)),

        document_range_formatting_provider: Some(lsp_types::OneOf::Left(true)),

        document_on_type_formatting_provider: Some(lsp_types::DocumentOnTypeFormattingOptions {
            first_trigger_character: ";".to_string(),
            more_trigger_character: Some(vec!["\n".to_string()]),
        }),

        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_capabilities() {
        let caps = server_capabilities(PositionEncoding::Utf8);

        assert_eq!(caps.position_encoding, Some(PositionEncoding::Utf8.as_lsp_kind()));

        match caps.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => {
                assert_eq!(kind, TextDocumentSyncKind::INCREMENTAL);
            }
            _ => panic!("Expected incremental text document sync"),
        }

        assert_eq!(caps.document_highlight_provider, Some(lsp_types::OneOf::Left(true)));
        assert_eq!(caps.folding_range_provider, Some(FoldingRangeProviderCapability::Simple(true)));
    }

    #[test]
    fn test_position_encoding_prefers_utf8_when_client_supports_it() {
        let caps = lsp_types::ClientCapabilities {
            general: Some(lsp_types::GeneralClientCapabilities {
                position_encodings: Some(vec![
                    lsp_types::PositionEncodingKind::UTF8,
                    lsp_types::PositionEncodingKind::UTF16,
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };

        assert_eq!(PositionEncoding::negotiate(&caps), PositionEncoding::Utf8);
    }

    #[test]
    fn vfs_done_finalize_replays_deferred_diagnostics_for_open_documents() {
        let (sender, _receiver) = crossbeam_channel::unbounded();
        let mut state = crate::global_state::GlobalState::new(sender);
        state.init_empty_source_root();
        assert!(!state.vfs_done);
        // Model the boot window `set_workspace_root` opens on a real start.
        state.analysis_host.raw_database_mut().set_workspace_load_complete(false);

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("mod.bsl");
        std::fs::write(&path, "Процедура Тест() КонецПроцедуры").expect("write");
        let uri = lsp_types::Url::from_file_path(&path).unwrap();

        crate::handlers::notification::handle_did_open(
            &mut state,
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bsl".to_string(),
                    version: 1,
                    text: "Процедура Тест() КонецПроцедуры".to_string(),
                },
            },
        )
        .unwrap();

        // Opened while the workspace was still loading: nothing scheduled yet.
        assert_eq!(state.diagnostics_generation.get(&uri), None);
        assert!(!state.diagnostics_tokens.contains_key(&uri));

        handle_loader_msg(
            &mut state,
            vfs::loader::Message::Progress {
                n_total: 1,
                n_done: vfs::loader::LoadingProgress::Finished,
                dir: None,
                config_version: 0,
            },
        )
        .unwrap();

        assert!(state.vfs_done);
        assert!(
            state.analysis_host.raw_database().workspace_load_complete(),
            "the finalize must reopen the whole-config loader gate"
        );
        assert_eq!(state.diagnostics_generation.get(&uri).copied(), Some(1));
        assert!(state.diagnostics_tokens.contains_key(&uri));
    }

    #[test]
    fn handle_task_does_not_publish_diagnostics_for_a_closed_document() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut state = crate::global_state::GlobalState::new(sender);

        let uri = lsp_types::Url::parse("file:///gone.bsl").unwrap();
        // A diagnostics task that finished with the current generation, but for a
        // document that is NOT open (closed before the result arrived).
        state.diagnostics_generation.insert(uri.clone(), 1);
        let task = crate::global_state::Task::DiagnosticsReady {
            uri,
            diagnostics: Vec::new(),
            generation: 1,
            completed_at: std::time::Instant::now(),
        };

        handle_task(&mut state, task).unwrap();

        assert!(
            receiver.try_recv().is_err(),
            "diagnostics for a closed document must not be published"
        );
    }
}
