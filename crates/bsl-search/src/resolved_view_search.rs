use crate::engine::SearchHit;
use crate::lexical::lexical_hits_for_documents;
use crate::resolver::ResolvedView;

pub fn lexical_hits(
    view: &ResolvedView,
    query: &str,
    limit: usize,
    collection: Option<&str>,
) -> Vec<SearchHit> {
    let documents = view.documents().iter().filter(|document| match collection {
        Some(collection) => document.collection == collection,
        None => true,
    });
    lexical_hits_for_documents(documents, query, limit)
}

#[cfg(test)]
mod tests {
    use super::lexical_hits;
    use crate::{BaselineRef, CorpusId, IndexedDocument, ResolvedView};

    #[test]
    fn lexical_search_filters_by_collection() {
        let view = ResolvedView::new(
            BaselineRef::for_snapshot(CorpusId::WorkspaceCode, "snapshot-1"),
            vec![
                IndexedDocument {
                    collection: "code".to_owned(),
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                    path: "A.bsl".to_owned(),
                    symbol_name: "Найти".to_owned(),
                    kind: "procedure".to_owned(),
                    line_start: 1,
                    line_end: 2,
                    text: "body".to_owned(),
                    content_hash: "a".to_owned(),
                    graph_context: None,
                },
                IndexedDocument {
                    collection: "platform".to_owned(),
                    root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
                    path: "platform://docs".to_owned(),
                    symbol_name: "Найти".to_owned(),
                    kind: "method".to_owned(),
                    line_start: 0,
                    line_end: 0,
                    text: "docs".to_owned(),
                    content_hash: "b".to_owned(),
                    graph_context: None,
                },
            ],
        );

        let hits = lexical_hits(&view, "Найти", 10, Some("code"));

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].collection, "code");
    }
}
