use crate::domain::{BaselineRef, CorpusId, DocumentPath, IndexedDocument, SearchOverlay};
use crate::embedder::Embedder;
use crate::error::SearchError;
use crate::lexical::lexical_hits_for_documents;
use crate::ports::GraphContextProvider;
use crate::store::Store;
use code_chunk::Chunker;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineHashMode {
    RawFileBytes,
    NormalizedChunks,
}

#[derive(Debug, Clone)]
pub struct OverlayVectorDocument {
    pub document: IndexedDocument,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceOverlayIndex {
    pub overlay: SearchOverlay,
    pub hidden_paths: HashSet<String>,
    pub lexical_documents: Vec<IndexedDocument>,
    pub vector_documents: Vec<OverlayVectorDocument>,
}

impl WorkspaceOverlayIndex {
    pub fn is_empty(&self) -> bool {
        self.overlay.changes.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOverlayStats {
    pub overlay_files: usize,
    pub deleted_files: usize,
    pub hidden_paths: usize,
    pub lexical_chunks: usize,
    pub semantic_chunks: usize,
    pub cached_embeddings: usize,
    pub watcher_mode: bool,
    pub pending_dirty_paths: usize,
}

#[derive(Default)]
pub struct WorkspaceOverlayCache {
    entries: HashMap<String, OverlayFileEntry>,
    hidden_paths: HashSet<String>,
    embedding_cache: HashMap<String, Vec<f32>>,
    dirty_paths: HashSet<String>,
    watcher_mode: bool,
    initialized: bool,
    /// Optional graph-context provider (dependency-inverted). When set, overlay
    /// (uncommitted-edit) chunks are enriched with their call-graph context before
    /// embedding, matching the local index.
    graph_context_provider: Option<Arc<dyn GraphContextProvider>>,
}

impl std::fmt::Debug for WorkspaceOverlayCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceOverlayCache")
            .field("entries", &self.entries)
            .field("hidden_paths", &self.hidden_paths)
            .field("embedding_cache_len", &self.embedding_cache.len())
            .field("dirty_paths", &self.dirty_paths)
            .field("watcher_mode", &self.watcher_mode)
            .field("initialized", &self.initialized)
            .field("graph_context", &self.graph_context_provider.is_some())
            .finish()
    }
}

impl WorkspaceOverlayCache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.hidden_paths.clear();
        self.dirty_paths.clear();
        self.initialized = false;
    }

    /// Inject the graph-context provider so overlay chunks are enriched like the
    /// local index. Clears cached entries so they rebuild with context.
    pub fn set_graph_context_provider(&mut self, provider: Arc<dyn GraphContextProvider>) {
        self.graph_context_provider = Some(provider);
        self.entries.clear();
        self.embedding_cache.clear();
        self.initialized = false;
    }

    pub fn enable_watcher_mode(&mut self) {
        self.watcher_mode = true;
    }

    pub fn mark_dirty_path(&mut self, rel_path: impl Into<String>) {
        self.dirty_paths.insert(rel_path.into());
    }

    pub fn refresh_with_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        store: &Store,
    ) -> Result<(), SearchError> {
        if !self.initialized || !self.watcher_mode {
            self.full_refresh_from_manifest(
                manifest_fingerprints,
                workspace_root,
                embedder,
                batch_size,
                store,
            )?;
        } else if !self.dirty_paths.is_empty() {
            self.refresh_dirty_paths_from_manifest(
                manifest_fingerprints,
                workspace_root,
                embedder,
                batch_size,
            )?;
        }
        self.initialized = true;
        Ok(())
    }

    pub fn refresh(
        &mut self,
        store: &Store,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let baseline_files: HashMap<String, Vec<u8>> =
            store.all_files_in_collection("code")?.into_iter().collect();
        if !self.initialized || !self.watcher_mode {
            self.full_refresh(&baseline_files, workspace_root, embedder, batch_size, hash_mode)?;
        } else if !self.dirty_paths.is_empty() {
            self.refresh_dirty_paths(
                &baseline_files,
                workspace_root,
                embedder,
                batch_size,
                hash_mode,
            )?;
        }
        self.initialized = true;
        Ok(())
    }

    fn full_refresh(
        &mut self,
        baseline_files: &HashMap<String, Vec<u8>>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let workspace_files = scan_workspace_files(workspace_root);
        let mut seen_paths = HashSet::new();
        let mut hidden_paths = HashSet::new();

        for file in workspace_files {
            seen_paths.insert(file.rel_path.clone());
            let baseline_hash = baseline_files.get(&file.rel_path);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.rel_path) {
                if entry.fingerprint == file.fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_hash.is_some() {
                            hidden_paths.insert(file.rel_path.clone());
                        }
                        if let Some(embedder) = embedder {
                            if entry.vector_documents.is_empty() {
                                entry.vector_documents = build_overlay_vectors(
                                    embedder,
                                    batch_size,
                                    &entry.lexical_documents,
                                    &entry.embedding_inputs,
                                    &mut self.embedding_cache,
                                )?;
                            }
                        }
                        continue;
                    }
                }
            }
            if should_remove_cached_entry {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.rel_path,
                &content,
                file.fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;
            if baseline_hash.is_some() {
                hidden_paths.insert(file.rel_path.clone());
            }
            self.entries.insert(file.rel_path, entry);
        }

        self.entries.retain(|path, _| seen_paths.contains(path));

        for rel_path in baseline_files.keys() {
            if !seen_paths.contains(rel_path) {
                hidden_paths.insert(rel_path.clone());
            }
        }

        self.hidden_paths = hidden_paths;
        self.dirty_paths.clear();
        Ok(())
    }

    fn refresh_dirty_paths(
        &mut self,
        baseline_files: &HashMap<String, Vec<u8>>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        hash_mode: BaselineHashMode,
    ) -> Result<(), SearchError> {
        let dirty_paths: Vec<String> = self.dirty_paths.drain().collect();

        for rel_path in dirty_paths {
            let baseline_hash = baseline_files.get(&rel_path);
            let abs_path = workspace_root.join(&rel_path);

            if !abs_path.exists() {
                self.entries.remove(&rel_path);
                if baseline_hash.is_some() {
                    self.hidden_paths.insert(rel_path);
                } else {
                    self.hidden_paths.remove(&rel_path);
                }
                continue;
            }

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&rel_path) {
                if entry.fingerprint == fingerprint {
                    if baseline_hash.is_some_and(|stored_hash| stored_hash == &entry.file_hash) {
                        self.entries.remove(&rel_path);
                        self.hidden_paths.remove(&rel_path);
                    } else {
                        if baseline_hash.is_some() {
                            self.hidden_paths.insert(rel_path.clone());
                        } else {
                            self.hidden_paths.remove(&rel_path);
                        }
                        if let Some(embedder) = embedder {
                            if entry.vector_documents.is_empty() {
                                entry.vector_documents = build_overlay_vectors(
                                    embedder,
                                    batch_size,
                                    &entry.lexical_documents,
                                    &entry.embedding_inputs,
                                    &mut self.embedding_cache,
                                )?;
                            }
                        }
                    }
                    continue;
                }
            }

            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = compute_file_hash(&content, hash_mode);
            if baseline_hash.is_some_and(|stored_hash| stored_hash == &file_hash) {
                self.entries.remove(&rel_path);
                self.hidden_paths.remove(&rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &rel_path,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;

            if baseline_hash.is_some() {
                self.hidden_paths.insert(rel_path.clone());
            } else {
                self.hidden_paths.remove(&rel_path);
            }
            self.entries.insert(rel_path, entry);
        }

        Ok(())
    }

    fn full_refresh_from_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
        store: &Store,
    ) -> Result<(), SearchError> {
        let manifest_snapshot_id = store
            .load_baseline_manifest()
            .ok()
            .flatten()
            .map(|r| r.snapshot_id)
            .unwrap_or_default();
        let persisted = store
            .load_overlay_fingerprint_cache(&manifest_snapshot_id)
            .unwrap_or(None)
            .unwrap_or_default();

        if self.embedding_cache.is_empty() {
            if let Some(embedder) = embedder {
                let model_id = embedder.model();
                let dim = embedder.dim();
                match store.load_overlay_embedding_cache(model_id, dim) {
                    Ok(cached) if !cached.is_empty() => {
                        tracing::info!(
                            model_id,
                            dim,
                            cached_embeddings = cached.len(),
                            "loaded persisted overlay embedding cache"
                        );
                        self.embedding_cache = cached;
                    }
                    _ => {}
                }
            }
        }

        let workspace_files = scan_workspace_files(workspace_root);
        let mut seen_paths = HashSet::new();
        let mut hidden_paths = HashSet::new();
        let mut updated_persisted = HashMap::new();

        for file in &workspace_files {
            seen_paths.insert(file.rel_path.clone());
            let baseline_fingerprint = manifest_fingerprints.get(&file.rel_path);

            let mut should_remove_cached_entry = false;
            if let Some(entry) = self.entries.get_mut(&file.rel_path) {
                if entry.fingerprint == file.fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &file.rel_path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        should_remove_cached_entry = true;
                    } else {
                        if baseline_fingerprint.is_some() {
                            hidden_paths.insert(file.rel_path.clone());
                        }
                        if let Some(embedder) = embedder {
                            if entry.vector_documents.is_empty() {
                                entry.vector_documents = build_overlay_vectors(
                                    embedder,
                                    batch_size,
                                    &entry.lexical_documents,
                                    &entry.embedding_inputs,
                                    &mut self.embedding_cache,
                                )?;
                            }
                        }
                        continue;
                    }
                }
            }
            if should_remove_cached_entry {
                self.entries.remove(&file.rel_path);
                continue;
            }

            if let Some(cached) = persisted.get(&file.rel_path) {
                if cached.file_size == file.fingerprint.len
                    && fingerprint_mtime_matches(file.fingerprint.modified, cached)
                {
                    updated_persisted.insert(file.rel_path.clone(), cached.clone());

                    if baseline_fingerprint
                        .is_some_and(|stored| stored == &cached.content_fingerprint)
                    {
                        self.entries.remove(&file.rel_path);
                        continue;
                    }
                }
            }

            let content = match std::fs::read_to_string(&file.abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &file.rel_path);

            if let Some((secs, nanos)) = mtime_to_secs_nanos(file.fingerprint.modified) {
                updated_persisted.insert(
                    file.rel_path.clone(),
                    crate::store::PersistedFingerprint {
                        file_size: file.fingerprint.len,
                        file_mtime_secs: secs,
                        file_mtime_nanos: nanos,
                        content_fingerprint: local_fp.clone(),
                    },
                );
            }

            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&file.rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &file.rel_path,
                &content,
                file.fingerprint.clone(),
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;
            if baseline_fingerprint.is_some() {
                hidden_paths.insert(file.rel_path.clone());
            }
            self.entries.insert(file.rel_path.clone(), entry);
        }

        self.entries.retain(|path, _| seen_paths.contains(path));

        for rel_path in manifest_fingerprints.keys() {
            if !seen_paths.contains(rel_path) {
                hidden_paths.insert(rel_path.clone());
            }
        }

        self.hidden_paths = hidden_paths;
        self.dirty_paths.clear();

        if !updated_persisted.is_empty() {
            if let Err(error) =
                store.save_overlay_fingerprint_cache(&manifest_snapshot_id, &updated_persisted)
            {
                tracing::warn!("failed to persist overlay fingerprint cache: {error}");
            }
        }

        if let Some(embedder) = embedder {
            if !self.embedding_cache.is_empty() {
                if let Err(error) = store.save_overlay_embedding_cache(
                    embedder.model(),
                    embedder.dim(),
                    &self.embedding_cache,
                ) {
                    tracing::warn!("failed to persist overlay embedding cache: {error}");
                }
            }
        }

        Ok(())
    }

    fn refresh_dirty_paths_from_manifest(
        &mut self,
        manifest_fingerprints: &HashMap<String, String>,
        workspace_root: &Path,
        embedder: Option<&Embedder>,
        batch_size: usize,
    ) -> Result<(), SearchError> {
        let dirty_paths: Vec<String> = self.dirty_paths.drain().collect();

        for rel_path in dirty_paths {
            let baseline_fingerprint = manifest_fingerprints.get(&rel_path);
            let abs_path = workspace_root.join(&rel_path);

            if !abs_path.exists() {
                self.entries.remove(&rel_path);
                if baseline_fingerprint.is_some() {
                    self.hidden_paths.insert(rel_path);
                } else {
                    self.hidden_paths.remove(&rel_path);
                }
                continue;
            }

            let metadata = match std::fs::metadata(&abs_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let fingerprint =
                FileFingerprint { len: metadata.len(), modified: metadata.modified().ok() };

            if let Some(entry) = self.entries.get_mut(&rel_path) {
                if entry.fingerprint == fingerprint {
                    let local_fp =
                        fingerprint_overlay_documents(&entry.lexical_documents, &rel_path);
                    if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                        self.entries.remove(&rel_path);
                        self.hidden_paths.remove(&rel_path);
                    } else {
                        if baseline_fingerprint.is_some() {
                            self.hidden_paths.insert(rel_path.clone());
                        } else {
                            self.hidden_paths.remove(&rel_path);
                        }
                        if let Some(embedder) = embedder {
                            if entry.vector_documents.is_empty() {
                                entry.vector_documents = build_overlay_vectors(
                                    embedder,
                                    batch_size,
                                    &entry.lexical_documents,
                                    &entry.embedding_inputs,
                                    &mut self.embedding_cache,
                                )?;
                            }
                        }
                    }
                    continue;
                }
            }

            let content = match std::fs::read_to_string(&abs_path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            let file_hash = normalized_file_hash_for_content(&content);
            let local_fp = fingerprint_content(&content, &rel_path);
            if baseline_fingerprint.is_some_and(|stored| stored == &local_fp) {
                self.entries.remove(&rel_path);
                self.hidden_paths.remove(&rel_path);
                continue;
            }

            let provider = self.graph_context_provider.clone();
            let entry = build_overlay_entry(
                &rel_path,
                &content,
                fingerprint,
                file_hash,
                embedder,
                batch_size,
                &mut self.embedding_cache,
                provider.as_deref(),
            )?;

            if baseline_fingerprint.is_some() {
                self.hidden_paths.insert(rel_path.clone());
            } else {
                self.hidden_paths.remove(&rel_path);
            }
            self.entries.insert(rel_path, entry);
        }

        Ok(())
    }

    pub fn snapshot(&self) -> WorkspaceOverlayIndex {
        let baseline =
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline");
        let mut overlay = SearchOverlay::new(baseline);
        let mut lexical_documents = Vec::new();
        let mut vector_documents = Vec::new();

        let mut entry_paths: Vec<&String> = self.entries.keys().collect();
        entry_paths.sort();
        for rel_path in entry_paths {
            let entry = self.entries.get(rel_path).expect("path collected from map keys");
            overlay.replace_file(
                DocumentPath::new("code", rel_path.clone()),
                entry.lexical_documents.clone(),
            );
            lexical_documents.extend(entry.lexical_documents.clone());
            vector_documents.extend(entry.vector_documents.clone());
        }

        let mut deleted_paths: Vec<&String> =
            self.hidden_paths.iter().filter(|path| !self.entries.contains_key(*path)).collect();
        deleted_paths.sort();
        for rel_path in deleted_paths {
            overlay.delete_file(DocumentPath::new("code", rel_path.clone()));
        }

        WorkspaceOverlayIndex {
            overlay,
            hidden_paths: self.hidden_paths.clone(),
            lexical_documents,
            vector_documents,
        }
    }

    pub fn stats(&self) -> WorkspaceOverlayStats {
        let overlay_files = self.entries.len();
        let hidden_paths = self.hidden_paths.len();
        let deleted_files =
            self.hidden_paths.iter().filter(|path| !self.entries.contains_key(*path)).count();
        let lexical_chunks =
            self.entries.values().map(|entry| entry.lexical_documents.len()).sum::<usize>();
        let semantic_chunks =
            self.entries.values().map(|entry| entry.vector_documents.len()).sum::<usize>();

        WorkspaceOverlayStats {
            overlay_files,
            deleted_files,
            hidden_paths,
            lexical_chunks,
            semantic_chunks,
            cached_embeddings: self.embedding_cache.len(),
            watcher_mode: self.watcher_mode,
            pending_dirty_paths: self.dirty_paths.len(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    len: u64,
    modified: Option<SystemTime>,
}

fn fingerprint_mtime_matches(
    mtime: Option<SystemTime>,
    cached: &crate::store::PersistedFingerprint,
) -> bool {
    let Some((secs, nanos)) = mtime_to_secs_nanos(mtime) else {
        return false;
    };
    secs == cached.file_mtime_secs && nanos == cached.file_mtime_nanos
}

fn mtime_to_secs_nanos(mtime: Option<SystemTime>) -> Option<(i64, u32)> {
    let duration = mtime?.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some((duration.as_secs() as i64, duration.subsec_nanos()))
}

#[derive(Debug, Clone)]
struct OverlayFileEntry {
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    lexical_documents: Vec<IndexedDocument>,
    vector_documents: Vec<OverlayVectorDocument>,
    embedding_inputs: Vec<String>,
}

#[derive(Debug, Clone)]
struct WorkspaceFileState {
    rel_path: String,
    abs_path: PathBuf,
    fingerprint: FileFingerprint,
}

fn scan_workspace_files(workspace_root: &Path) -> Vec<WorkspaceFileState> {
    walkdir::WalkDir::new(workspace_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("bsl")))
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let rel_path = entry
                .path()
                .strip_prefix(workspace_root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .to_string();
            Some(WorkspaceFileState {
                rel_path,
                abs_path: entry.into_path(),
                fingerprint: FileFingerprint {
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_overlay_entry(
    rel_path: &str,
    content: &str,
    fingerprint: FileFingerprint,
    file_hash: Vec<u8>,
    embedder: Option<&Embedder>,
    batch_size: usize,
    embedding_cache: &mut HashMap<String, Vec<f32>>,
    graph_context: Option<&dyn GraphContextProvider>,
) -> Result<OverlayFileEntry, SearchError> {
    let (lexical_documents, embedding_inputs) =
        build_overlay_documents(rel_path, content, graph_context);
    let vector_documents = if let Some(embedder) = embedder {
        build_overlay_vectors(
            embedder,
            batch_size,
            &lexical_documents,
            &embedding_inputs,
            embedding_cache,
        )?
    } else {
        Vec::new()
    };

    Ok(OverlayFileEntry {
        fingerprint,
        file_hash,
        lexical_documents,
        vector_documents,
        embedding_inputs,
    })
}

fn compute_file_hash(content: &str, hash_mode: BaselineHashMode) -> Vec<u8> {
    match hash_mode {
        BaselineHashMode::RawFileBytes => blake3::hash(content.as_bytes()).as_bytes().to_vec(),
        BaselineHashMode::NormalizedChunks => normalized_file_hash_for_content(content),
    }
}

pub(crate) fn normalized_file_hash_for_content(content: &str) -> Vec<u8> {
    let chunks = Chunker::chunk(content);
    normalized_file_hash_for_chunks(chunks.iter().map(|chunk| {
        (
            chunk.kind.label(),
            chunk.name.as_str(),
            chunk.line_start,
            chunk.line_end,
            chunk.text.as_str(),
        )
    }))
}

pub(crate) fn normalized_file_hash_for_indexed_documents(documents: &[IndexedDocument]) -> Vec<u8> {
    normalized_file_hash_for_chunks(documents.iter().map(|document| {
        (
            document.kind.as_str(),
            document.symbol_name.as_str(),
            document.line_start,
            document.line_end,
            document.text.as_str(),
        )
    }))
}

fn normalized_file_hash_for_chunks<'a>(
    chunks: impl Iterator<Item = (&'a str, &'a str, u32, u32, &'a str)>,
) -> Vec<u8> {
    let mut hasher = blake3::Hasher::new();
    for (kind, name, line_start, line_end, text) in chunks {
        hasher.update(kind.as_bytes());
        hasher.update(&[0]);
        hasher.update(name.as_bytes());
        hasher.update(&[0]);
        hasher.update(&line_start.to_le_bytes());
        hasher.update(&line_end.to_le_bytes());
        hasher.update(text.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().as_bytes().to_vec()
}

pub(crate) fn fingerprint_content(content: &str, rel_path: &str) -> String {
    let documents = Chunker::chunk(content);
    let mut hasher = blake3::Hasher::new();
    for chunk in &documents {
        let kind = match chunk.kind {
            code_chunk::ChunkKind::ModuleHeader => "header",
            code_chunk::ChunkKind::Procedure => "procedure",
            code_chunk::ChunkKind::Function => "function",
        };
        let content_hash = blake3::hash(chunk.text.as_bytes()).to_hex().to_string();
        hasher.update("code".as_bytes());
        hasher.update(&[0]);
        hasher.update(rel_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(chunk.name.as_bytes());
        hasher.update(&[0]);
        hasher.update(kind.as_bytes());
        hasher.update(&chunk.line_start.to_le_bytes());
        hasher.update(&chunk.line_end.to_le_bytes());
        hasher.update(content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(chunk.text.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn fingerprint_overlay_documents(
    documents: &[IndexedDocument],
    rel_path: &str,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for document in documents {
        hasher.update(document.collection.as_bytes());
        hasher.update(&[0]);
        hasher.update(rel_path.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.symbol_name.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.kind.as_bytes());
        hasher.update(&document.line_start.to_le_bytes());
        hasher.update(&document.line_end.to_le_bytes());
        hasher.update(document.content_hash.as_bytes());
        hasher.update(&[0]);
        hasher.update(document.text.as_bytes());
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

fn build_overlay_documents(
    rel_path: &str,
    content: &str,
    graph_context: Option<&dyn GraphContextProvider>,
) -> (Vec<IndexedDocument>, Vec<String>) {
    let chunks = Chunker::chunk(content);
    let mut lexical_documents = Vec::with_capacity(chunks.len());
    let mut embedding_inputs = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let document = crate::document::indexed_document_for_chunk(rel_path, chunk, graph_context);
        embedding_inputs.push(crate::document::semantic_text_for_indexed_document(&document));
        lexical_documents.push(document);
    }

    (lexical_documents, embedding_inputs)
}

fn build_overlay_vectors(
    embedder: &Embedder,
    batch_size: usize,
    documents: &[IndexedDocument],
    embedding_inputs: &[String],
    embedding_cache: &mut HashMap<String, Vec<f32>>,
) -> Result<Vec<OverlayVectorDocument>, SearchError> {
    if documents.is_empty() {
        return Ok(Vec::new());
    }

    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; documents.len()];
    let mut missing_indexes = Vec::new();
    let mut missing_inputs = Vec::new();

    for (idx, document) in documents.iter().enumerate() {
        if let Some(embedding) = embedding_cache.get(&document.content_hash) {
            vectors[idx] = Some(embedding.clone());
        } else {
            missing_indexes.push(idx);
            missing_inputs.push(embedding_inputs[idx].as_str());
        }
    }

    for (batch_indexes, batch_inputs) in
        missing_indexes.chunks(batch_size.max(1)).zip(missing_inputs.chunks(batch_size.max(1)))
    {
        // The overlay refresh always runs with the engine mutex held (interactive search or the
        // warmup prime), so the embed must be fail-fast, not the indexing path's 120s x 10 retry.
        let embeddings = embedder.embed_batch_interactive(batch_inputs)?;
        for (idx, embedding) in batch_indexes.iter().copied().zip(embeddings) {
            embedding_cache.insert(documents[idx].content_hash.clone(), embedding.clone());
            vectors[idx] = Some(embedding);
        }
    }

    Ok(documents
        .iter()
        .cloned()
        .zip(vectors)
        .map(|(document, embedding)| OverlayVectorDocument {
            document,
            embedding: embedding.unwrap_or_default(),
        })
        .collect())
}

pub fn lexical_hits(
    overlay: &WorkspaceOverlayIndex,
    query: &str,
    limit: usize,
) -> Vec<crate::engine::SearchHit> {
    lexical_hits_for_documents(overlay.lexical_documents.iter(), query, limit)
}

pub fn semantic_hits(
    overlay: &WorkspaceOverlayIndex,
    query_embedding: &[f32],
    limit: usize,
) -> Vec<crate::engine::SearchHit> {
    let mut hits: Vec<crate::engine::SearchHit> = overlay
        .vector_documents
        .iter()
        .map(|document| crate::engine::SearchHit {
            collection: document.document.collection.clone(),
            file_path: document.document.path.clone(),
            symbol_name: document.document.symbol_name.clone(),
            kind: document.document.kind.clone(),
            text: document.document.text.clone(),
            line_start: document.document.line_start,
            line_end: document.document.line_end,
            score: cosine_similarity(query_embedding, &document.embedding),
        })
        .collect();

    hits.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
    hits.truncate(limit);
    hits
}

fn cosine_similarity(lhs: &[f32], rhs: &[f32]) -> f32 {
    if lhs.len() != rhs.len() || lhs.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut lhs_norm = 0.0f32;
    let mut rhs_norm = 0.0f32;

    for (&left, &right) in lhs.iter().zip(rhs.iter()) {
        dot += left * right;
        lhs_norm += left * left;
        rhs_norm += right * right;
    }

    let denom = lhs_norm.sqrt() * rhs_norm.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::{
        fingerprint_content, lexical_hits, BaselineHashMode, WorkspaceOverlayCache,
        WorkspaceOverlayStats,
    };
    use crate::store::Store;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn overlay_detects_changed_and_deleted_files() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file_a = workspace.join("A.bsl");
        let file_b = workspace.join("B.bsl");
        fs::write(&file_a, "Процедура Старая()\nКонецПроцедуры").unwrap();
        fs::write(&file_b, "Процедура Удаляемая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks_a = crate::Chunker::chunk(&fs::read_to_string(&file_a).unwrap());
        let chunks_b = crate::Chunker::chunk(&fs::read_to_string(&file_b).unwrap());
        let hash_a = blake3::hash(fs::read(&file_a).unwrap().as_slice());
        let hash_b = blake3::hash(fs::read(&file_b).unwrap().as_slice());
        store.reindex_file("A.bsl", hash_a.as_bytes(), &chunks_a, None).unwrap();
        store.reindex_file("B.bsl", hash_b.as_bytes(), &chunks_b, None).unwrap();

        fs::write(&file_a, "Процедура НовоеИмя()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        let overlay = cache.snapshot();

        assert!(overlay.hidden_paths.contains("A.bsl"));
        assert!(overlay.hidden_paths.contains("B.bsl"));
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "НовоеИмя");
    }

    #[test]
    fn lexical_hits_rank_overlay_matches() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура НоваяПроцедура123()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        let overlay = cache.snapshot();

        let hits = lexical_hits(&overlay, "НоваяПроцедура123", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].symbol_name, "НоваяПроцедура123");
    }

    #[test]
    fn refresh_updates_only_changed_overlay_state() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура ВерсияОдин111()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let store = Store::open(&db_path).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        let first = cache.snapshot();
        assert_eq!(first.lexical_documents[0].symbol_name, "ВерсияОдин111");

        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        let second = cache.snapshot();
        assert_eq!(second.lexical_documents[0].symbol_name, "ВерсияОдин111");

        fs::write(&file, "Процедура ВерсияДва222222()\nКонецПроцедуры").unwrap();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        let third = cache.snapshot();
        assert_eq!(third.lexical_documents[0].symbol_name, "ВерсияДва222222");
    }

    #[test]
    fn stats_report_overlay_shape() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file_a = workspace.join("A.bsl");
        let file_b = workspace.join("B.bsl");
        fs::write(&file_a, "Процедура Первая()\nКонецПроцедуры").unwrap();
        fs::write(&file_b, "Процедура Вторая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks_a = crate::Chunker::chunk(&fs::read_to_string(&file_a).unwrap());
        let chunks_b = crate::Chunker::chunk(&fs::read_to_string(&file_b).unwrap());
        let hash_a = blake3::hash(fs::read(&file_a).unwrap().as_slice());
        let hash_b = blake3::hash(fs::read(&file_b).unwrap().as_slice());
        store.reindex_file("A.bsl", hash_a.as_bytes(), &chunks_a, None).unwrap();
        store.reindex_file("B.bsl", hash_b.as_bytes(), &chunks_b, None).unwrap();

        fs::write(&file_a, "Процедура Измененная()\nКонецПроцедуры").unwrap();
        fs::remove_file(&file_b).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();

        assert_eq!(
            cache.stats(),
            WorkspaceOverlayStats {
                overlay_files: 1,
                deleted_files: 1,
                hidden_paths: 2,
                lexical_chunks: 1,
                semantic_chunks: 0,
                cached_embeddings: 0,
                watcher_mode: false,
                pending_dirty_paths: 0,
            }
        );
    }

    #[test]
    fn watcher_mode_refreshes_only_marked_paths() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Базовая()\nКонецПроцедуры").unwrap();

        let db_path = workspace.join("search.db");
        let mut store = Store::open(&db_path).unwrap();
        let chunks = crate::Chunker::chunk(&fs::read_to_string(&file).unwrap());
        let hash = blake3::hash(fs::read(&file).unwrap().as_slice());
        store.reindex_file("A.bsl", hash.as_bytes(), &chunks, None).unwrap();

        let mut cache = WorkspaceOverlayCache::default();
        cache.enable_watcher_mode();
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();
        assert_eq!(cache.stats().overlay_files, 0);

        fs::write(&file, "Процедура ИзWatcher()\nКонецПроцедуры").unwrap();
        cache.mark_dirty_path("A.bsl");
        cache.refresh(&store, workspace, None, 32, BaselineHashMode::RawFileBytes).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "ИзWatcher");
        assert_eq!(cache.stats().pending_dirty_paths, 0);
        assert!(cache.stats().watcher_mode);
    }

    #[test]
    fn manifest_refresh_treats_all_files_as_new_without_manifest() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Новая()\nКонецПроцедуры").unwrap();

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        let manifest: HashMap<String, String> = HashMap::new();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Новая");
        assert!(overlay.hidden_paths.is_empty());
    }

    #[test]
    fn manifest_refresh_detects_unchanged_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let content = "Процедура Базовая()\nКонецПроцедуры";
        let file = workspace.join("A.bsl");
        fs::write(&file, content).unwrap();

        let fp = fingerprint_content(content, "A.bsl");
        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), fp);

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert!(!overlay.hidden_paths.contains("A.bsl"));
    }

    #[test]
    fn manifest_refresh_detects_modified_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();
        let file = workspace.join("A.bsl");
        fs::write(&file, "Процедура Старая()\nКонецПроцедуры").unwrap();

        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "different-fingerprint".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 1);
        assert_eq!(overlay.lexical_documents[0].symbol_name, "Старая");
        assert!(overlay.hidden_paths.contains("A.bsl"));
    }

    #[test]
    fn manifest_refresh_detects_deleted_baseline_file() {
        let dir = tempdir().unwrap();
        let workspace = dir.path();

        let mut manifest = HashMap::new();
        manifest.insert("A.bsl".to_owned(), "some-fp".to_owned());
        manifest.insert("B.bsl".to_owned(), "other-fp".to_owned());

        let store = Store::open(&workspace.join("search.db")).unwrap();
        let mut cache = WorkspaceOverlayCache::default();
        cache.refresh_with_manifest(&manifest, workspace, None, 32, &store).unwrap();

        let overlay = cache.snapshot();
        assert_eq!(overlay.lexical_documents.len(), 0);
        assert_eq!(overlay.hidden_paths.len(), 2);
        assert!(overlay.hidden_paths.contains("A.bsl"));
        assert!(overlay.hidden_paths.contains("B.bsl"));
    }
}
