use crate::domain::IndexedDocument;
use crate::engine::SearchHit;

pub(crate) fn lexical_hits_for_documents<'a>(
    documents: impl Iterator<Item = &'a IndexedDocument>,
    query: &str,
    limit: usize,
) -> Vec<SearchHit> {
    let mut hits: Vec<SearchHit> = documents
        .filter_map(|document| lexical_score(document, query).map(|score| (document, score)))
        .map(|(document, score)| SearchHit {
            collection: document.collection.clone(),
            file_path: document.path.clone(),
            symbol_name: document.symbol_name.clone(),
            kind: document.kind.clone(),
            text: document.text.clone(),
            line_start: document.line_start,
            line_end: document.line_end,
            score,
        })
        .collect();

    hits.sort_by(|lhs, rhs| rhs.score.total_cmp(&lhs.score));
    hits.truncate(limit);
    hits
}

pub(crate) fn lexical_score(document: &IndexedDocument, query: &str) -> Option<f32> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }

    let symbol = document.symbol_name.to_lowercase();
    let text = document.text.to_lowercase();

    if symbol == needle {
        return Some(1.0);
    }
    if symbol.contains(&needle) {
        return Some(0.95);
    }
    if text.contains(&needle) {
        let occurrences = text.matches(&needle).count().min(10) as f32;
        return Some((0.70 + occurrences * 0.02).min(0.90));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::lexical_hits_for_documents;
    use crate::IndexedDocument;

    fn doc(path: &str, symbol_name: &str, text: &str) -> IndexedDocument {
        IndexedDocument {
            collection: "code".to_owned(),
            path: path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 2,
            text: text.to_owned(),
            content_hash: format!("hash-{symbol_name}"),
        }
    }

    #[test]
    fn exact_symbol_match_ranks_above_body_match() {
        let documents = [
            doc("A.bsl", "Найти", "Текст без совпадения"),
            doc("B.bsl", "Другая", "Вызов Найти в теле"),
        ];

        let hits = lexical_hits_for_documents(documents.iter(), "Найти", 10);

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].symbol_name, "Найти");
        assert!(hits[0].score > hits[1].score);
    }
}
