use crate::domain::IndexedDocument;
use crate::Document;

pub fn fingerprint_documents(documents: &[Document]) -> String {
    let mut entries: Vec<String> = documents
        .iter()
        .map(|document| format!("{}\n{}\n{}", document.kind, document.title, document.body))
        .collect();
    entries.sort();
    blake3::hash(entries.join("\n---\n").as_bytes()).to_hex().to_string()
}

pub fn fingerprint_indexed_documents(documents: &[IndexedDocument]) -> String {
    let mut entries: Vec<String> = documents
        .iter()
        .map(|document| {
            format!(
                "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
                document.collection,
                document.path,
                document.symbol_name,
                document.kind,
                document.line_start,
                document.line_end,
                document.content_hash,
                document.text
            )
        })
        .collect();
    entries.sort();
    blake3::hash(entries.join("\n---\n").as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::{fingerprint_documents, fingerprint_indexed_documents};
    use crate::{Document, IndexedDocument};

    #[test]
    fn document_fingerprint_is_order_independent() {
        let docs_a = vec![
            Document { title: "A".to_owned(), body: "1".to_owned(), kind: "type".to_owned() },
            Document { title: "B".to_owned(), body: "2".to_owned(), kind: "method".to_owned() },
        ];
        let docs_b = vec![docs_a[1].clone(), docs_a[0].clone()];

        assert_eq!(fingerprint_documents(&docs_a), fingerprint_documents(&docs_b));
    }

    #[test]
    fn indexed_document_fingerprint_changes_when_content_changes() {
        let docs_a = vec![IndexedDocument {
            collection: "platform".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: "platform://docs".to_owned(),
            symbol_name: "A".to_owned(),
            kind: "type".to_owned(),
            line_start: 0,
            line_end: 0,
            text: "body".to_owned(),
            content_hash: "hash-a".to_owned(),
            graph_context: None,
        }];
        let docs_b = vec![IndexedDocument { text: "changed".to_owned(), ..docs_a[0].clone() }];

        assert_ne!(fingerprint_indexed_documents(&docs_a), fingerprint_indexed_documents(&docs_b));
    }
}
