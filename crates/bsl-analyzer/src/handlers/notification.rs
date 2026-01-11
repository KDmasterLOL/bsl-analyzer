//! Handlers for LSP notifications.
//!
//! This module implements handlers for LSP notifications like
//! textDocument/didOpen, didChange, didClose, and didSave.

use std::sync::Arc;

use anyhow::Result;
use ide::DiagnosticsConfig;
use line_index::LineIndex;
use lsp_server::Notification;
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, PublishDiagnosticsParams, Url,
};

use crate::global_state::GlobalState;

/// Publishes diagnostics for a file.
fn publish_diagnostics(state: &mut GlobalState, uri: &Url) -> Result<()> {
    let file_id = crate::lsp::file_id(state, uri)?;

    // Get file text for line index
    let text = state
        .mem_docs
        .get(uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    let line_index = LineIndex::new(&text);

    // Run diagnostics
    let config = DiagnosticsConfig::default();
    let analysis = state.analysis_host.analysis();

    let ide_diagnostics = analysis.diagnostics(file_id, &config);
    tracing::info!(
        "Diagnostics computed successfully: {} diagnostics found",
        ide_diagnostics.len()
    );

    // Convert to LSP diagnostics
    let lsp_diagnostics = crate::lsp::diagnostics(&line_index, &text, &ide_diagnostics);
    tracing::info!("Publishing {} diagnostics for {}", lsp_diagnostics.len(), uri);

    // Publish
    let params =
        PublishDiagnosticsParams { uri: uri.clone(), diagnostics: lsp_diagnostics, version: None };

    let notification = Notification::new("textDocument/publishDiagnostics".to_string(), params);

    state.sender.send(notification.into())?;

    Ok(())
}

/// Handles textDocument/didOpen notification.
///
/// When a document is opened in the editor:
/// 1. Store the full text in MemDocs
/// 2. Update VFS with file content
/// 3. Sync changes to Salsa database via process_changes()
pub fn handle_did_open(state: &mut GlobalState, params: DidOpenTextDocumentParams) -> Result<()> {
    let _p = tracing::info_span!("handle_did_open", uri = %params.text_document.uri).entered();

    let uri = params.text_document.uri;
    let text = params.text_document.text;
    let version = params.text_document.version;

    tracing::debug!("Document opened: {} (version {})", uri, version);

    // Store in MemDocs for incremental updates
    state.mem_docs.insert(uri.clone(), text.clone(), version);

    // Get or create FileId
    let _file_id = state.vfs_file_for_url(&uri)?;

    // Update VFS with file content
    {
        let vfs_path = vfs::VfsPath::new(uri.to_file_path().unwrap());
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    // Process VFS changes and sync to Salsa database
    state.process_changes();

    tracing::debug!("Document opened successfully: {}", uri);

    // Publish diagnostics
    publish_diagnostics(state, &uri)?;

    Ok(())
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

    // Publish diagnostics
    publish_diagnostics(state, &uri)?;

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
