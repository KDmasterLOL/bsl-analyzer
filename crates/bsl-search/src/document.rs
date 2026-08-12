use crate::domain::IndexedDocument;
use crate::ports::GraphContextProvider;
use crate::workspace_roots::FileKey;
use code_chunk::{Chunk, ChunkKind};

#[derive(Debug, Clone)]
pub struct Document {
    pub title: String,
    pub body: String,
    pub kind: String,
}

/// The single embedding-text builder for an indexed code chunk — the one place a
/// chunk's text is composed for both lexical context and vector embedding, across
/// every path (local index, workspace overlay, central publish). It also keys
/// re-embedding (see [`semantic_key_for_indexed_document`]).
///
/// Layout: the BSL-native module path (richer than the raw file path for recall),
/// the kind and symbol, then the optional graph context (dispatch / signature /
/// calls / metadata reads — what the method *does*), then the source. Graph context
/// sits between the header and the body so the vector sees behaviour before syntax;
/// `render()` upstream already newline-terminates each line.
pub fn semantic_text_for_indexed_document(document: &IndexedDocument) -> String {
    semantic_text_from_parts(
        &document.path,
        &document.kind,
        &document.symbol_name,
        document.graph_context.as_deref().unwrap_or(""),
        &document.text,
    )
}

pub fn semantic_key_for_indexed_document(document: &IndexedDocument) -> String {
    blake3::hash(semantic_text_for_indexed_document(document).as_bytes()).to_hex().to_string()
}

/// The single source of truth for the embedding-text layout, expressed over a
/// chunk's raw parts. Both the indexing path (via
/// [`semantic_text_for_indexed_document`]) and the central serving-side key
/// recomputation read through this so the blake3 key matches on both ends — the
/// `rel_path` is folded into the BSL-native module path exactly once, here.
pub fn semantic_text_from_parts(
    rel_path: &str,
    kind: &str,
    symbol_name: &str,
    graph_context: &str,
    text: &str,
) -> String {
    let module_path = crate::context::file_path_to_module_path(rel_path);
    format!("Module: {module_path}\nKind: {kind}\nSymbol: {symbol_name}\n{graph_context}{text}")
}

pub fn semantic_key_from_parts(
    rel_path: &str,
    kind: &str,
    symbol_name: &str,
    graph_context: &str,
    text: &str,
) -> String {
    blake3::hash(
        semantic_text_from_parts(rel_path, kind, symbol_name, graph_context, text).as_bytes(),
    )
    .to_hex()
    .to_string()
}

