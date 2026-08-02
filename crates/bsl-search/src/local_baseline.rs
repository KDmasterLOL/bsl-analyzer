use crate::domain::{BaselineRef, CorpusId, IndexedDocument, Snapshot};
use crate::error::SearchError;
use crate::ports::{SnapshotCatalog, SnapshotContentStore};
use crate::Store;

pub struct LocalStoreBaselineAdapter<'a> {
    store: &'a Store,
    corpus: CorpusId,
    collection: &'static str,
    snapshot_id: &'static str,
}

impl<'a> LocalStoreBaselineAdapter<'a> {
    pub fn workspace_code(store: &'a Store) -> Self {
        Self {
            store,
            corpus: CorpusId::WorkspaceCode,
            collection: "code",
            snapshot_id: "local-workspace-baseline",
        }
    }

    pub fn reference(store: &'a Store) -> Self {
        Self {
            store,
            corpus: CorpusId::Reference,
            collection: "platform",
            snapshot_id: "local-reference-baseline",
        }
    }
}

impl SnapshotCatalog for LocalStoreBaselineAdapter<'_> {
    fn resolve_baseline(&self, baseline: &BaselineRef) -> Result<Option<Snapshot>, SearchError> {
        if baseline.corpus != self.corpus {
            return Ok(None);
        }
        if let Some(snapshot_id) = &baseline.snapshot_id {
            if snapshot_id.0 != self.snapshot_id {
                return Ok(None);
            }
        }
        if baseline.branch.is_some() || baseline.commit.is_some() {
            return Ok(None);
        }

        Ok(Some(Snapshot::new(self.snapshot_id, self.corpus.clone())))
    }
}

impl SnapshotContentStore for LocalStoreBaselineAdapter<'_> {
    fn load_snapshot_documents(
        &self,
        snapshot: &Snapshot,
    ) -> Result<Vec<IndexedDocument>, SearchError> {
        if snapshot.corpus != self.corpus || snapshot.id.0 != self.snapshot_id {
            return Ok(Vec::new());
        }
        self.store.load_indexed_documents(Some(self.collection))
    }
}

#[cfg(test)]
mod tests {
    use super::LocalStoreBaselineAdapter;
    use crate::baseline_runtime::BaselineOverlaySearchService;
    use crate::ports::{SnapshotCatalog, SnapshotContentStore};
    use crate::workspace_roots::CONFIGURATION_ROOT_ID;
    use crate::{
        BaselineRef, Chunker, CorpusId, DocumentPath, InMemoryResolvedViewResolver, SearchOverlay,
        Store,
    };

    #[test]
    fn local_store_adapter_resolves_workspace_baseline() {
        let mut store = Store::in_memory().unwrap();
        let content = "Процедура Исходная()\nКонецПроцедуры";
        let chunks = Chunker::chunk(content);
        let hash = blake3::hash(content.as_bytes());
        store
            .reindex_file(
                CONFIGURATION_ROOT_ID,
                "CommonModules/A.bsl",
                hash.as_bytes(),
                &chunks,
                None,
            )
            .unwrap();

        let adapter = LocalStoreBaselineAdapter::workspace_code(&store);
        let baseline =
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline");
        let snapshot = adapter.resolve_baseline(&baseline).unwrap().unwrap();
        let docs = adapter.load_snapshot_documents(&snapshot).unwrap();

        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].symbol_name, "Исходная");
    }

    #[test]
    fn local_store_adapter_works_with_baseline_overlay_service() {
        let mut store = Store::in_memory().unwrap();
        let content = "Процедура Исходная()\nКонецПроцедуры";
        let chunks = Chunker::chunk(content);
        let hash = blake3::hash(content.as_bytes());
        store
            .reindex_file(
                CONFIGURATION_ROOT_ID,
                "CommonModules/A.bsl",
                hash.as_bytes(),
                &chunks,
                None,
            )
            .unwrap();

        let adapter = LocalStoreBaselineAdapter::workspace_code(&store);
        let service = BaselineOverlaySearchService::new(
            LocalStoreBaselineAdapter::workspace_code(&store),
            adapter,
            InMemoryResolvedViewResolver,
        );
        let baseline =
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "local-workspace-baseline");
        let mut overlay = SearchOverlay::new(baseline.clone());
        overlay.replace_file(
            DocumentPath::configuration("code", "CommonModules/A.bsl"),
            vec![crate::IndexedDocument {
                collection: "code".to_owned(),
                root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                path: "CommonModules/A.bsl".to_owned(),
                symbol_name: "Измененная".to_owned(),
                kind: "procedure".to_owned(),
                line_start: 1,
                line_end: 2,
                text: "новый текст".to_owned(),
                content_hash: "changed".to_owned(),
                graph_context: None,
            }],
        );

        let view = service.resolve_view(baseline, overlay).unwrap().unwrap();
        assert_eq!(view.documents().len(), 1);
        assert_eq!(view.documents()[0].symbol_name, "Измененная");
    }
}
