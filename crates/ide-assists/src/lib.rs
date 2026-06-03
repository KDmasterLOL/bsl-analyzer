use ide_db::TextRange;

#[derive(Debug, Clone)]
pub struct Assist {
    pub id: AssistId,
    pub label: String,
    pub group: Option<String>,
    pub source_change: SourceChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssistId(pub &'static str);

#[derive(Debug, Clone)]
pub struct SourceChange {
    pub edits: Vec<FileEdit>,
}

#[derive(Debug, Clone)]
pub struct FileEdit {
    pub file_id: vfs::FileId,
    pub edits: Vec<TextEdit>,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}
