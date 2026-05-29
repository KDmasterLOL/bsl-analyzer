use crate::domain::IndexedDocument;

#[derive(Debug, Clone)]
pub struct Document {
    pub title: String,
    pub body: String,
    pub kind: String,
}

pub fn semantic_text_for_indexed_document(document: &IndexedDocument) -> String {
    format!(
        "Path: {}\nKind: {}\nSymbol: {}\n{}",
        document.path, document.kind, document.symbol_name, document.text
    )
}

pub fn semantic_key_for_indexed_document(document: &IndexedDocument) -> String {
    blake3::hash(semantic_text_for_indexed_document(document).as_bytes()).to_hex().to_string()
}
