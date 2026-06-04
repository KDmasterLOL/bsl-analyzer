use syntax::ast::{Annotation, AstNode, FunctionDef, ProcedureDef};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    ModuleHeader,
    Procedure,
    Function,
}

impl ChunkKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ModuleHeader => "header",
            Self::Procedure => "procedure",
            Self::Function => "function",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub kind: ChunkKind,
    pub name: String,
    pub is_export: bool,
    pub annotations: Vec<String>,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
}

const MAX_CHUNK_BYTES: usize = 32 * 1024;

/// Suffix [`split_large_chunk`] appends to each part of an over-large method chunk.
const PART_SUFFIX: &str = " (часть ";

/// The declaration name a chunk belongs to, undoing the `" (часть N)"` suffix that
/// [`split_large_chunk`] appends when a method exceeds [`MAX_CHUNK_BYTES`]. For an
/// unsplit chunk this is the name unchanged; for a split part it is the shared base
/// name, so a consumer keying per method (e.g. attaching one graph context to every
/// part) groups the parts correctly.
pub fn base_chunk_name(name: &str) -> &str {
    match name.find(PART_SUFFIX) {
        Some(i) => &name[..i],
        None => name,
    }
}

pub struct Chunker;

impl Chunker {
    pub fn chunk(source: &str) -> Vec<Chunk> {
        let parse = parser::parse(source);
        Self::chunk_parsed(&parse.syntax_node(), source)
    }

    pub fn chunk_parsed(root: &syntax::SyntaxNode, source: &str) -> Vec<Chunk> {
        let line_index = LineIndex::new(source);

        let mut chunks = Vec::new();

        for node in root.descendants() {
            let (kind, name, is_export, annotations) =
                if let Some(proc) = ProcedureDef::cast(node.clone()) {
                    (
                        ChunkKind::Procedure,
                        extract_name_proc(&proc),
                        proc.export_keyword().is_some(),
                        extract_annotations_proc(&proc),
                    )
                } else if let Some(func) = FunctionDef::cast(node.clone()) {
                    (
                        ChunkKind::Function,
                        extract_name_func(&func),
                        func.export_keyword().is_some(),
                        extract_annotations_func(&func),
                    )
                } else {
                    continue;
                };

            let range = node.text_range();
            let start_byte = u32::from(range.start());
            let end_byte = u32::from(range.end());

            chunks.push(Chunk {
                kind,
                name,
                is_export,
                annotations,
                line_start: line_index.line_of(start_byte),
                line_end: line_index.line_of(end_byte.saturating_sub(1)) + 1,
                text: source[start_byte as usize..end_byte as usize].to_owned(),
            });
        }

        if chunks.is_empty() && has_meaningful_content(source) {
            chunks.push(Chunk {
                kind: ChunkKind::ModuleHeader,
                name: String::new(),
                is_export: false,
                annotations: Vec::new(),
                line_start: 0,
                line_end: line_index.line_of(source.len() as u32),
                text: source.trim_end().to_owned(),
            });
        }

        let mut result = Vec::new();
        for chunk in chunks {
            if chunk.text.len() <= MAX_CHUNK_BYTES {
                result.push(chunk);
            } else {
                result.extend(split_large_chunk(chunk));
            }
        }

        result
    }
}

fn split_large_chunk(chunk: Chunk) -> Vec<Chunk> {
    let lines: Vec<&str> = chunk.text.lines().collect();
    let mut parts = Vec::new();
    let mut current_lines = Vec::new();
    let mut current_bytes = 0usize;
    let mut part_line_start = chunk.line_start;

    for (i, line) in lines.iter().enumerate() {
        let line_bytes = line.len() + 1;
        if current_bytes + line_bytes > MAX_CHUNK_BYTES && !current_lines.is_empty() {
            let part_num = parts.len() + 1;
            parts.push(Chunk {
                kind: chunk.kind,
                name: if chunk.name.is_empty() {
                    String::new()
                } else {
                    format!("{}{}{})", chunk.name, PART_SUFFIX, part_num)
                },
                is_export: chunk.is_export,
                annotations: chunk.annotations.clone(),
                line_start: part_line_start,
                line_end: chunk.line_start + i as u32,
                text: current_lines.join("\n"),
            });
            current_lines.clear();
            current_bytes = 0;
            part_line_start = chunk.line_start + i as u32;
        }
        current_lines.push(*line);
        current_bytes += line_bytes;
    }

    if !current_lines.is_empty() {
        let part_num = parts.len() + 1;
        parts.push(Chunk {
            kind: chunk.kind,
            name: if chunk.name.is_empty() {
                String::new()
            } else {
                format!("{} (часть {})", chunk.name, part_num)
            },
            is_export: chunk.is_export,
            annotations: chunk.annotations.clone(),
            line_start: part_line_start,
            line_end: chunk.line_end,
            text: current_lines.join("\n"),
        });
    }

    parts
}

