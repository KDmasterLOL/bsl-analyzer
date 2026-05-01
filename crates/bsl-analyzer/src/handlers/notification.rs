//! Handlers for LSP notifications.
//!
//! This module implements handlers for LSP notifications like
//! textDocument/didOpen, didChange, didClose, and didSave.

use std::{sync::Arc, time::Instant};

use anyhow::Result;
use base_db::FileIdInput;
use ide::file_diagnostics_query;
use line_index::LineIndex;
use lsp_server::Notification;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, PublishDiagnosticsParams, Url,
};
use salsa::Database; // brings cancellation_token() into scope

use crate::global_state::{GlobalState, Task};

/// Returns `true` when a textDocument notification should reach the BSL
/// pipeline. Editors may attach this server to non-BSL buffers (XML
/// metadata, OneScript, plain text) — the pipeline can't analyze them and
/// the URL guards return `Err`, which would surface as a notification
/// failure. Silently skipping at the notification boundary is the
/// LSP-correct response for `didOpen` / `didChange` etc.
fn is_handled_uri(uri: &Url) -> bool {
    uri.to_file_path().map(|p| project_model::is_bsl_source_path(&p)).unwrap_or(false)
}

/// Schedules diagnostics computation in a background thread.
///
/// Before spawning, any previous in-flight worker for this URI is cancelled via its
/// Salsa `CancellationToken`, letting it unwind cooperatively at the next query
/// boundary (no write to the global revision required). A concurrent `set_file_text()`
/// still triggers `Cancelled::PendingWrite`; both flavors are caught by `Cancelled::catch`,
/// while bug panics propagate out and are logged by the task pool.
pub fn schedule_diagnostics(state: &mut GlobalState, uri: &Url) {
    // Don't schedule diagnostics until VFS is ready.
    // They will be scheduled again after VFS loading completes.
    if !state.vfs_done {
        tracing::debug!("VFS not ready, skipping diagnostics scheduling");
        return;
    }

    if let Some(prev) = state.diagnostics_tokens.remove(uri) {
        prev.cancel();
    }

    state.diagnostics_generation += 1;
    let generation = state.diagnostics_generation;

    let file_id = match crate::lsp::file_id(state, uri) {
        Ok(id) => id,
        Err(_) => return,
    };
    let text = match state.mem_docs.get(uri) {
        Some(t) => t,
        None => return,
    };

    let db = state.analysis_host.raw_database().clone();
    state.diagnostics_tokens.insert(uri.clone(), db.cancellation_token());
    let config = state.diagnostics_config().clone();
    let uri = uri.clone();
    let queued_at = Instant::now();

    state.task_pool.pool.spawn(move || {
        let started_at = Instant::now();
        let queue_wait_ms = started_at.duration_since(queued_at).as_millis() as u64;
        let result = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            let file_id_input = FileIdInput::new(&db, file_id);
            let config_id = base_db::DiagnosticsConfigId::new(&db, config);
            let ide_diagnostics = file_diagnostics_query(&db, file_id_input, config_id);
            let line_index = LineIndex::new(&text);
            crate::lsp::diagnostics(&line_index, &text, &ide_diagnostics)
        }));
        let compute_ms = started_at.elapsed().as_millis() as u64;

        match result {
            Ok(diagnostics) => {
                tracing::info!(
                    %uri,
                    queue_wait_ms,
                    compute_ms,
                    diagnostic_count = diagnostics.len(),
                    "diagnostics ready",
                );
                Task::DiagnosticsReady { uri, diagnostics, generation }
            }
            Err(_) => {
                tracing::debug!(%uri, queue_wait_ms, compute_ms, "diagnostics cancelled");
                Task::DiagnosticsCancelled { generation }
            }
        }
    });
}

