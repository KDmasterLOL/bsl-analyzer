//! Handlers for LSP notifications.
//!
//! This module implements handlers for LSP notifications like
//! textDocument/didOpen, didChange, didClose, and didSave.

use std::sync::Arc;

use anyhow::Result;
use base_db::FileIdInput;
use ide_diagnostics::file_diagnostics_query;
use line_index::LineIndex;
use lsp_server::Notification;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, PublishDiagnosticsParams, Url,
};

use crate::global_state::{GlobalState, Task};

/// Schedules diagnostics computation in a background thread.
///
/// The background thread clones the Salsa database and runs `file_diagnostics_query`.
/// If a new `set_file_text()` is called before the query finishes, Salsa cancels
/// the in-flight query via `zalsa_mut()` → `cancel_others()`, the background thread
/// panics, `catch_unwind` catches it, and the stale result is discarded by generation check.
pub fn schedule_diagnostics(state: &mut GlobalState, uri: &Url) {
    // Don't schedule diagnostics until VFS is ready.
    // They will be scheduled again after VFS loading completes.
    if !state.vfs_done {
        tracing::debug!("VFS not ready, skipping diagnostics scheduling");
        return;
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
    let config = state.diagnostics_config().clone();
    let uri = uri.clone();

    state.task_pool.pool.spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let file_id_input = FileIdInput::new(&db, file_id);
            let config_id = base_db::DiagnosticsConfigId::new(&db, config);
            let ide_diagnostics = file_diagnostics_query(&db, file_id_input, config_id);
            let line_index = LineIndex::new(&text);
            crate::lsp::diagnostics(&line_index, &text, &ide_diagnostics)
        }));

        match result {
            Ok(diagnostics) => Task::DiagnosticsReady { uri, diagnostics, generation },
            Err(_) => Task::DiagnosticsCancelled { generation },
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
        let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    // Process VFS changes and sync to Salsa database
    state.process_changes();

    tracing::debug!("Document opened successfully: {}", uri);

    // Preload dependencies in background for fast GoToDefinition
    preload_dependencies(state, file_id);

    // Schedule diagnostics in background (didOpen runs immediately, no batching benefit)
    schedule_diagnostics(state, &uri);

    Ok(())
}

/// Preloads dependencies of a file in background.
///
/// This warms up caches for modules that the opened file depends on:
/// - symbol_tree (for fast GoToDefinition)
/// - module_bodies (for hover info)
/// - diagnostics (for fast diagnostics on dependency navigation)
fn preload_dependencies(state: &GlobalState, file_id: vfs::FileId) {
    use base_db::DiagnosticsConfigId;
    use ide_db::hir_def::{DefDatabase, ModuleId};

    // Get file dependencies (resolved ExternalRefs → FileIds)
    let analysis = state.analysis_host.analysis();
    let module_id = ModuleId::new(file_id);
    let deps = analysis.database().file_dependencies(module_id);

    if deps.is_empty() {
        tracing::debug!(file_id = file_id.0, "preload: no dependencies found");
        return;
    }

    let dep_count = deps.len();
    let dep_ids: Vec<u32> = deps.iter().map(|f| f.0).collect();
    tracing::debug!(file_id = file_id.0, dep_count, ?dep_ids, "preload: starting background task");

    // Clone what we need for the background task
    let deps = deps.as_ref().clone();
    let db = analysis.database().clone();
    let config = state.diagnostics_config().clone();

    // Spawn background task to warm up caches for dependencies
    // This includes symbol_tree (for GoToDefinition), module_bodies (for hover),
    // and diagnostics (for fast diagnostics when navigating to dependencies)
    state.task_pool.pool.spawn(move || {
        tracing::debug!(dep_count = deps.len(), "preload: background task started");

        // Create config ID once for all files (Salsa interns it)
        let config_id = DiagnosticsConfigId::new(&db, config);

        for dep_file_id in &deps {
            let dep_module_id = ModuleId::new(*dep_file_id);

            tracing::debug!(dep_file_id = dep_file_id.0, "preload: warming symbol_tree");
            // Warm up symbol_tree (for GoToDefinition resolution)
            let _ = db.symbol_tree(dep_module_id);

            tracing::debug!(dep_file_id = dep_file_id.0, "preload: warming module_bodies");
            // Warm up module_bodies (for diagnostics, hover info)
            // This also primes item_tree and other intermediate caches
            let _ = db.module_bodies(dep_module_id);

            tracing::debug!(dep_file_id = dep_file_id.0, "preload: warming diagnostics");
            // Warm up diagnostics (so opening a dependency file is instant)
            let file_id_input = FileIdInput::new(&db, *dep_file_id);
            let _ = file_diagnostics_query(&db, file_id_input, config_id);
        }
        tracing::debug!(dep_count = deps.len(), "preload: background task completed");
        Task::DependenciesPreloaded { file_id, count: deps.len() }
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
        let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    // Process VFS changes and sync to Salsa database (triggers incremental recomputation)
    state.process_changes();

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
/// Currently a no-op, but could be used for:
/// - Triggering additional analysis
/// - Running formatters
/// - Updating external caches
pub fn handle_did_save(_state: &mut GlobalState, params: DidSaveTextDocumentParams) -> Result<()> {
    let _p = tracing::info_span!("handle_did_save", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;

    tracing::debug!("Document saved: {}", uri);

    // Future: Could trigger additional analysis or formatting here

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
