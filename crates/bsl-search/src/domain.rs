use std::fmt;

/// Logical search corpus.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CorpusId {
    WorkspaceCode,
    Reference,
    Custom(String),
}

impl CorpusId {
    pub fn custom(id: impl Into<String>) -> Self {
        Self::Custom(id.into())
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::WorkspaceCode => "workspace-code",
            Self::Reference => "reference",
            Self::Custom(id) => id.as_str(),
        }
    }
}

impl fmt::Display for CorpusId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Immutable snapshot identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

/// Selected baseline for building a resolved search view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineRef {
    pub corpus: CorpusId,
    pub snapshot_id: Option<SnapshotId>,
    pub branch: Option<String>,
    pub commit: Option<String>,
}

impl BaselineRef {
    pub fn for_snapshot(corpus: CorpusId, snapshot_id: impl Into<String>) -> Self {
        Self { corpus, snapshot_id: Some(SnapshotId::new(snapshot_id)), branch: None, commit: None }
    }
}

/// Immutable baseline snapshot metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub corpus: CorpusId,
}

impl Snapshot {
    pub fn new(id: impl Into<String>, corpus: CorpusId) -> Self {
        Self { id: SnapshotId::new(id), corpus }
    }
}

/// File-like identifier inside a corpus collection.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentPath {
    pub collection: String,
    pub path: String,
}

impl DocumentPath {
    pub fn new(collection: impl Into<String>, path: impl Into<String>) -> Self {
        Self { collection: collection.into(), path: path.into() }
    }
}

/// Searchable document resolved from a chunk or reference entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDocument {
    pub collection: String,
    pub path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub content_hash: String,
}

impl IndexedDocument {
    pub fn document_path(&self) -> DocumentPath {
        DocumentPath::new(self.collection.clone(), self.path.clone())
    }
}

/// Overlay for one logical file/document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOverlay {
    pub target: DocumentPath,
    pub items: Vec<IndexedDocument>,
}

impl FileOverlay {
    pub fn new(target: DocumentPath, items: Vec<IndexedDocument>) -> Self {
        Self { target, items }
    }
}

/// Local changes that modify baseline visibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayChange {
    ReplaceFile(FileOverlay),
    DeleteFile(DocumentPath),
}

/// Overlay built relative to a selected baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchOverlay {
    pub baseline: BaselineRef,
    pub changes: Vec<OverlayChange>,
}

impl SearchOverlay {
    pub fn new(baseline: BaselineRef) -> Self {
        Self { baseline, changes: Vec::new() }
    }

    pub fn replace_file(&mut self, target: DocumentPath, items: Vec<IndexedDocument>) {
        self.changes.push(OverlayChange::ReplaceFile(FileOverlay::new(target, items)));
    }

    pub fn delete_file(&mut self, target: DocumentPath) {
        self.changes.push(OverlayChange::DeleteFile(target));
    }
}