/// Handles textDocument/didOpen notification.
///
/// When a document is opened in the editor:
/// 1. Store the full text in MemDocs
/// 2. Update VFS with file content
/// 3. Sync changes to Salsa database via process_changes()
/// 4. Preload dependencies in background for fast GoToDefinition
pub fn handle_did_open(state: &mut GlobalState, params: DidOpenTextDocumentParams) -> Result<()> {
    let _p = tracing::info_span!("handle_did_open", uri = %params.text_document.uri).entered();
    if !is_handled_uri(&params.text_document.uri) {
        tracing::debug!(uri = %params.text_document.uri, "didOpen: ignoring non-BSL file");
        return Ok(());
    }
    let start = Instant::now();
    let vfs_done = state.vfs_done;

    let uri = params.text_document.uri;
    let text = params.text_document.text;
    let version = params.text_document.version;

    tracing::debug!("Document opened: {} (version {})", uri, version);

    // Store in MemDocs for incremental updates
    state.mem_docs.insert(uri.clone(), text.clone(), version);

    // Get or create FileId
    let file_id = state.vfs_file_for_url(&uri)?;

    // Update VFS with file content
    {
        let vfs_path = vfs::VfsPath::new(
            uri.to_file_path().map_err(|()| anyhow::anyhow!("Not a file URI: {}", uri))?,
        );
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    // Process VFS changes and sync to Salsa database. During cold-start
    // (`!vfs_done`) loader batches stream straight into `Vfs::changes`,
    // so a pre-finish drain here will pick them up too — that is fine
    // (set_file_text is idempotent and the bulk-finish drain just becomes
    // a no-op for already-synced files), but suppress the metadata-cache
    // version bump while `warm_metadata_cache` has not yet populated it.
    let process_start = Instant::now();
    state.process_changes(!vfs_done);
    let process_changes_ms = process_start.elapsed().as_millis() as u64;

    // Preload dependencies in background for fast GoToDefinition
    let preload_start = Instant::now();
    preload_dependencies(state, file_id);
    let preload_dispatch_ms = preload_start.elapsed().as_millis() as u64;

    // Schedule diagnostics in background (didOpen runs immediately, no batching benefit)
    schedule_diagnostics(state, &uri);

    tracing::info!(
        %uri,
        vfs_done,
        process_changes_ms,
        preload_dispatch_ms,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "handle_did_open complete",
    );

    Ok(())
}

/// Preloads dependencies of a file in background.
///
/// This warms up caches for modules that the opened file depends on:
/// - symbol_tree (for fast GoToDefinition)
/// - module_bodies (for hover info)
/// - diagnostics (for fast diagnostics on dependency navigation)
///
/// Any previous preload for the same head `file_id` is cancelled cooperatively
/// via its Salsa `CancellationToken`, so repeated `didOpen` does not pile up
/// stale warming tasks. Cancellation is also triggered from `handle_did_close`.
fn preload_dependencies(state: &mut GlobalState, file_id: vfs::FileId) {
    let discover_start = Instant::now();
    let analysis = state.analysis_host.analysis();
    let deps = analysis.file_dependencies(file_id);
    let discover_ms = discover_start.elapsed().as_millis() as u64;

    if deps.is_empty() {
        tracing::debug!(file_id = file_id.0, discover_ms, "preload: no dependencies found",);
        return;
    }

    let dep_count = deps.len();
    let dep_ids: Vec<u32> = deps.iter().map(|f| f.0).collect();
    tracing::info!(
        file_id = file_id.0,
        dep_count,
        discover_ms,
        ?dep_ids,
        "preload: dispatching warm-cache task",
    );

    if let Some(prev) = state.preload_tokens.remove(&file_id) {
        prev.cancel();
    }

    let task = analysis.warm_caches_task(&deps);
    state.preload_tokens.insert(file_id, task.cancellation_token());
    let queued_at = Instant::now();

    state.task_pool.pool.spawn(move || {
        let started_at = Instant::now();
        let queue_wait_ms = started_at.duration_since(queued_at).as_millis() as u64;
        let count = match salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| task.run())) {
            Ok(count) => {
                tracing::info!(
                    file_id = file_id.0,
                    dep_count = count,
                    queue_wait_ms,
                    run_ms = started_at.elapsed().as_millis() as u64,
                    "preload: warm-cache complete",
                );
                count
            }
            Err(_) => {
                tracing::debug!(
                    file_id = file_id.0,
                    queue_wait_ms,
                    run_ms = started_at.elapsed().as_millis() as u64,
                    "preload: warm-cache cancelled",
                );
                0
            }
        };
        Task::DependenciesPreloaded { file_id, count }
    });
}