/// Build the `IndexedDocument` for one code chunk — the single construction point
/// shared by the local index and the workspace overlay, so both carry identical
/// fields (and the same graph context). Graph context is requested from `provider`
/// only for method chunks (procedure / function); module headers and absent
/// providers yield `None`, leaving the embedding text unenriched.
pub(crate) fn indexed_document_for_chunk(
    key: &FileKey,
    chunk: &Chunk,
    provider: Option<&dyn GraphContextProvider>,
) -> IndexedDocument {
    let kind = chunk.kind.label();
    let graph_context = match chunk.kind {
        // The graph reads a module's identity out of the metadata-shaped path,
        // which is the path relative to its own root — an extension repeats that
        // shape, so the root-relative spelling is the one to hand over.
        ChunkKind::Procedure | ChunkKind::Function => {
            provider.and_then(|p| p.graph_context(&key.path, &chunk.name, kind))
        }
        ChunkKind::ModuleHeader => None,
    };
    IndexedDocument {
        collection: "code".to_owned(),
        root_id: key.root_id.clone(),
        path: key.path.clone(),
        symbol_name: chunk.name.clone(),
        kind: kind.to_owned(),
        line_start: chunk.line_start,
        line_end: chunk.line_end,
        content_hash: blake3::hash(chunk.text.as_bytes()).to_hex().to_string(),
        text: chunk.text.clone(),
        graph_context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> IndexedDocument {
        IndexedDocument {
            collection: "code".to_owned(),
            root_id: crate::CONFIGURATION_ROOT_ID.to_owned(),
            path: "A.bsl".to_owned(),
            symbol_name: "Найти".to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 2,
            text: "Возврат 1;".to_owned(),
            content_hash: "h".to_owned(),
            graph_context: None,
        }
    }

    #[test]
    fn graph_context_is_folded_between_header_and_body() {
        let plain = doc();
        let enriched = doc().with_graph_context("Dispatch: server | сервер\nCalls: Иная\n");
        assert!(!semantic_text_for_indexed_document(&plain).contains("Dispatch"));
        assert!(semantic_text_for_indexed_document(&enriched)
            .contains("Symbol: Найти\nDispatch: server | сервер\nCalls: Иная\nВозврат 1;"));
        // The embedding cache key tracks the enrichment, so adding/changing context
        // triggers a re-embed.
        assert_ne!(
            semantic_key_for_indexed_document(&plain),
            semantic_key_for_indexed_document(&enriched)
        );
    }

    /// The embedding key must stay deaf to the root, and the cost of breaking that is
    /// paid twice. Every embedding published so far would be orphaned — a full paid
    /// re-embed of the corpus — and the serving side, which recomputes the key from the
    /// columns of a row that has no root to give, would stop matching the publisher's key
    /// for good.
    ///
    /// The property is stated over the key of two documents rather than over "a second
    /// publish calls no embedder", because that phrasing cannot fail: both publishes
    /// compute the key with the same function, so the second hits the first's cache under
    /// ANY recipe, the forbidden one included.
    #[test]
    fn the_embedding_key_ignores_the_root() {
        let configuration = doc();
        let extension = IndexedDocument { root_id: "Расширение".to_owned(), ..doc() };

        assert_eq!(
            semantic_key_for_indexed_document(&configuration),
            semantic_key_for_indexed_document(&extension),
            "the same chunk under a different root embeds to the same vector, so it must \
             keep the same key"
        );
    }

    struct FakeProvider;
    impl GraphContextProvider for FakeProvider {
        fn graph_context(&self, _: &str, symbol_name: &str, _: &str) -> Option<String> {
            Some(format!("Dispatch: server | сервер\nCalls: {symbol_name}Вызов\n"))
        }
    }

    fn chunk(kind: ChunkKind, name: &str, text: &str) -> Chunk {
        Chunk {
            kind,
            name: name.to_owned(),
            is_export: true,
            annotations: Vec::new(),
            line_start: 0,
            line_end: 1,
            text: text.to_owned(),
        }
    }

    #[test]
    fn chunk_document_carries_provider_context_for_methods_only() {
        let path = FileKey::configuration("CommonModules/Сервер/Ext/Module.bsl");
        let provider = FakeProvider;

        // A method chunk gets the provider's context, folded into the embed text.
        let method = chunk(ChunkKind::Procedure, "Делать", "Процедура Делать() КонецПроцедуры");
        let doc = indexed_document_for_chunk(&path, &method, Some(&provider));
        assert!(doc.graph_context.as_deref().unwrap().contains("Dispatch: server"));
        let embed = semantic_text_for_indexed_document(&doc);
        assert!(embed.contains("Module: ОбщийМодуль.Сервер.Модуль"), "{embed}");
        assert!(embed.contains("Calls: ДелатьВызов"), "{embed}");

        // A module header never gets graph context, even with a provider.
        let header = indexed_document_for_chunk(
            &path,
            &chunk(ChunkKind::ModuleHeader, "", "Перем А;"),
            Some(&provider),
        );
        assert_eq!(header.graph_context, None);

        // No provider → no context.
        let plain = indexed_document_for_chunk(&path, &method, None);
        assert_eq!(plain.graph_context, None);
    }

    #[test]
    fn blank_graph_context_is_absent_and_does_not_perturb_text() {
        let blank = doc().with_graph_context("   \n");
        assert_eq!(blank.graph_context, None);
        assert_eq!(
            semantic_text_for_indexed_document(&blank),
            semantic_text_for_indexed_document(&doc())
        );
    }
}
