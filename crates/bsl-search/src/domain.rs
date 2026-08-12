use std::fmt;

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

    pub fn from_storage(id: impl Into<String>) -> Self {
        let id = id.into();
        match id.as_str() {
            "workspace-code" => Self::WorkspaceCode,
            "reference" => Self::Reference,
            _ => Self::Custom(id),
        }
    }
}

impl fmt::Display for CorpusId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineSourceConfig {
    Local,
    External(ExternalBaselineConfig),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalBaselineBackend {
    Postgres,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBaselineConfig {
    pub backend: ExternalBaselineBackend,
    pub connection: String,
    pub schema: Option<String>,
}

impl ExternalBaselineConfig {
    pub fn postgres(connection: impl Into<String>) -> Self {
        Self {
            backend: ExternalBaselineBackend::Postgres,
            connection: connection.into(),
            schema: None,
        }
    }

    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub corpus: CorpusId,
    pub fingerprint: Option<String>,
    pub parent_id: Option<SnapshotId>,
}

impl Snapshot {
    pub fn new(id: impl Into<String>, corpus: CorpusId) -> Self {
        Self { id: SnapshotId::new(id), corpus, fingerprint: None, parent_id: None }
    }

    pub fn with_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.fingerprint = Some(fingerprint.into());
        self
    }

    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(SnapshotId::new(parent_id));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotPublishMetadata {
    pub branch: Option<String>,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SnapshotPublishStats {
    pub reused_files: usize,
    pub written_files: usize,
    pub deleted_files: usize,
    pub reused_documents: usize,
    pub written_documents: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocumentPath {
    pub collection: String,
    /// The source root the file belongs to; [`crate::CONFIGURATION_ROOT_ID`] for
    /// the configuration. `path` alone does not identify a file — an extension
    /// repeats the configuration's layout verbatim.
    pub root_id: String,
    pub path: String,
}

impl DocumentPath {
    pub fn new(
        collection: impl Into<String>,
        root_id: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self { collection: collection.into(), root_id: root_id.into(), path: path.into() }
    }

    /// A file of the configuration root.
    pub fn configuration(collection: impl Into<String>, path: impl Into<String>) -> Self {
        Self::new(collection, crate::CONFIGURATION_ROOT_ID, path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedDocument {
    pub collection: String,
    /// The source root this document's file belongs to; see
    /// [`DocumentPath::root_id`].
    pub root_id: String,
    pub path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub content_hash: String,
    /// Optional pre-rendered graph context (signature, dispatch, calls, metadata
    /// reads) prepended to the embedding text. Opaque to this crate — produced
    /// upstream by a layer that has the call graph, so `bsl-search` stays graph
    /// agnostic. `None` for documents indexed without graph enrichment (the docs
    /// collection, plain re-index). Folding it into the embedded text (and thus the
    /// content hash that keys re-embedding) is the whole point of GE.
    pub graph_context: Option<String>,
}

impl IndexedDocument {
    pub fn document_path(&self) -> DocumentPath {
        DocumentPath::new(self.collection.clone(), self.root_id.clone(), self.path.clone())
    }

    /// Attach pre-rendered graph context. A blank/whitespace-only string is treated
    /// as absent so it never perturbs the embedding text or its hash.
    pub fn with_graph_context(mut self, context: impl Into<String>) -> Self {
        let context = context.into();
        self.graph_context = (!context.trim().is_empty()).then_some(context);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LexicalHit {
    pub collection: String,
    pub root_id: String,
    pub path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub rank: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticHit {
    pub collection: String,
    pub root_id: String,
    pub path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub score: f32,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayChange {
    ReplaceFile(FileOverlay),
    DeleteFile(DocumentPath),
}

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
