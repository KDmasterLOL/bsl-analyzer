//! LSP server main loop.
//!
//! This module implements the core event loop for the LSP server.

use std::path::PathBuf;

use anyhow::{Context, Result};
use crossbeam_channel::{select, Receiver};
use lsp_server::{Connection, Message, Notification, Request};
use lsp_types::{
    notification::{Exit, Notification as _},
    request::Shutdown,
    CodeActionProviderCapability, InitializeParams, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkDoneProgressOptions,
};

use crate::{
    global_state::GlobalState,
    handlers::{NotificationDispatcher, RequestDispatcher},
    locale::parse_lsp_locale,
    lsp::Progress,
};

/// Runs the main LSP server loop.
///
/// This function:
/// 1. Performs the LSP initialize handshake
/// 2. Creates the GlobalState
/// 3. Runs the event loop
/// 4. Handles shutdown
pub fn main_loop(connection: Connection) -> Result<()> {
    tracing::info!("BSL Analyzer LSP server starting");

    // Perform initialize handshake
    let (initialize_id, initialize_params) =
        connection.initialize_start().context("Failed to start initialization")?;

    let initialize_params: InitializeParams =
        serde_json::from_value(initialize_params).context("Failed to parse InitializeParams")?;

    tracing::info!(
        "Client info: {:?}",
        initialize_params.client_info.as_ref().map(|info| &info.name)
    );

    // Build server capabilities
    let server_capabilities = server_capabilities();

    let initialize_result = lsp_types::InitializeResult {
        capabilities: server_capabilities,
        server_info: Some(lsp_types::ServerInfo {
            name: "bsl-analyzer".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };

    connection
        .initialize_finish(initialize_id, serde_json::to_value(initialize_result)?)
        .context("Failed to finish initialization")?;

    tracing::info!("LSP server initialized");

    // Create global state
    let mut state = GlobalState::new(connection.sender);

    // Initialize empty SourceRoot(0) to prevent race condition where files
    // are opened via LSP before VFS loader finishes
    state.init_empty_source_root();

    // Capture the IDE-supplied locale (RFC 4646) so that diagnostics_state
    // can use it as a fallback when `[output] display_language` is unset.
    // Must run BEFORE `set_workspace_root`, which triggers
    // `update_diagnostics_config` and freezes the locale into the Salsa
    // input.
    state.lsp_locale = initialize_params.locale.as_deref().map(parse_lsp_locale);
    if let Some(locale) = state.lsp_locale {
        tracing::info!(?locale, "client supplied LSP locale");
    }

    // Extract workspace root from initialize params
    let workspace_root = extract_workspace_root(&initialize_params);

    // Set workspace root in LSP state. `set_workspace_root` is what
    // triggers `update_diagnostics_config` and folds the LSP-supplied
    // locale into the Salsa input — without it, `diagnostics_config`
    // stays at its default (`Locale::Ru`) and the client's `"en-US"`
    // is silently ignored. The no-workspace-root branch must therefore
    // still run `update_diagnostics_config` so hover and completion
    // (which read `diagnostics_config.locale`) see the LSP fallback.
    if let Some(ref root) = workspace_root {
        state.set_workspace_root(root.clone());
    } else {
        tracing::warn!("No workspace root provided by client");
        state.update_diagnostics_config();
    }

    // Run event loop
    run_event_loop(&mut state, &connection.receiver)?;

    tracing::info!("LSP server shutting down");
    Ok(())
}

/// Extracts the workspace root path from LSP initialize params.
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

/// Runs the main event loop.
///
/// Handles incoming LSP messages, VFS loader messages, and background task results.
/// Uses event coalescing (drain loops) to batch rapid changes before scheduling diagnostics.
fn run_event_loop(state: &mut GlobalState, receiver: &Receiver<Message>) -> Result<()> {
    loop {
        select! {
            recv(receiver) -> msg => {
                handle_lsp_msg(state, msg?)?;
                // Drain pending LSP messages to coalesce rapid changes (e.g., 50dd)
                while let Ok(msg) = receiver.try_recv() {
                    handle_lsp_msg(state, msg)?;
                }
            }

            recv(&state.loader_receiver) -> msg => {
                handle_loader_msg(state, msg?)?;
                // Don't drain loader messages - process one at a time
                // to allow progress updates to be displayed
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

        // Schedule pending diagnostics after all events drained.
        // This ensures rapid changes (e.g., 50dd) are coalesced into a single diagnostic run.
        if state.vfs_done {
            if let Some(uri) = state.pending_diagnostics_uri.take() {
                crate::handlers::schedule_diagnostics(state, &uri);
            }
        }
    }

    Ok(())
}

/// Handles a single LSP message (request, notification, or response).
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

/// Handles a VFS loader message.
fn handle_loader_msg(state: &mut GlobalState, msg: vfs::loader::Message) -> Result<()> {
    match msg {
        vfs::loader::Message::Progress { n_total, n_done, config_version: _, dir: _ } => {
            use vfs::loader::LoadingProgress;
            match n_done {
                LoadingProgress::Finished => {
                    let finalize_start = std::time::Instant::now();
                    state.vfs_done = true;
                    tracing::info!("VFS loading complete");

                    // Loader batches were already streamed into VFS as they arrived
                    // (see `Message::Loaded`/`Changed` arm below), so no buffered
                    // payload needs draining here. We only need the single Salsa
                    // flush over the accumulated `Vfs::changes`.
                    //
                    // During the cold-start sync every workspace file shows up as a
                    // VFS change, including all `.xml` metadata files. Suppress the
                    // metadata bump for this sweep so it does not invalidate the
                    // configuration cache immediately before `warm_metadata_cache`
                    // populates it.
                    state.process_changes(true);

                    state.init_source_root();

                    // Eagerly load metadata to warm Salsa cache
                    state.report_progress(
                        "Loading",
                        Progress::Report,
                        Some("Loading metadata...".into()),
                        Some(0.95),
                    );
                    state.warm_metadata_cache();

                    state.report_progress("Loading", Progress::End, Some("Done".into()), Some(1.0));

                    // Schedule diagnostics for files that were opened before VFS finished
                    for uri in state.mem_docs.uris() {
                        crate::handlers::notification::schedule_diagnostics(state, &uri);
                    }

                    // Request client to refresh semantic tokens for all open files
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
            // Stream straight into VFS in both phases. handle_vfs_msg converts
            // each file's `Vec<u8>` to `Arc<str>` and drops the bytes per entry,
            // so we never accumulate raw payloads in the application layer.
            //
            // During cold-start (`vfs_done == false`) the loader path here does
            // not sync Salsa — changes pile up in `Vfs::changes`. They are
            // drained either by the bulk `process_changes` at
            // `LoadingProgress::Finished` below, or earlier by a `didOpen` /
            // `didChange` handler that fires during cold-start (those drains
            // pass `suppress_metadata_bump = true`, see `notification.rs`).
            // After cold-start this call performs the immediate Salsa sync.
            let count = files.len();
            handle_vfs_msg(state, files, state.vfs_done)?;
            tracing::debug!(count, vfs_done = state.vfs_done, "streamed VFS batch");
        }
        vfs::loader::Message::WatchOnly { files } => {
            // Mirror the mini-batching strategy used for content batches
            // (`handle_vfs_msg`): allocate FileIds for each watch-only path
            // under short-held write locks so latency-sensitive read
            // snapshots from hover/completion/goto are not starved during
            // the cold-start sweep on ERP-scale workspaces.
            //
            // Cold-start (`vfs_done == false`): registrations alone, no
            // metadata bump — `LoadingProgress::Finished` later calls
            // `warm_metadata_cache` which reads XML straight from disk.
            // Runtime (`vfs_done == true`): bump `metadata_version` so the
            // bsl-metadata Salsa cache invalidates and the next query
            // re-reads from disk. `register_watch_only` is idempotent so
            // duplicate registrations across overlapping watch events
            // remain cheap.
            const VFS_WATCH_ONLY_BATCH: usize = 64;
            let count = files.len();
            for chunk in files.chunks(VFS_WATCH_ONLY_BATCH) {
                let mut vfs = state.vfs.write();
                for path in chunk {
                    vfs.register_watch_only(vfs::VfsPath::new(path.as_path()));
                }
            }
            if state.vfs_done {
                // Wake outstanding Salsa snapshots before any setter, mirroring
                // the ABBA-prevention pattern in `process_changes`
                // (`base_db::Files`). `bump_metadata_version` would otherwise
                // race a live background snapshot reading the stale metadata
                // cache between this `vfs.write()` release and the bump.
                state.analysis_host.request_cancellation();
                state.analysis_host.raw_database_mut().bump_metadata_version();
            }
            tracing::debug!(count, vfs_done = state.vfs_done, "registered WatchOnly batch",);
        }
    }
    Ok(())
}

/// Handles a background task result.
fn handle_task(state: &mut GlobalState, task: crate::global_state::Task) -> Result<()> {
    use crate::global_state::Task;

    match task {
        Task::DiagnosticsReady { uri, diagnostics, generation } => {
            if generation >= state.diagnostics_generation {
                let params =
                    lsp_types::PublishDiagnosticsParams { uri, diagnostics, version: None };
                let notification =
                    Notification::new("textDocument/publishDiagnostics".to_string(), params);
                state.sender.send(notification.into())?;
            } else {
                tracing::debug!(
                    generation,
                    current = state.diagnostics_generation,
                    "discarding stale diagnostics"
                );
            }
        }
        Task::DiagnosticsCancelled { generation } => {
            tracing::debug!(generation, "diagnostics cancelled");
        }
        Task::DependenciesPreloaded { file_id, count } => {
            tracing::debug!(file_id = file_id.0, count, "dependencies preloaded");
            // Best-effort cleanup. If a rapid re-spawn replaced our token
            // between worker completion and this handler, the newer entry
            // is evicted and its worker becomes uncancellable — worst case
            // is wasted CPU on a redundant cache warmer.
            state.preload_tokens.remove(&file_id);
            state.preload_external_tokens.remove(&file_id);
        }
        Task::RequestResult { response } => {
            // Best-effort cleanup: a concurrent `$/cancelRequest` may have
            // already removed the entry. Missing is fine — the worker
            // beat the cancel to the finish line.
            state.request_tokens.remove(&response.id);
            state.respond(response);
        }
        Task::PreloadExternalFiles { files } => {
            if files.is_empty() {
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

            state.task_pool.pool.spawn(move || {
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
        }
    }
    Ok(())
}

/// Handle VFS messages from the loader thread.
///
/// This function processes file contents received from the background loader thread
/// and updates the VFS. Salsa database sync is deferred until VFS loading completes
/// to allow progress messages to be processed immediately.
///
/// # Arguments
/// * `sync_to_salsa` - If true, immediately sync VFS changes to Salsa database.
///   During initial loading this should be false to avoid blocking on Salsa.
fn handle_vfs_msg(
    state: &mut GlobalState,
    files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>,
    sync_to_salsa: bool,
) -> Result<()> {
    use std::sync::Arc;

    /// Number of files written under a single `vfs.write()` lock acquisition.
    /// Releasing the lock between mini-batches lets latency-sensitive task-pool
    /// workers (hover/completion/goto) grab a read snapshot during cold-start
    /// instead of blocking for the full chunk.
    const VFS_WRITE_MINI_BATCH: usize = 16;

    // Phase 1: convert raw bytes to `Arc<str>` and filter `mem_docs` paths
    // outside any VFS lock. UTF-8 validation + Arc allocation for a 64 MiB
    // chunk is non-trivial, and holding `vfs.write()` across it would block
    // every read snapshot taken by background latency tasks.
    let converted: Vec<(vfs::VfsPath, Option<Arc<str>>)> = files
        .into_iter()
        .filter_map(|(path, contents)| {
            let std_path: &std::path::Path = path.as_ref();

            // Skip files managed by the LSP client (in MemDocs) — their
            // content comes from didOpen/didChange, not disk.
            if let Ok(url) = lsp_types::Url::from_file_path(std_path) {
                if state.mem_docs.contains(&url) {
                    return None;
                }
            }

            let vfs_path = vfs::VfsPath::new(std_path);

            // Convert Vec<u8> to Arc<str>, stripping UTF-8 BOM if present.
            // BOM (0xEF 0xBB 0xBF) is common in BSL files from 1C:Enterprise,
            // but LSP clients like VS Code strip it when sending file
            // content. Without this, VFS would see a "modify" change on
            // every didOpen.
            let contents_str = contents.and_then(|bytes| {
                String::from_utf8(bytes).ok().map(|s| {
                    let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
                    Arc::from(s)
                })
            });

            Some((vfs_path, contents_str))
        })
        .collect();

    // Phase 2: drain into VFS in mini-batches under short-held write locks.
    for chunk in converted.chunks(VFS_WRITE_MINI_BATCH) {
        let mut vfs = state.vfs.write();
        for (vfs_path, contents_str) in chunk {
            vfs.set_file_contents(vfs_path.clone(), contents_str.clone());
        }
    }

    // Loader-driven cold-start batches pass `sync_to_salsa = false`: the bytes
    // are now in `Vfs::changes` but Salsa is not synced here. The eventual
    // drain happens at `LoadingProgress::Finished`, or sooner if a `didOpen` /
    // `didChange` arrives during cold-start (those handlers call
    // `process_changes` directly).
    if !sync_to_salsa {
        return Ok(());
    }

    // Process changes and sync to Salsa database
    let (_, config_changed) = state.process_changes(false);

    // If config changed, schedule diagnostics for all open files
    if config_changed {
        tracing::info!("config changed, scheduling diagnostics refresh for all open documents");
        for uri in state.opened_document_uris() {
            crate::handlers::notification::schedule_diagnostics(state, &uri);
        }
    }

    Ok(())
}

/// Handles an LSP request.
fn handle_request(state: &mut GlobalState, req: Request) -> Result<()> {
    use lsp_types::request::{
        CodeActionRequest, Completion, DocumentSymbolRequest, Formatting, GotoDefinition,
        HoverRequest, OnTypeFormatting, RangeFormatting, References, SemanticTokensFullRequest,
        SignatureHelpRequest,
    };

    tracing::info!("INCOMING REQUEST: method={} id={:?}", req.method, req.id);

    RequestDispatcher { req: Some(req), global_state: state }
        .on_sync_mut::<Shutdown>(|state, ()| {
            state.shutdown_requested = true;
            Ok(())
        })
        // Read-only latency-sensitive requests run on the task pool so the
        // main loop stays responsive to $/cancelRequest and subsequent edits.
        .on_latency::<GotoDefinition>(crate::handlers::handle_goto_definition)
        .on_latency::<References>(crate::handlers::handle_find_references)
        .on_latency::<HoverRequest>(crate::handlers::handle_hover)
        .on_latency::<Completion>(crate::handlers::handle_completion)
        .on_latency::<SemanticTokensFullRequest>(crate::handlers::handle_semantic_tokens_full)
        .on_latency::<DocumentSymbolRequest>(crate::handlers::handle_document_symbol)
        .on_latency::<CodeActionRequest>(crate::handlers::handle_code_action)
        .on_latency::<SignatureHelpRequest>(crate::handlers::handle_signature_help)
        // Formatting stays synchronous: clients (VS Code, Helix, Neovim)
        // run save-and-format synchronously, and our formatter is fast
        // enough that task-pool dispatch would add latency, not hide it.
        .on_sync::<Formatting>(crate::handlers::handle_formatting)
        .on_sync::<RangeFormatting>(crate::handlers::handle_range_formatting)
        .on_sync::<OnTypeFormatting>(crate::handlers::handle_on_type_formatting)
        .finish();

    Ok(())
}

/// Handles an LSP notification.
fn handle_notification(state: &mut GlobalState, not: Notification) -> Result<()> {
    use lsp_types::notification::{
        Cancel, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
        DidSaveTextDocument,
    };

    // Check for exit notification (special case - ends the loop)
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

/// Returns the server capabilities.
///
/// This tells the client what features the server supports.
fn server_capabilities() -> ServerCapabilities {
    let legend = crate::lsp::semantic_tokens_legend();

    ServerCapabilities {
        // Text document synchronization
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),

        // Navigation
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        references_provider: Some(lsp_types::OneOf::Left(true)),

        // Hover information
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),

        // Code completion
        completion_provider: Some(lsp_types::CompletionOptions {
            resolve_provider: None,
            trigger_characters: Some(vec![".".to_string()]),
            all_commit_characters: None,
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
            completion_item: None,
        }),

        // Semantic tokens (syntax highlighting)
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
                legend,
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),

        // Document symbols (outline, breadcrumbs)
        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),

        // Code actions (quick fixes)
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),

        // Signature help (parameter hints)
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
        }),

        // Document formatting
        document_formatting_provider: Some(lsp_types::OneOf::Left(true)),

        // Range formatting
        document_range_formatting_provider: Some(lsp_types::OneOf::Left(true)),

        // On-type formatting
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
        let caps = server_capabilities();

        // Text sync should be incremental
        match caps.text_document_sync {
            Some(TextDocumentSyncCapability::Kind(kind)) => {
                assert_eq!(kind, TextDocumentSyncKind::INCREMENTAL);
            }
            _ => panic!("Expected incremental text document sync"),
        }
    }
}
