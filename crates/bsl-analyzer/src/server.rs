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
    InitializeParams, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensServerCapabilities, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncKind, WorkDoneProgressOptions,
};

use crate::{
    global_state::GlobalState,
    handlers::{NotificationDispatcher, RequestDispatcher},
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

    // Run event loop
    run_event_loop(&mut state, &connection.receiver)?;

    tracing::info!("LSP server shutting down");
    Ok(())
}

/// Runs the main event loop.
///
/// Handles incoming LSP messages until shutdown is requested.
fn run_event_loop(state: &mut GlobalState, receiver: &Receiver<Message>) -> Result<()> {
    loop {
        select! {
            recv(receiver) -> msg => {
                match msg? {
                    Message::Request(req) => {
                        if state.shutdown_requested {
                            tracing::warn!("Received request after shutdown: {}", req.method);
                            continue;
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

                if state.shutdown_requested {
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Handles an LSP request.
fn handle_request(state: &mut GlobalState, req: Request) -> Result<()> {
    use lsp_types::request::{GotoDefinition, References, SemanticTokensFullRequest};

    RequestDispatcher { req: Some(req), global_state: state }
        .on_sync_mut::<Shutdown>(|state, ()| {
            state.shutdown_requested = true;
            Ok(())
        })
        .on_sync::<GotoDefinition>(crate::handlers::handle_goto_definition)
        .on_sync::<References>(crate::handlers::handle_find_references)
        .on_sync::<SemanticTokensFullRequest>(crate::handlers::handle_semantic_tokens_full)
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

        // Semantic tokens (syntax highlighting)
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions { work_done_progress: None },
                legend,
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),

        // Future capabilities will be added here:
        // - hover_provider
        // - completion_provider
        // - diagnostic_provider
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
