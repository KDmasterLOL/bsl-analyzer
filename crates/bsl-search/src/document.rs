use crate::domain::IndexedDocument;
use crate::ports::GraphContextProvider;
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
    let module_path = crate::context::file_path_to_module_path(&document.path);
    let graph_context = document.graph_context.as_deref().unwrap_or("");
    format!(
        "Module: {}\nKind: {}\nSymbol: {}\n{}{}",
        module_path, document.kind, document.symbol_name, graph_context, document.text
    )
}

pub fn semantic_key_for_indexed_document(document: &IndexedDocument) -> String {
    blake3::hash(semantic_text_for_indexed_document(document).as_bytes()).to_hex().to_string()
}

/// Build the `IndexedDocument` for one code chunk — the single construction point
/// shared by the local index and the workspace overlay, so both carry identical
/// fields (and the same graph context). Graph context is requested from `provider`
/// only for method chunks (procedure / function); module headers and absent
/// providers yield `None`, leaving the embedding text unenriched.
pub(crate) fn indexed_document_for_chunk(
    rel_path: &str,
    chunk: &Chunk,
    provider: Option<&dyn GraphContextProvider>,
) -> IndexedDocument {
    let kind = chunk.kind.label();
    let graph_context = match chunk.kind {
        ChunkKind::Procedure | ChunkKind::Function => {
            provider.and_then(|p| p.graph_context(rel_path, &chunk.name, kind))
        }
        ChunkKind::ModuleHeader => None,
    };
    IndexedDocument {
        collection: "code".to_owned(),
        path: rel_path.to_owned(),
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
        let path = "CommonModules/Сервер/Ext/Module.bsl";
        let provider = FakeProvider;

        // A method chunk gets the provider's context, folded into the embed text.
        let method = chunk(ChunkKind::Procedure, "Делать", "Процедура Делать() КонецПроцедуры");
        let doc = indexed_document_for_chunk(path, &method, Some(&provider));
        assert!(doc.graph_context.as_deref().unwrap().contains("Dispatch: server"));
        let embed = semantic_text_for_indexed_document(&doc);
        assert!(embed.contains("Module: ОбщийМодуль.Сервер.Модуль"), "{embed}");
        assert!(embed.contains("Calls: ДелатьВызов"), "{embed}");

        // A module header never gets graph context, even with a provider.
        let header = indexed_document_for_chunk(
            path,
            &chunk(ChunkKind::ModuleHeader, "", "Перем А;"),
            Some(&provider),
        );
        assert_eq!(header.graph_context, None);

        // No provider → no context.
        let plain = indexed_document_for_chunk(path, &method, None);
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
