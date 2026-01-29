//! LSP server main loop.
//!
//! This module implements the core event loop for the LSP server,
//! following the rust-analyzer architecture.

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

    // Set workspace root from initialize params
    #[allow(deprecated)]
    if let Some(root_uri) = initialize_params.root_uri {
        if let Ok(root_path) = root_uri.to_file_path() {
            state.set_workspace_root(root_path);
        } else {
            tracing::warn!("Failed to convert root_uri to path: {}", root_uri);
        }
    } else if let Some(root_path) = initialize_params.root_path {
        state.set_workspace_root(root_path.into());
    } else {
        tracing::warn!("No workspace root provided by client");
    }

    // Run event loop
    run_event_loop(&mut state, &connection.receiver)?;

    tracing::info!("LSP server shutting down");
    Ok(())
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
                while let Ok(msg) = state.loader_receiver.try_recv() {
                    handle_loader_msg(state, msg)?;
                }
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
        if let Some(uri) = state.pending_diagnostics_uri.take() {
            crate::handlers::schedule_diagnostics(state, &uri);
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
        vfs::loader::Message::Progress { n_total, n_done, config_version, dir } => {
            use vfs::loader::LoadingProgress;
            match n_done {
                LoadingProgress::Finished => {
                    state.vfs_done = true;
                    tracing::info!("VFS loading complete");

                    state.report_progress("Loading", Progress::End, Some("Done".into()), Some(1.0));

                    state.init_source_root();
                }
                LoadingProgress::Started => {
                    tracing::info!("VFS loading started: {} entries", n_total);
                    state.report_progress(
                        "Loading",
                        Progress::Begin,
                        Some(format!("Scanning {} files...", n_total)),
                        Some(0.0),
                    );
                }
                LoadingProgress::Progress(done) => {
                    tracing::debug!(
                        "VFS loading progress: {}/{} (config v{})",
                        done,
                        n_total,
                        config_version
                    );
                    if let Some(ref dir) = dir {
                        tracing::debug!("  processing: {:?}", dir);
                    }
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
            handle_vfs_msg(state, files)?;
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
        }
    }
    Ok(())
}

/// Handle VFS messages from the loader thread.
///
/// This function processes file contents received from the background loader thread
/// and updates the VFS and Salsa database accordingly.
fn handle_vfs_msg(
    state: &mut GlobalState,
    files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>,
) -> Result<()> {
    use std::sync::Arc;

    let mut vfs = state.vfs.write();

    for (path, contents) in files {
        // Convert AbsPathBuf to std::path::Path for VfsPath
        let std_path: &std::path::Path = path.as_ref();
        let vfs_path = vfs::VfsPath::new(std_path);

        // Skip files that are managed by the LSP client (in MemDocs)
        // These files' content comes from didOpen/didChange, not disk
        if let Ok(url) = lsp_types::Url::from_file_path(std_path) {
            if state.mem_docs.contains(&url) {
                continue;
            }
        }

        // Convert Vec<u8> to Arc<str>, stripping UTF-8 BOM if present.
        // BOM (0xEF 0xBB 0xBF) is common in BSL files from 1C:Enterprise,
        // but LSP clients like VS Code strip it when sending file content.
        // Without this, VFS would see a "modify" change on every didOpen.
        let contents_str = contents.and_then(|bytes| {
            String::from_utf8(bytes).ok().map(|s| {
                let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
                Arc::from(s)
            })
        });

        vfs.set_file_contents(vfs_path, contents_str);
    }

    drop(vfs); // Release VFS lock before processing changes

    // Process changes and sync to Salsa database
    let (_, config_changed) = state.process_changes();

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
        CodeActionRequest, Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest,
        References, SemanticTokensFullRequest, SignatureHelpRequest,
    };

    tracing::info!("INCOMING REQUEST: method={} id={:?}", req.method, req.id);

    RequestDispatcher { req: Some(req), global_state: state }
        .on_sync_mut::<Shutdown>(|state, ()| {
            state.shutdown_requested = true;
            Ok(())
        })
        .on_sync::<GotoDefinition>(crate::handlers::handle_goto_definition)
        .on_sync::<References>(crate::handlers::handle_find_references)
        .on_sync::<HoverRequest>(crate::handlers::handle_hover)
        .on_sync::<Completion>(crate::handlers::handle_completion)
        .on_sync::<SemanticTokensFullRequest>(crate::handlers::handle_semantic_tokens_full)
        .on_sync::<DocumentSymbolRequest>(crate::handlers::handle_document_symbol)
        .on_sync::<CodeActionRequest>(crate::handlers::handle_code_action)
        .on_sync::<SignatureHelpRequest>(crate::handlers::handle_signature_help)
        .finish();

    Ok(())
}

/// Handles an LSP notification.
fn handle_notification(state: &mut GlobalState, not: Notification) -> Result<()> {
    use lsp_types::notification::{
        DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument,
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