fn extract_name_proc(proc: &ProcedureDef) -> String {
    proc.name().map(|t| t.text().to_string()).unwrap_or_default()
}

fn extract_name_func(func: &FunctionDef) -> String {
    func.name().map(|t| t.text().to_string()).unwrap_or_default()
}

fn extract_annotations_proc(proc: &ProcedureDef) -> Vec<String> {
    proc.annotations().filter_map(|a| annotation_text(&a)).collect()
}

fn extract_annotations_func(func: &FunctionDef) -> Vec<String> {
    func.annotations().filter_map(|a| annotation_text(&a)).collect()
}

fn annotation_text(ann: &Annotation) -> Option<String> {
    ann.kind_token().map(|t| t.text().to_string())
}

fn has_meaningful_content(text: &str) -> bool {
    let tokens = lexer::tokenize(text);
    tokens.iter().any(|t| {
        !matches!(
            t.kind,
            lexer::TokenKind::Whitespace | lexer::TokenKind::Newline | lexer::TokenKind::Comment
        )
    })
}

struct LineIndex {
    line_starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i as u32 + 1);
            }
        }
        Self { line_starts }
    }

    fn line_of(&self, offset: u32) -> u32 {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line as u32,
            Err(line) => (line as u32).saturating_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source() {
        let chunks = Chunker::chunk("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn single_procedure() {
        let source = "Процедура Тест()\n    а = 1;\nКонецПроцедуры";
        let chunks = Chunker::chunk(source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Procedure);
        assert_eq!(chunks[0].name, "Тест");
        assert_eq!(chunks[0].line_start, 0);
        assert_eq!(chunks[0].line_end, 3);
    }

    #[test]
    fn procedure_and_function() {
        let source = "\
Процедура Первая() Экспорт
КонецПроцедуры

Функция Вторая()
    Возврат 42;
КонецФункции";
        let chunks = Chunker::chunk(source);
        assert_eq!(chunks.len(), 2);

        assert_eq!(chunks[0].kind, ChunkKind::Procedure);
        assert_eq!(chunks[0].name, "Первая");
        assert!(chunks[0].is_export);

        assert_eq!(chunks[1].kind, ChunkKind::Function);
        assert_eq!(chunks[1].name, "Вторая");
        assert!(!chunks[1].is_export);
    }

    #[test]
    fn module_header_skipped_when_procedures_exist() {
        let source = "\
Перем А;
Перем Б;

Процедура Тест()
КонецПроцедуры";
        let chunks = Chunker::chunk(source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::Procedure);
        assert_eq!(chunks[0].name, "Тест");
    }

    #[test]
    fn annotations_extracted() {
        let source = "\
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Функция КлиентскийМетод()
    Возврат 1;
КонецФункции";
        let chunks = Chunker::chunk(source);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].annotations, vec!["&НаСервере"]);
        assert_eq!(chunks[1].annotations, vec!["&НаКлиенте"]);
    }

    #[test]
    fn no_procedures_entire_file_as_header() {
        let source = "а = 1;\nб = 2;";
        let chunks = Chunker::chunk(source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].kind, ChunkKind::ModuleHeader);
    }

    #[test]
    fn whitespace_only_no_chunks() {
        let chunks = Chunker::chunk("   \n\n  \n");
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_parsed_matches_chunk() {
        let source = "\
Процедура Первая() Экспорт
КонецПроцедуры

Функция Вторая()
    Возврат 42;
КонецФункции";
        let parse = parser::parse(source);
        let via_parsed = Chunker::chunk_parsed(&parse.syntax_node(), source);
        let via_source = Chunker::chunk(source);
        assert_eq!(via_parsed.len(), via_source.len());
        for (a, b) in via_parsed.iter().zip(via_source.iter()) {
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.name, b.name);
            assert_eq!(a.is_export, b.is_export);
            assert_eq!(a.annotations, b.annotations);
            assert_eq!(a.line_start, b.line_start);
            assert_eq!(a.line_end, b.line_end);
            assert_eq!(a.text, b.text);
        }
    }
}
