use crate::document::{semantic_key_for_indexed_document, semantic_text_for_indexed_document};
use crate::domain::{IndexedDocument, Snapshot, SnapshotPublishMetadata, SnapshotPublishStats};
use crate::error::SearchError;
use crate::ports::{EmbeddingGenerator, EmbeddingStore, SnapshotPublisher};
use crossbeam_channel::bounded;
use std::collections::BTreeMap;
use std::thread;
use tracing::info;

const DEFAULT_BATCH_SIZE: usize = 32;
const DEFAULT_CONCURRENCY: usize = 10;
const DEFAULT_PROGRESS_INTERVAL: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingExecutionPolicy {
    pub batch_size: usize,
    pub concurrency: usize,
    pub progress_interval: usize,
}

impl EmbeddingExecutionPolicy {
    pub fn batch_size(&self) -> usize {
        self.batch_size.max(1)
    }

    pub fn concurrency(&self) -> usize {
        self.concurrency.max(1)
    }

    pub fn progress_interval(&self) -> usize {
        self.progress_interval.max(1)
    }
}

impl Default for EmbeddingExecutionPolicy {
    fn default() -> Self {
        Self {
            batch_size: DEFAULT_BATCH_SIZE,
            concurrency: DEFAULT_CONCURRENCY,
            progress_interval: DEFAULT_PROGRESS_INTERVAL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedEmbeddingPublishStats {
    pub model_id: String,
    pub dimension: usize,
    pub reused: usize,
    pub stored: usize,
    pub total_unique: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselinePublishReport {
    pub snapshot: SnapshotPublishStats,
    pub embeddings: Option<SharedEmbeddingPublishStats>,
}

#[derive(Debug, Clone)]
pub struct SharedEmbeddingPublisher {
    policy: EmbeddingExecutionPolicy,
}

impl SharedEmbeddingPublisher {
    pub fn new(policy: EmbeddingExecutionPolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &EmbeddingExecutionPolicy {
        &self.policy
    }

    pub fn publish<S, E>(
        &self,
        store: &S,
        embedder: &E,
        documents: &[IndexedDocument],
    ) -> Result<SharedEmbeddingPublishStats, SearchError>
    where
        S: EmbeddingStore,
        E: EmbeddingGenerator + Clone + Send + 'static,
    {
        let dimension = embedder.dimension();
        let model_id = embedder.model_id().to_owned();
        if documents.is_empty() {
            return Ok(SharedEmbeddingPublishStats {
                model_id,
                dimension,
                reused: 0,
                stored: 0,
                total_unique: 0,
            });
        }

        let unique_documents = documents
            .iter()
            .map(|document| {
                (
                    semantic_key_for_indexed_document(document),
                    semantic_text_for_indexed_document(document),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let total_unique = unique_documents.len();
        let embedding_keys = unique_documents.keys().cloned().collect::<Vec<_>>();
        let existing = store.load_embeddings(&embedding_keys, &model_id, dimension)?;
        let reused = existing.len();

        let missing = unique_documents
            .into_iter()
            .filter(|(key, _)| !existing.contains_key(key))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(SharedEmbeddingPublishStats {
                model_id,
                dimension,
                reused,
                stored: 0,
                total_unique,
            });
        }

        let batch_size = self.policy.batch_size();
        let progress_interval = self.policy.progress_interval();
        let total_missing = missing.len();
        let total_batches = total_missing.div_ceil(batch_size);
        let concurrency = self.policy.concurrency().min(total_batches.max(1));

        let batched = missing
            .chunks(batch_size)
            .map(|batch| batch.to_vec())
            .collect::<Vec<Vec<(String, String)>>>();

        let (task_tx, task_rx) = bounded::<Vec<(String, String)>>(concurrency * 2);
        let (result_tx, result_rx) =
            bounded::<Result<Vec<(String, Vec<f32>)>, SearchError>>(concurrency * 2);

        let workers = (0..concurrency)
            .map(|_| {
                let rx = task_rx.clone();
                let tx = result_tx.clone();
                let emb = embedder.clone();
                thread::spawn(move || {
                    while let Ok(batch) = rx.recv() {
                        let texts = batch.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>();
                        let result = emb.embed_batch(&texts).map(|vectors| {
                            batch
                                .into_iter()
                                .zip(vectors)
                                .map(|((embedding_key, _), embedding)| (embedding_key, embedding))
                                .collect::<Vec<_>>()
                        });
                        let _ = tx.send(result);
                    }
                })
            })
            .collect::<Vec<_>>();
        drop(task_rx);
        drop(result_tx);

        let producer = thread::spawn(move || {
            for batch in batched {
                if task_tx.send(batch).is_err() {
                    break;
                }
            }
        });

        let mut processed = 0usize;
        let mut stored_total = 0usize;
        let mut reused_total = reused;
        let mut batch_index = 0usize;
        let mut first_error = None;

        while batch_index < total_batches {
            let result = result_rx.recv().map_err(|_| {
                SearchError::Embedder(
                    "embedding worker pool terminated before all batches were processed".to_owned(),
                )
            })?;
            batch_index += 1;

            match result {
                Ok(generated) if first_error.is_none() => {
                    let stats = store.store_embeddings(&model_id, dimension, &generated)?;
                    stored_total += stats.stored;
                    reused_total += stats.reused;
                    processed += generated.len();
                    if batch_index.is_multiple_of(progress_interval) || processed == total_missing {
                        info!(
                            model_id = %model_id,
                            processed,
                            total_missing,
                            batches_done = batch_index,
                            total_batches,
                            stored_total,
                            reused_total,
                            "shared embedding publish progress"
                        );
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }

        let _ = producer.join();
        for worker in workers {
            let _ = worker.join();
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(SharedEmbeddingPublishStats {
            model_id,
            dimension,
            reused: reused_total,
            stored: stored_total,
            total_unique,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BaselinePublisher {
    shared_embeddings: SharedEmbeddingPublisher,
}

impl BaselinePublisher {
    pub fn new(policy: EmbeddingExecutionPolicy) -> Self {
        Self { shared_embeddings: SharedEmbeddingPublisher::new(policy) }
    }

    pub fn shared_embeddings(&self) -> &SharedEmbeddingPublisher {
        &self.shared_embeddings
    }

    pub fn publish<S, E>(
        &self,
        store: &S,
        snapshot: &Snapshot,
        metadata: &SnapshotPublishMetadata,
        documents: &[IndexedDocument],
        embedder: Option<&E>,
    ) -> Result<BaselinePublishReport, SearchError>
    where
        S: SnapshotPublisher + EmbeddingStore,
        E: EmbeddingGenerator + Clone + Send + 'static,
    {
        store.ensure_storage()?;
        let snapshot_stats = store.publish_snapshot(snapshot, metadata, documents)?;
        let embeddings = match embedder {
            Some(embedder) => Some(self.shared_embeddings.publish(store, embedder, documents)?),
            None => None,
        };
        Ok(BaselinePublishReport { snapshot: snapshot_stats, embeddings })
    }
}

#[cfg(test)]
mod tests {
    use super::{BaselinePublisher, EmbeddingExecutionPolicy, SharedEmbeddingPublisher};
    use crate::domain::{
        CorpusId, IndexedDocument, Snapshot, SnapshotPublishMetadata, SnapshotPublishStats,
    };
    use crate::error::SearchError;
    use crate::external_baseline::BaselineEmbeddingStats;
    use crate::ports::{EmbeddingGenerator, EmbeddingStore, SnapshotPublisher};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type SharedEmbeddingMap = Arc<Mutex<HashMap<(String, String, usize), Vec<f32>>>>;

    #[derive(Clone, Default)]
    struct FakeEmbeddingStore {
        embeddings: SharedEmbeddingMap,
        stored_batches: Arc<Mutex<Vec<usize>>>,
        ensure_storage_calls: Arc<Mutex<usize>>,
        publish_calls: Arc<Mutex<usize>>,
    }

    impl EmbeddingStore for FakeEmbeddingStore {
        fn load_embeddings(
            &self,
            embedding_keys: &[String],
            model_id: &str,
            dimension: usize,
        ) -> Result<HashMap<String, Vec<f32>>, SearchError> {
            let embeddings = self.embeddings.lock().unwrap();
            Ok(embedding_keys
                .iter()
                .filter_map(|key| {
                    embeddings
                        .get(&(key.clone(), model_id.to_owned(), dimension))
                        .cloned()
                        .map(|value| (key.clone(), value))
                })
                .collect())
        }

        fn store_embeddings(
            &self,
            model_id: &str,
            dimension: usize,
            embeddings: &[(String, Vec<f32>)],
        ) -> Result<BaselineEmbeddingStats, SearchError> {
            self.stored_batches.lock().unwrap().push(embeddings.len());
            let mut stored = 0usize;
            let mut reused = 0usize;
            let mut state = self.embeddings.lock().unwrap();
            for (key, value) in embeddings {
                let entry = (key.clone(), model_id.to_owned(), dimension);
                if state.insert(entry, value.clone()).is_some() {
                    reused += 1;
                } else {
                    stored += 1;
                }
            }
            Ok(BaselineEmbeddingStats { stored, reused })
        }
    }

    impl SnapshotPublisher for FakeEmbeddingStore {
        fn ensure_storage(&self) -> Result<(), SearchError> {
            *self.ensure_storage_calls.lock().unwrap() += 1;
            Ok(())
        }

        fn publish_snapshot(
            &self,
            _snapshot: &Snapshot,
            _metadata: &SnapshotPublishMetadata,
            _documents: &[IndexedDocument],
        ) -> Result<SnapshotPublishStats, SearchError> {
            *self.publish_calls.lock().unwrap() += 1;
            Ok(SnapshotPublishStats {
                reused_files: 3,
                written_files: 2,
                deleted_files: 1,
                reused_documents: 10,
                written_documents: 4,
            })
        }
    }

    #[derive(Clone, Default)]
    struct FakeEmbedder {
        calls: Arc<Mutex<Vec<usize>>>,
    }

    impl EmbeddingGenerator for FakeEmbedder {
        fn model_id(&self) -> &str {
            "fake-model"
        }

        fn dimension(&self) -> usize {
            3
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, SearchError> {
            self.calls.lock().unwrap().push(texts.len());
            Ok(texts.iter().map(|text| vec![text.len() as f32, 1.0, 0.0]).collect())
        }
    }

    #[test]
    fn shared_embedding_publisher_reuses_existing_and_batches_missing() {
        let store = FakeEmbeddingStore::default();
        let existing_doc = indexed_document("path/0.bsl", "existing");
        let existing_key = crate::semantic_key_for_indexed_document(&existing_doc);
        store
            .embeddings
            .lock()
            .unwrap()
            .insert((existing_key, "fake-model".to_owned(), 3), vec![1.0, 1.0, 1.0]);

        let documents = vec![
            existing_doc,
            indexed_document("path/1.bsl", "one"),
            indexed_document("path/2.bsl", "two"),
            indexed_document("path/3.bsl", "three"),
            indexed_document("path/4.bsl", "four"),
        ];

        let publisher = SharedEmbeddingPublisher::new(EmbeddingExecutionPolicy {
            batch_size: 2,
            concurrency: 3,
            progress_interval: 1,
        });
        let stats = publisher.publish(&store, &FakeEmbedder::default(), &documents).unwrap();

        assert_eq!(stats.model_id, "fake-model");
        assert_eq!(stats.dimension, 3);
        assert_eq!(stats.reused, 1);
        assert_eq!(stats.stored, 4);
        assert_eq!(stats.total_unique, 5);
        let mut batch_sizes = store.stored_batches.lock().unwrap().clone();
        batch_sizes.sort_unstable();
        assert_eq!(batch_sizes, vec![2, 2]);
    }

    #[test]
    fn baseline_publisher_orchestrates_snapshot_and_embeddings() {
        let store = FakeEmbeddingStore::default();
        let documents = vec![indexed_document("path/1.bsl", "one")];
        let publisher = BaselinePublisher::new(EmbeddingExecutionPolicy::default());
        let report = publisher
            .publish(
                &store,
                &Snapshot::new("workspace-code:test@1", CorpusId::WorkspaceCode),
                &SnapshotPublishMetadata {
                    branch: Some("test".to_owned()),
                    commit: Some("1".to_owned()),
                },
                &documents,
                Some(&FakeEmbedder::default()),
            )
            .unwrap();

        assert_eq!(*store.ensure_storage_calls.lock().unwrap(), 1);
        assert_eq!(*store.publish_calls.lock().unwrap(), 1);
        assert_eq!(report.snapshot.written_files, 2);
        assert!(report.embeddings.is_some());
        assert_eq!(report.embeddings.unwrap().stored, 1);
    }

    fn indexed_document(path: &str, text: &str) -> IndexedDocument {
        IndexedDocument {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: path.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 2,
            text: text.to_owned(),
            content_hash: format!("hash:{path}:{text}"),
        }
    }
}
