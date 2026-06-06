use crate::domain::IndexedDocument;
use crate::engine::SearchHit;

/// Split a raw user query into searchable terms: whitespace-separated tokens that contain at
/// least one alphanumeric character, with case-insensitive duplicates collapsed (first spelling
/// kept). Punctuation-only fragments are dropped so they cannot produce empty or syntactically
/// invalid backend queries; deduplication stops a repeated word from emitting redundant OR arms
/// or double-counting toward the in-memory breadth bonus. This is the shared tokenisation behind
/// every lexical backend (SQLite FTS5, Postgres `plainto_tsquery`, and the in-memory overlay
/// matcher) so a multi-term query behaves the same everywhere.
pub(crate) fn query_terms(query: &str) -> Vec<&str> {
    let mut seen = std::collections::HashSet::new();
    query
        .split_whitespace()
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .filter(|term| seen.insert(term.to_lowercase()))
        .collect()
}

/// Build an FTS5 `MATCH` expression that OR-joins each query term as its own quoted string token.
///
/// Quoting neutralises FTS5 operators and punctuation inside a term; OR-joining the terms — rather
/// than wrapping the whole query as one phrase — lets any single term surface a chunk while FTS5's
/// bm25 rank still lifts chunks that match more of the terms. Wrapping the entire query in one
/// quoted phrase (the previous behaviour) required an exact contiguous match, so any multi-word
/// query collapsed the lexical branch to zero hits. Returns `None` when the query has no usable
/// term.
pub(crate) fn fts5_match_query(query: &str) -> Option<String> {
    let expr = query_terms(query)
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    if expr.is_empty() {
        None
    } else {
        Some(expr)
    }
}

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
    let terms = query_terms(query);
    if terms.is_empty() {
        return None;
    }

    let symbol = document.symbol_name.to_lowercase();
    let text = document.text.to_lowercase();

    // A symbol whose name is exactly the whole query is the strongest possible lexical signal.
    let whole = query.trim().to_lowercase();
    if symbol == whole {
        return Some(1.0);
    }

    // OR semantics: any matched term surfaces the document. The strongest single-term match sets
    // the base score and each additional matched term adds a small breadth bonus, so a chunk that
    // matches more of the query ranks above one that matches fewer without letting a single exact
    // identifier match be buried by unrelated natural-language words.
    let mut best = 0.0f32;
    let mut matched = 0usize;
    for term in &terms {
        let term = term.to_lowercase();
        let term_score = if symbol.contains(&term) {
            0.95
        } else if text.contains(&term) {
            let occurrences = text.matches(&term).count().min(10) as f32;
            (0.70 + occurrences * 0.02).min(0.90)
        } else {
            0.0
        };
        if term_score > 0.0 {
            matched += 1;
            best = best.max(term_score);
        }
    }

    if matched == 0 {
        return None;
    }
    let breadth_bonus = 0.005 * (matched - 1) as f32;
    Some((best + breadth_bonus).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::{fts5_match_query, lexical_hits_for_documents, query_terms};
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
            graph_context: None,
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

    #[test]
    fn query_terms_splits_words_and_drops_punctuation_only_tokens() {
        assert_eq!(query_terms("ВызватьHTTPМетод отправка"), vec!["ВызватьHTTPМетод", "отправка"]);
        // A punctuation-only fragment carries no token and must be dropped.
        assert_eq!(query_terms("Найти ()"), vec!["Найти"]);
        assert!(query_terms("   ").is_empty());
        assert!(query_terms("--").is_empty());
        // Case-insensitive duplicates collapse to the first spelling.
        assert_eq!(query_terms("Найти найти НАЙТИ Прочее"), vec!["Найти", "Прочее"]);
    }

    #[test]
    fn fts5_match_query_keeps_dotted_identifier_as_one_quoted_token() {
        // A dotted call term stays a single quoted FTS5 token; unicode61 splits the `.`/`()` so it
        // becomes an adjacency phrase over its sub-tokens, which still matches the same dotted call
        // in code. It is intentionally NOT split into independent OR arms.
        assert_eq!(
            fts5_match_query("КоннекторHTTP.ВызватьМетод()"),
            Some("\"КоннекторHTTP.ВызватьМетод()\"".to_owned())
        );
    }

    #[test]
    fn fts5_match_query_or_joins_quoted_terms() {
        assert_eq!(
            fts5_match_query("ВызватьHTTPМетод отправка"),
            Some("\"ВызватьHTTPМетод\" OR \"отправка\"".to_owned())
        );
        // Internal double-quotes are doubled so they cannot escape the quoted token.
        assert_eq!(fts5_match_query("a\"b"), Some("\"a\"\"b\"".to_owned()));
        assert_eq!(fts5_match_query("   "), None);
    }

    #[test]
    fn multi_term_query_surfaces_any_term_and_breadth_wins() {
        // `both` contains both query terms; `one` contains only the identifier. With OR semantics
        // both surface (the old whole-phrase match found neither), and the broader match ranks
        // first.
        let documents = [
            doc("one.bsl", "Прочее", "вызов ВызватьHTTПМетод тут"),
            doc("both.bsl", "Прочее", "ВызватьHTTПМетод выполняет отправку запроса"),
        ];

        let hits = lexical_hits_for_documents(documents.iter(), "ВызватьHTTПМетод отправку", 10);

        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].file_path, "both.bsl");
        assert!(hits[0].score > hits[1].score);
    }
}
