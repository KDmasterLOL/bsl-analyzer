use crate::domain::BaselineRef;
use crate::error::SearchError;
use crate::ports::{ResolvedViewService, SnapshotCatalog, SnapshotContentStore};
use crate::resolver::ResolvedView;
use crate::SearchOverlay;

pub struct BaselineOverlaySearchService<C, S, R> {
    catalog: C,
    content_store: S,
    resolver: R,
}

impl<C, S, R> BaselineOverlaySearchService<C, S, R>
where
    C: SnapshotCatalog,
    S: SnapshotContentStore,
    R: ResolvedViewService,
{
    pub fn new(catalog: C, content_store: S, resolver: R) -> Self {
        Self { catalog, content_store, resolver }
    }

    pub fn resolve_view(
        &self,
        baseline: BaselineRef,
        overlay: SearchOverlay,
    ) -> Result<Option<ResolvedView>, SearchError> {
        let Some(snapshot) = self.catalog.resolve_baseline(&baseline)? else {
            return Ok(None);
        };
        let documents = self.content_store.load_snapshot_documents(&snapshot)?;
        let view = self.resolver.resolve_view(baseline, documents, overlay)?;
        Ok(Some(view))
    }
}

#[cfg(test)]
mod tests {
    use super::BaselineOverlaySearchService;
    use crate::domain::{
        BaselineRef, CorpusId, DocumentPath, IndexedDocument, SearchOverlay, Snapshot,
    };
    use crate::ports::{SnapshotCatalog, SnapshotContentStore};
    use crate::{InMemoryResolvedViewResolver, SearchError};
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestCatalog {
        snapshots: HashMap<String, Snapshot>,
    }

    impl SnapshotCatalog for TestCatalog {
        fn resolve_baseline(
            &self,
            baseline: &BaselineRef,
        ) -> Result<Option<Snapshot>, SearchError> {
            let id = baseline.snapshot_id.as_ref().map(|id| id.0.as_str()).unwrap_or_default();
            Ok(self.snapshots.get(id).cloned())
        }
    }

    #[derive(Default)]
    struct TestContentStore {
        documents: HashMap<String, Vec<IndexedDocument>>,
    }

    impl SnapshotContentStore for TestContentStore {
        fn load_snapshot_documents(
            &self,
            snapshot: &Snapshot,
        ) -> Result<Vec<IndexedDocument>, SearchError> {
            Ok(self.documents.get(&snapshot.id.0).cloned().unwrap_or_default())
        }
    }

    fn doc(symbol_name: &str, path: &str, text: &str, content_hash: &str) -> IndexedDocument {
        IndexedDocument {
            collection: "code".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 2,
            text: text.to_owned(),
            content_hash: content_hash.to_owned(),
            graph_context: None,
        }
    }

    #[test]
    fn resolves_snapshot_documents_and_applies_overlay() {
        let baseline = BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snapshot-1");
        let snapshot = Snapshot::new("snapshot-1", CorpusId::WorkspaceCode);

        let mut catalog = TestCatalog::default();
        catalog.snapshots.insert(snapshot.id.0.clone(), snapshot.clone());

        let mut content_store = TestContentStore::default();
        content_store.documents.insert(
            snapshot.id.0.clone(),
            vec![doc("OldName", "CommonModules/A.bsl", "old body", "old-hash")],
        );

        let service =
            BaselineOverlaySearchService::new(catalog, content_store, InMemoryResolvedViewResolver);
        let mut overlay = SearchOverlay::new(baseline.clone());
        overlay.replace_file(
            DocumentPath::configuration("code", "CommonModules/A.bsl"),
            vec![doc("NewName", "CommonModules/A.bsl", "new body", "new-hash")],
        );

        let view = service.resolve_view(baseline, overlay).unwrap().unwrap();

        assert_eq!(view.documents().len(), 1);
        assert_eq!(view.documents()[0].symbol_name, "NewName");
    }

    #[test]
    fn returns_none_when_baseline_is_unknown() {
        let baseline = BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "missing");
        let service = BaselineOverlaySearchService::new(
            TestCatalog::default(),
            TestContentStore::default(),
            InMemoryResolvedViewResolver,
        );

        assert!(service
            .resolve_view(baseline.clone(), SearchOverlay::new(baseline))
            .unwrap()
            .is_none());
    }
}
