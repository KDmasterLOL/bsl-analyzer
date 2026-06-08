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
use salsa::Database as _;

use crate::global_state::{GlobalState, Task};

fn is_handled_uri(uri: &Url) -> bool {
    uri.to_file_path().map(|p| project_model::is_bsl_source_path(&p)).unwrap_or(false)
}

pub fn schedule_diagnostics(state: &mut GlobalState, uri: &Url) {
    if let Some(prev) = state.diagnostics_tokens.remove(uri) {
        prev.cancel();
    }

    let file_id = match crate::lsp::file_id(state, uri) {
        Ok(id) => id,
        Err(_) => return,
    };
    let text = match state.mem_docs.get(uri) {
        Some(t) => t,
        None => return,
    };

    // Advance the generation only once the schedule will actually spawn, so a
    // no-op call (unresolved file / not an open buffer) cannot orphan a prior
    // in-flight result by leaving `current` ahead of every spawned task.
    let generation = {
        let g = state.diagnostics_generation.entry(uri.clone()).or_insert(0);
        *g += 1;
        *g
    };

    let db = state.analysis_host.raw_database().clone();
    state.diagnostics_tokens.insert(uri.clone(), db.cancellation_token());
    let config = state.diagnostics_config().clone();
    let position_encoding = state.position_encoding;
    let uri = uri.clone();
    let queued_at = Instant::now();
    tracing::info!(%uri, generation, vfs_done = state.vfs_done, "diagnostics scheduled");

    let analysis_guard = state.note_analysis_spawned();
    state.task_pool.pool.spawn(move || {
        let _analysis_guard = analysis_guard;
        let started_at = Instant::now();
        let queue_wait_ms = started_at.duration_since(queued_at).as_millis() as u64;
        tracing::info!(%uri, generation, queue_wait_ms, "diagnostics worker started");
        let result = salsa::Cancelled::catch(std::panic::AssertUnwindSafe(|| {
            let file_id_input = FileIdInput::new(&db, file_id);
            let config_id = base_db::DiagnosticsConfigId::new(&db, config);
            let ide_diagnostics = file_diagnostics_query(&db, file_id_input, config_id);
            let line_index = LineIndex::new(&text);
            crate::lsp::diagnostics_with_encoding(
                &line_index,
                &text,
                &ide_diagnostics,
                position_encoding,
            )
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
                Task::DiagnosticsReady {
                    uri,
                    diagnostics,
                    generation,
                    completed_at: Instant::now(),
                }
            }
            Err(_) => {
                tracing::debug!(%uri, queue_wait_ms, compute_ms, "diagnostics cancelled");
                Task::DiagnosticsCancelled { generation, completed_at: Instant::now() }
            }
        }
    });
}

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

    state.mem_docs.insert(uri.clone(), text.clone(), version);

    let file_id = state.vfs_file_for_url(&uri)?;
    // Mark open BEFORE process_changes so the edit is stored as a resident
    // overlay (authoritative for unsaved content), not disk-backed.
    state.open_files.insert(file_id);

    {
        let vfs_path = vfs::VfsPath::new(
            uri.to_file_path().map_err(|()| anyhow::anyhow!("Not a file URI: {}", uri))?,
        );
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    let process_start = Instant::now();
    state.process_changes(!vfs_done);
    let process_changes_ms = process_start.elapsed().as_millis() as u64;

    let preload_start = Instant::now();
    preload_dependencies(state, file_id);
    let preload_dispatch_ms = preload_start.elapsed().as_millis() as u64;

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

    let analysis_guard = state.note_analysis_spawned();
    state.task_pool.pool.spawn(move || {
        let _analysis_guard = analysis_guard;
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

    if let Err(err) =
        state.mem_docs.update_with_encoding(&uri, params.content_changes, state.position_encoding)
    {
        tracing::error!(%uri, error = %err, encoding = ?state.position_encoding, "didChange edit rejected");
        return Ok(());
    }

    let text = state
        .mem_docs
        .get(&uri)
        .ok_or_else(|| anyhow::anyhow!("Document not in MemDocs: {}", uri))?;

    {
        let vfs_path = vfs::VfsPath::new(
            uri.to_file_path().map_err(|()| anyhow::anyhow!("Not a file URI: {}", uri))?,
        );
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(vfs_path, Some(Arc::from(text.as_str())));
    }

    state.process_changes(!state.vfs_done);

    tracing::debug!("Document updated successfully: {}", uri);

    state.pending_diagnostics_uri = Some(uri);

    Ok(())
}

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
            state.open_files.remove(&file_id);
            // The file is no longer open: drop its editor-buffer overlay and
            // re-key on the on-disk content so its text becomes LRU-evictable
            // (discarding any unsaved edits, which the client also discards).
            // If it isn't readable from disk (e.g. never saved), keep the
            // overlay as a safe fallback rather than leave a dangling revision.
            let disk = {
                let vfs = state.vfs.read();
                let path = vfs.file_path(file_id).as_path().to_path_buf();
                std::fs::read_to_string(path).ok()
            };
            if let Some(content) = disk {
                use base_db::SourceDatabase;
                let db = state.analysis_host.raw_database_mut();
                db.set_file_revision_from_disk(file_id, base_db::content_revision(&content));
            }
        }
        Err(e) => {
            tracing::warn!(%uri, error = %e, "didClose: could not resolve file_id for preload cleanup")
        }
    }

    state.mem_docs.remove(&uri);

    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics: vec![], version: None };

    let notification = Notification::new("textDocument/publishDiagnostics".to_string(), params);

    state.sender.send(notification.into())?;

    tracing::debug!("Document closed successfully: {}", uri);

    Ok(())
}

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
        TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
        VersionedTextDocumentIdentifier,
    };

    fn create_test_state() -> (GlobalState, Receiver<Message>) {
        let (sender, receiver) = unbounded();
        let mut state = GlobalState::new(sender);

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

        assert!(state.mem_docs.contains(&params.text_document.uri));
        assert_eq!(state.mem_docs.get(&params.text_document.uri), Some(params.text_document.text));

        assert!(!state.vfs_done);
        assert_eq!(state.diagnostics_generation.get(&params.text_document.uri).copied(), Some(1));
        assert!(state.diagnostics_tokens.contains_key(&params.text_document.uri));
    }

    #[test]
    fn diagnostics_generation_is_tracked_per_uri() {
        let (mut state, _receiver) = create_test_state();

        let a = lsp_types::Url::parse("file:///a.bsl").unwrap();
        let b = lsp_types::Url::parse("file:///b.bsl").unwrap();
        for (uri, text) in
            [(&a, "Процедура А() КонецПроцедуры"), (&b, "Процедура Б() КонецПроцедуры")]
        {
            handle_did_open(
                &mut state,
                DidOpenTextDocumentParams {
                    text_document: TextDocumentItem {
                        uri: uri.clone(),
                        language_id: "bsl".to_string(),
                        version: 1,
                        text: text.to_string(),
                    },
                },
            )
            .unwrap();
        }

        // Each document carries its own generation: opening B must NOT advance A's,
        // otherwise A's in-flight diagnostics task would be discarded as stale and
        // never published (the multi-document refresh bug).
        assert_eq!(state.diagnostics_generation.get(&a).copied(), Some(1));
        assert_eq!(state.diagnostics_generation.get(&b).copied(), Some(1));

        // Re-scheduling A advances only A's generation.
        schedule_diagnostics(&mut state, &a);
        assert_eq!(state.diagnostics_generation.get(&a).copied(), Some(2));
        assert_eq!(state.diagnostics_generation.get(&b).copied(), Some(1));
    }

    #[test]
    fn did_close_drops_overlay_and_re_keys_open_file_to_disk() {
        use base_db::SourceDatabase;

        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("mod.bsl");
        std::fs::write(&path, "Процедура НаДиске() КонецПроцедуры").expect("write");

        let (mut state, _receiver) = create_test_state();
        let uri = lsp_types::Url::from_file_path(&path).unwrap();

        // Open with an unsaved edit (buffer differs from disk) → resident overlay.
        handle_did_open(
            &mut state,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bsl".to_string(),
                    version: 1,
                    text: "Процедура Буфер() КонецПроцедуры".to_string(),
                },
            },
        )
        .unwrap();

        let file_id = state.vfs_file_for_url(&uri).unwrap();
        assert!(state.open_files.contains(&file_id), "open file is tracked in open_files");
        {
            let db = state.analysis_host.raw_database_mut();
            assert!(db.try_file_text(file_id).is_some(), "open file keeps a resident overlay");
            assert_eq!(&*db.file_text(file_id), "Процедура Буфер() КонецПроцедуры");
        }

        // Close: the editor buffer is gone, so the file re-keys to its on-disk
        // content (the unsaved edit is discarded) and the overlay is dropped so
        // the text becomes disk-backed / evictable.
        handle_did_close(
            &mut state,
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .unwrap();

        assert!(!state.open_files.contains(&file_id), "closed file is removed from open_files");
        {
            let db = state.analysis_host.raw_database_mut();
            assert!(
                db.try_file_text(file_id).is_none(),
                "closed file overlay is cleared (disk-backed)"
            );
            assert_eq!(&*db.file_text(file_id), "Процедура НаДиске() КонецПроцедуры");
        }
    }

    #[test]
    fn test_did_change() {
        let (mut state, _receiver) = create_test_state();

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

        assert_eq!(state.mem_docs.get(&uri), Some("new text".to_string()));
    }

    #[test]
    fn test_did_change_uses_negotiated_utf8_encoding() {
        let (mut state, _receiver) = create_test_state();
        state.position_encoding = crate::lsp::PositionEncoding::Utf8;

        let uri = lsp_types::Url::parse("file:///test.bsl").unwrap();
        handle_did_open(
            &mut state,
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "bsl".to_string(),
                    version: 1,
                    text: "Процедура Тест()\nКонецПроцедуры".to_string(),
                },
            },
        )
        .unwrap();

        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri: uri.clone(), version: 2 },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: Some(lsp_types::Range {
                    start: lsp_types::Position { line: 0, character: 19 },
                    end: lsp_types::Position { line: 0, character: 19 },
                }),
                range_length: Some(0),
                text: "Новая".to_string(),
            }],
        };

        handle_did_change(&mut state, params).unwrap();

        assert_eq!(
            state.mem_docs.get(&uri),
            Some("Процедура НоваяТест()\nКонецПроцедуры".to_string())
        );
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

        handle_did_close(
            &mut state,
            DidCloseTextDocumentParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: uri.clone() },
            },
        )
        .unwrap();

        assert!(!state.mem_docs.contains(&uri));
    }
}
