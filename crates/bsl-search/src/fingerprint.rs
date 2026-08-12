use crate::domain::IndexedDocument;
use crate::Document;

/// One document of a corpus, rendered so that its fields cannot trade characters across
/// their boundary.
///
/// Length-prefixed rather than separated by a character, because no character is unavailable
/// to the fields: a path may hold a newline, source text may hold anything at all. A
/// separator that can occur inside a field is not a boundary — a root ending in one and a
/// path beginning after it concatenate to exactly what a shorter root and a longer path
/// produce, and two different documents then share a fingerprint. A length says where a
/// field ends without forbidding anything inside it.
fn entry_of(fields: &[&str]) -> String {
    fields.iter().map(|field| format!("{}:{field}", field.len())).collect()
}

/// The identity of the reference corpus.
///
/// Deliberately NOT rendered through [`entry_of`], though its fields are separated the way
/// [`fingerprint_indexed_documents`] used to be. Both producers of these documents build them
/// from platform data: `kind` is one of three literals and `title` is `"{name} / {english}"`
/// of a platform type or method, so moving a field boundary would take a newline inside a
/// type name. Nothing can put one there.
///
/// The cost of changing it, by contrast, is real and lasting. This corpus has no manifest —
/// its fingerprint feeds exactly one thing, the freshness line of `search status`, which
/// compares the published value against this function recomputed locally. Any change marks a
/// reference corpus that is in fact current as stale on every run until someone republishes
/// it. Revisit only together with that republish, or if a caller ever feeds this arbitrary
/// documents.
pub fn fingerprint_documents(documents: &[Document]) -> String {
    let mut entries: Vec<String> = documents
        .iter()
        .map(|document| format!("{}\n{}\n{}", document.kind, document.title, document.body))
        .collect();
    entries.sort();
    blake3::hash(entries.join("\n---\n").as_bytes()).to_hex().to_string()
}

/// The identity of a whole corpus: has anything about it moved since the last publish?
///
/// The root is part of it because identity of a file is the pair `(root_id, path)` — the
/// same relative path lives under the configuration and under every extension at once.
/// This is deliberately not what a FILE fingerprint answers: that one describes CONTENT,
/// so two identical files in two roots legitimately share one file object.
pub fn fingerprint_indexed_documents(documents: &[IndexedDocument]) -> String {
    let mut entries: Vec<String> = documents
        .iter()
        .map(|document| {
            let line_start = document.line_start.to_string();
            let line_end = document.line_end.to_string();
            entry_of(&[
                document.collection.as_str(),
                document.root_id.as_str(),
                document.path.as_str(),
                document.symbol_name.as_str(),
                document.kind.as_str(),
                line_start.as_str(),
                line_end.as_str(),
                document.content_hash.as_str(),
                document.text.as_str(),
            ])
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

    /// The workspace corpus is assembled by a tree walk, and a walk's order is a property of
    /// the walk, not of the tree: change how the tree is traversed and the same files arrive in
    /// a different sequence. If that reordering moved the snapshot fingerprint, every consumer
    /// would see a snapshot it already holds as a new one and re-fetch the whole corpus.
    ///
    /// Its sibling above pins the same property for the reference corpus, which is built from
    /// platform data rather than from a walk — a different function with its own sort.
    #[test]
    fn indexed_document_fingerprint_is_order_independent() {
        let one = IndexedDocument {
            collection: "code".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: "CommonModules/Первый/Ext/Module.bsl".to_owned(),
            symbol_name: "Первый".to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 3,
            text: "Процедура Первый()".to_owned(),
            content_hash: "hash-1".to_owned(),
            graph_context: None,
        };
        let two = IndexedDocument {
            path: "CommonModules/Второй/Ext/Module.bsl".to_owned(),
            symbol_name: "Второй".to_owned(),
            content_hash: "hash-2".to_owned(),
            ..one.clone()
        };
        let forwards = vec![one.clone(), two.clone()];
        let backwards = vec![two, one];

        assert_eq!(
            fingerprint_indexed_documents(&forwards),
            fingerprint_indexed_documents(&backwards),
            "the same corpus in a different order is the same corpus"
        );
    }

    /// The snapshot fingerprint answers one question — is this the same corpus? — and two
    /// files with the same relative path under two different roots are two different files.
    /// A republish that moved a file's root attribution while its path and bytes stayed put
    /// would otherwise leave the fingerprint where it was, and every consumer holding a
    /// manifest of the old keys would accept it as current.
    #[test]
    fn indexed_document_fingerprint_changes_when_the_root_changes() {
        let docs_a = vec![IndexedDocument {
            collection: "code".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: "CommonModules/Общий/Ext/Module.bsl".to_owned(),
            symbol_name: "Общий".to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 3,
            text: "Процедура Общий()".to_owned(),
            content_hash: "hash-1".to_owned(),
            graph_context: None,
        }];
        let docs_b = vec![IndexedDocument {
            root_id: "Расширение".to_owned(),
            ..docs_a[0].clone()
        }];

        assert_ne!(
            fingerprint_indexed_documents(&docs_a),
            fingerprint_indexed_documents(&docs_b),
            "the same path under a different root is a different file, so it is a different corpus"
        );
    }

    /// A newline is legal in a Unix directory name, so a root ending in one and a path
    /// beginning after it concatenate to exactly what a shorter root and a longer path
    /// produce — and those are two different files.
    #[test]
    fn no_two_field_layouts_share_an_indexed_document_fingerprint() {
        let base = IndexedDocument {
            collection: "code".to_owned(),
            root_id: "src/cfe/a".to_owned(),
            path: "b\nc/Module.bsl".to_owned(),
            symbol_name: "Общий".to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 3,
            text: "Процедура Общий()".to_owned(),
            content_hash: "hash-1".to_owned(),
            graph_context: None,
        };
        let boundary_moved = IndexedDocument {
            root_id: "src/cfe/a\nb".to_owned(),
            path: "c/Module.bsl".to_owned(),
            ..base.clone()
        };

        assert_ne!(
            fingerprint_indexed_documents(std::slice::from_ref(&base)),
            fingerprint_indexed_documents(std::slice::from_ref(&boundary_moved)),
            "two different file keys must not hash alike"
        );
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