/// Handles textDocument/didChange notification.
///
/// When a document is modified:
/// 1. Apply incremental changes to MemDocs
/// 2. Update VFS with new content
/// 3. Sync changes to Salsa database via process_changes() (triggers incremental recomputation)
pub fn handle_did_change(
    state: &mut GlobalState,
    params: DidChangeTextDocumentParams,
) -> Result<()> {
    let _p = tracing::info_span!("handle_did_change", uri = %params.text_document.uri).entered();
    if !is_handled_uri(&params.text_document.uri) {
        tracing::debug!(uri = %params.text_document.uri, "didChange: ignoring non-BSL file");
        return Ok(());
    }

    let uri = params.text_document.uri;
    let version = params.text_document.version;

    tracing::debug!(
        "Document changed: {} (version {}, {} changes)",
        uri,
        version,
        params.content_changes.len()
    );

    // Apply changes to MemDocs
    state.mem_docs.update(&uri, params.content_changes);

    // Get updated text
    let text = state
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    // Update VFS
    {
        let vfs_path = vfs::VfsPath::new(
            uri.to_file_path().map_err(|()| anyhow::anyhow!("Not a file URI: {}", uri))?,
        );
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    // Process VFS changes and sync to Salsa database (triggers incremental recomputation).
    // During cold-start the loader streams batches into `Vfs::changes` too; a pre-finish
    // drain here is idempotent with the bulk drain at LoadingProgress::Finished, but we
    // suppress the metadata-cache bump until `warm_metadata_cache` has populated it.
    state.process_changes(!state.vfs_done);

    tracing::debug!("Document updated successfully: {}", uri);

    // Mark file as pending diagnostics (scheduled after event loop drains all messages)
    state.pending_diagnostics_uri = Some(uri);

    Ok(())
}

/// Handles textDocument/didClose notification.
///
/// When a document is closed:
/// 1. Remove from MemDocs
/// 2. Optionally keep in VFS (for goto definition across files)
pub fn handle_did_close(state: &mut GlobalState, params: DidCloseTextDocumentParams) -> Result<()> {
    let _p = tracing::info_span!("handle_did_close", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;

    tracing::debug!("Document closed: {}", uri);

    if let Some(token) = state.diagnostics_tokens.remove(&uri) {
        token.cancel();
    }
    match crate::lsp::file_id(state, &uri) {
        Ok(file_id) => {
            if let Some(token) = state.preload_tokens.remove(&file_id) {
                token.cancel();
            }
        }
        Err(e) => {
            tracing::warn!(%uri, error = %e, "didClose: could not resolve file_id for preload cleanup")
        }
    }

    // Remove from MemDocs
    state.mem_docs.remove(&uri);

    // Clear diagnostics
    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics: vec![], version: None };

    let notification = Notification::new("textDocument/publishDiagnostics".to_string(), params);

    state.sender.send(notification.into())?;

    // Note: We keep the file in VFS for cross-file operations
    // It will be garbage collected later if needed

    tracing::debug!("Document closed successfully: {}", uri);

    Ok(())
}

/// Handles textDocument/didSave notification.
///
/// If the saved file is a config file (.bsl-analyzer.json or .bsl-language-server.json),
/// reloads the project config and recalculates diagnostics for all open documents.
pub fn handle_did_save(state: &mut GlobalState, params: DidSaveTextDocumentParams) -> Result<()> {
    let _p = tracing::info_span!("handle_did_save", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;

    tracing::debug!("Document saved: {}", uri);

    if let Ok(path) = uri.to_file_path() {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if (name == "bsl-analyzer.toml"
                || name == ".bsl-analyzer.json"
                || name == ".bsl-language-server.json")
                && state.reload_project_config()
            {
                for uri in state.opened_document_uris() {
                    schedule_diagnostics(state, &uri);
                }
            }
        }
    }

    Ok(())
}

/// Handles `$/cancelRequest` notification.
///
/// Looks up the `salsa::CancellationToken` tracked by `on_latency` for the
/// given request id and cancels it. The background worker unwinds
/// cooperatively at the next Salsa query boundary, producing a
/// `RequestCanceled` response via the normal `Task::RequestResult` path.
///
/// Missing ids are a normal race (worker beat the cancel to the finish line);
/// just log at debug level.
pub fn handle_cancel(state: &mut GlobalState, params: lsp_types::CancelParams) -> Result<()> {
    let id = match params.id {
        lsp_types::NumberOrString::Number(n) => lsp_server::RequestId::from(n),
        lsp_types::NumberOrString::String(s) => lsp_server::RequestId::from(s),
    };

    if let Some(token) = state.request_tokens.remove(&id) {
        tracing::debug!(request_id = ?id, "cancelling in-flight async request");
        token.cancel();
    } else {
        tracing::debug!(request_id = ?id, "cancel arrived after response (no-op)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::{unbounded, Receiver};
    use lsp_server::Message;
    use lsp_types::{
        TextDocumentContentChangeEvent, TextDocumentItem, VersionedTextDocumentIdentifier,
    };

    fn create_test_state() -> (GlobalState, Receiver<Message>) {
        let (sender, receiver) = unbounded();
        let mut state = GlobalState::new(sender);

        // Initialize SourceRoot for tests (normally done by VFS loader)
        use base_db::{SourceDatabase, SourceRoot, SourceRootId};
        let db = state.analysis_host.raw_database_mut();
        let source_root_id = SourceRootId(0);
        let file_set = vfs::FileSet::new();
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(source_root_id, source_root);

        (state, receiver)
    }

    #[test]
    fn test_did_open() {
        let (mut state, _receiver) = create_test_state();

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: lsp_types::Url::parse("file:///test.bsl").unwrap(),
                language_id: "bsl".to_string(),
                version: 1,
                text: "Процедура Тест() КонецПроцедуры".to_string(),
            },
        };

        let result = handle_did_open(&mut state, params.clone());
        assert!(result.is_ok());

        // Check MemDocs
        assert!(state.mem_docs.contains(&params.text_document.uri));
        assert_eq!(state.mem_docs.get(&params.text_document.uri), Some(params.text_document.text));
    }

    #[test]
    fn test_did_change() {
        let (mut state, _receiver) = create_test_state();

        // First open the document
        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();
        handle_did_open(
            &mut state,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bsl".to_string(),
                    version: 1,
                    text: "old text".to_string(),
                },
            },
        )
        .unwrap();

        // Now change it
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 2 },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "new text".to_string(),
            }],
        };

        let result = handle_did_change(&mut state, params);
        assert!(result.is_ok());

        // Check MemDocs has updated text
        assert_eq!(state.mem_docs.get(&uri), Some("new text".to_string()));
    }

    #[test]
    fn handle_cancel_cancels_and_evicts_token() {
        let (mut state, _receiver) = create_test_state();

        let db = state.analysis_host.raw_database().clone();
        let token = db.cancellation_token();
        let id = lsp_server::RequestId::from(123);
        state.request_tokens.insert(id.clone(), token.clone());

        let params = lsp_types::CancelParams { id: lsp_types::NumberOrString::Number(123) };
        handle_cancel(&mut state, params).unwrap();

        assert!(token.is_cancelled(), "token must be cancelled after $/cancelRequest");
        assert!(!state.request_tokens.contains_key(&id), "token must be evicted from map");
    }

    #[test]
    fn handle_cancel_is_noop_for_unknown_id() {
        let (mut state, _receiver) = create_test_state();

        // No token registered for id 999.
        let params = lsp_types::CancelParams { id: lsp_types::NumberOrString::Number(999) };
        let result = handle_cancel(&mut state, params);
        assert!(result.is_ok());
    }

    #[test]
    fn handle_cancel_supports_string_ids() {
        let (mut state, _receiver) = create_test_state();

        let db = state.analysis_host.raw_database().clone();
        let token = db.cancellation_token();
        let id = lsp_server::RequestId::from("req-abc".to_string());
        state.request_tokens.insert(id.clone(), token.clone());

        let params = lsp_types::CancelParams {
            id: lsp_types::NumberOrString::String("req-abc".to_string()),
        };
        handle_cancel(&mut state, params).unwrap();

        assert!(token.is_cancelled());
        assert!(!state.request_tokens.contains_key(&id));
    }

    #[test]
    fn test_did_close() {
        let (mut state, _receiver) = create_test_state();

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();

        // Open document
        handle_did_open(
            &mut state,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bsl".to_string(),
                    version: 1,
                    text: "test".to_string(),
                },
            },
        )
        .unwrap();

        assert!(state.mem_docs.contains(&uri));

        // Close document
        handle_did_close(
            &mut state,
            DidCloseTextDocumentParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .unwrap();

        // Should be removed from MemDocs
        assert!(!state.mem_docs.contains(&uri));
    }
}
