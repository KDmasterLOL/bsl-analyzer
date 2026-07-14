use std::hash::BuildHasherDefault;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHasher;
use vfs::{FileId, VfsPath};

mod change;
mod input;
mod locale;
mod queries;

pub use change::FileChange;
pub use input::{
    content_revision, DiagnosticsConfigId, DiagnosticsConfigInput, FileIdInput, FileRevisionInput,
    FileSourceRootInput, FileTextInput, SourceRoot, SourceRootId, SourceRootInput, BSL_SOURCE_ROOT,
    METADATA_SOURCE_ROOT,
};
pub use locale::{Locale, UnknownLocale};
pub use queries::{
    decode_disk_bytes, file_text_query, method_regions_query, parse_query, read_disk_text,
    resolve_vfs_path_query, set_parse_lru_sweep_mode,
};

#[salsa::db]
pub trait SourceDatabase: salsa::Database {
    fn file_text_input(&self, file_id: FileId) -> FileTextInput;

    fn try_file_text_input(&self, file_id: FileId) -> Option<FileTextInput>;

    fn file_revision_input(&self, file_id: FileId) -> FileRevisionInput;

    fn try_file_revision_input(&self, file_id: FileId) -> Option<FileRevisionInput>;

    fn source_root_input(&self, source_root_id: SourceRootId) -> SourceRootInput;

    fn file_source_root_input(&self, file_id: FileId) -> FileSourceRootInput;

    fn set_file_text(&mut self, file_id: FileId, text: &str);

    /// Register a file's content revision without storing its text; the text is
    /// read from disk on demand by [`file_text`](Self::file_text). See
    /// [`Files::set_file_revision_from_disk`].
    fn set_file_revision_from_disk(&mut self, file_id: FileId, revision: u64);

    /// The file's source text, as a version-keyed tracked query: returns the
    /// in-memory overlay when present, otherwise reads disk and verifies the
    /// bytes against the file's content revision. LRU-evictable.
    fn file_text(&self, file_id: FileId) -> Arc<str>;

    /// Borrowed variant of [`file_text`](Self::file_text) for read-only paths:
    /// no `Arc` refcount traffic per read.
    fn file_text_ref(&self, file_id: FileId) -> &Arc<str>;

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId);

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot);

    fn resolve_vfs_path(&self, source_root_id: SourceRootId, vfs_path: &VfsPath) -> Option<FileId>;
}

#[salsa::db]
pub trait RootQueryDb: SourceDatabase {
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode>;

    /// Borrow the parsed tree directly from the memo, skipping the `Vec<SyntaxError>`
    /// deep clone that [`parse`](Self::parse) pays on every read. Prefer this on hot
    /// per-file paths that only read the syntax node or errors; [`parse`](Self::parse)
    /// stays for callers that must own the `Parse`. The borrow is tied to `&self`, and
    /// Salsa only evicts a memo under `&mut`, so it cannot dangle.
    fn parse_ref(&self, file_id: FileId) -> &syntax::Parse<syntax::SyntaxNode>;

    fn method_regions(
        &self,
        file_id: FileId,
    ) -> Arc<std::collections::HashMap<syntax::TextRange, String>>;
}

#[derive(Debug, Default, Clone)]
pub struct Files {
    file_texts: Arc<DashMap<FileId, FileTextInput, BuildHasherDefault<FxHasher>>>,
    file_revisions: Arc<DashMap<FileId, FileRevisionInput, BuildHasherDefault<FxHasher>>>,
    source_roots: Arc<DashMap<SourceRootId, SourceRootInput, BuildHasherDefault<FxHasher>>>,
    file_source_roots: Arc<DashMap<FileId, FileSourceRootInput, BuildHasherDefault<FxHasher>>>,
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn file_text(&self, file_id: FileId) -> FileTextInput {
        self.file_texts.get(&file_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?file_id, "file text not set — this is a programming error, all files must be loaded before queries run");
            panic!("file text not set for {:?}", file_id)
        })
    }

    pub fn try_file_text(&self, file_id: FileId) -> Option<FileTextInput> {
        self.file_texts.get(&file_id).map(|entry| *entry.value())
    }

    pub fn set_file_text(&self, db: &mut dyn SourceDatabase, file_id: FileId, text: &str) {
        use salsa::Setter;

        let existing = self.file_texts.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_text(db).to(text.to_string());
            }
            None => {
                let input = FileTextInput::new(db, text.to_string());
                let previous = self.file_texts.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_text violates single-mutator invariant"
                );
            }
        }
        // Set the revision in the SAME exclusive `&mut db` op so a snapshot never
        // observes overlay-and-revision out of step. The revision is the
        // invalidation trigger for `file_text_query` and the token a later disk
        // re-read (when this file is closed) must match.
        self.set_file_revision(db, file_id, input::content_revision(text));
    }

    pub fn set_file_text_with_durability(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        text: &str,
        durability: salsa::Durability,
    ) {
        use salsa::Setter;

        let existing = self.file_texts.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_text(db).with_durability(durability).to(text.to_string());
            }
            None => {
                let input = FileTextInput::builder(text.to_string()).durability(durability).new(db);
                let previous = self.file_texts.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_text_with_durability violates single-mutator invariant"
                );
            }
        }
        self.set_file_revision_with_durability(
            db,
            file_id,
            input::content_revision(text),
            durability,
        );
    }

    /// The content-revision input handle for a file (panics if neither
    /// [`set_file_text`](Self::set_file_text) nor
    /// [`set_file_revision_from_disk`](Self::set_file_revision_from_disk) ran for it).
    pub fn file_revision(&self, file_id: FileId) -> FileRevisionInput {
        self.file_revisions.get(&file_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?file_id, "file revision not set — this is a programming error, all files must be registered before queries run");
            panic!("file revision not set for {:?}", file_id)
        })
    }

    pub fn try_file_revision(&self, file_id: FileId) -> Option<FileRevisionInput> {
        self.file_revisions.get(&file_id).map(|entry| *entry.value())
    }

    /// Register a file's content revision WITHOUT storing its text (the
    /// disk-backed path): `file_text_query` will read the file from disk on
    /// demand and verify the bytes hash to this revision. Used by batch analysis
    /// and for closed LSP files to keep them evictable instead of resident.
    ///
    /// Drops any existing in-memory overlay for the file in the SAME exclusive
    /// update so `file_text_query` (which prefers the overlay) actually falls
    /// through to the disk read. Without this, a once-open file's stale overlay
    /// would be hash-checked against the new disk revision and panic.
    pub fn set_file_revision_from_disk(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        revision: u64,
    ) {
        self.file_texts.remove(&file_id);
        let durability = self.durability_for_file(db, file_id).unwrap_or(salsa::Durability::LOW);
        self.set_file_revision_with_durability(db, file_id, revision, durability);
    }

    fn set_file_revision(&self, db: &mut dyn SourceDatabase, file_id: FileId, revision: u64) {
        self.set_file_revision_with_durability(db, file_id, revision, salsa::Durability::LOW);
    }

    /// The durability of a file's inputs, decided by its source root (see
    /// [`SourceRoot::durability`]); `None` when the file has no root mapping yet.
    fn durability_for_file(
        &self,
        db: &dyn SourceDatabase,
        file_id: FileId,
    ) -> Option<salsa::Durability> {
        let mapping = self.file_source_roots.get(&file_id).map(|e| *e.value())?;
        let source_root_id = mapping.source_root_id(db);
        let root_input = self.source_roots.get(&source_root_id).map(|e| *e.value())?;
        Some(root_input.root(db).durability())
    }

    fn set_file_revision_with_durability(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        revision: u64,
        durability: salsa::Durability,
    ) {
        use salsa::Setter;

        let existing = self.file_revisions.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_revision(db).with_durability(durability).to(revision);
            }
            None => {
                let input = FileRevisionInput::builder(revision).durability(durability).new(db);
                let previous = self.file_revisions.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_revision violates single-mutator invariant"
                );
            }
        }
    }

    pub fn set_file_text_smart(&self, db: &mut dyn SourceDatabase, file_id: FileId, text: &str) {
        let durability = self.durability_for_file(db, file_id);

        match durability {
            Some(d) => {
                tracing::debug!(
                    ?file_id,
                    durability = ?d,
                    "set_file_text_smart: determined durability from source root"
                );
                self.set_file_text_with_durability(db, file_id, text, d);
            }
            None => {
                tracing::debug!(
                    ?file_id,
                    "set_file_text_smart: fallback to LOW durability (source root not set)"
                );
                self.set_file_text_with_durability(db, file_id, text, salsa::Durability::LOW);
            }
        }
    }

    pub fn source_root(&self, source_root_id: SourceRootId) -> SourceRootInput {
        self.source_roots.get(&source_root_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?source_root_id, "source root not set — this is a programming error");
            panic!("source root not set for {:?}", source_root_id)
        })
    }

    /// The `SourceRootInput` carries the root's own durability: `file_text_query`'s
    /// disk branch reads it (path resolution), so a LOW root input would floor every
    /// disk-backed text memo under the root at LOW regardless of the file inputs.
    pub fn set_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        source_root_id: SourceRootId,
        source_root: SourceRoot,
    ) {
        use salsa::Setter;

        let durability = source_root.durability();
        let existing = self.source_roots.get(&source_root_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_root(db).with_durability(durability).to(source_root);
            }
            None => {
                let input = SourceRootInput::builder(source_root).durability(durability).new(db);
                let previous = self.source_roots.insert(source_root_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_source_root violates single-mutator invariant"
                );
            }
        }
    }

    pub fn file_source_root(&self, file_id: FileId) -> FileSourceRootInput {
        self.file_source_roots.get(&file_id).map(|entry| *entry.value()).unwrap_or_else(|| {
            tracing::error!(?file_id, "file source root not set — this is a programming error");
            panic!("file source root not set for {:?}", file_id)
        })
    }

    /// The file→root mapping inherits the target root's durability (the root input
    /// must already be registered; an unknown root falls back to LOW). Same reason
    /// as [`Self::set_source_root`]: `file_text_query`'s disk branch reads the
    /// mapping, so it must not undercut the root's durability floor.
    pub fn set_file_source_root(
        &self,
        db: &mut dyn SourceDatabase,
        file_id: FileId,
        source_root_id: SourceRootId,
    ) {
        use salsa::Setter;

        let durability = self
            .source_roots
            .get(&source_root_id)
            .map(|e| *e.value())
            .map(|input| input.root(db).durability())
            .unwrap_or(salsa::Durability::LOW);
        let existing = self.file_source_roots.get(&file_id).map(|e| *e.value());
        match existing {
            Some(input) => {
                input.set_source_root_id(db).with_durability(durability).to(source_root_id);
            }
            None => {
                let input =
                    FileSourceRootInput::builder(source_root_id).durability(durability).new(db);
                let previous = self.file_source_roots.insert(file_id, input);
                debug_assert!(
                    previous.is_none(),
                    "concurrent set_file_source_root violates single-mutator invariant"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vfs::file_set::FileSet;
    use vfs::VfsPath;

    #[salsa::db]
    #[derive(Clone)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
        files: Files,
        events: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Default for TestDatabase {
        fn default() -> Self {
            let events: std::sync::Arc<std::sync::Mutex<Vec<String>>> = Default::default();
            Self {
                storage: salsa::Storage::new(Some(Box::new({
                    let events = events.clone();
                    move |event| events.lock().unwrap().push(format!("{:?}", event.kind))
                }))),
                files: Files::default(),
                events,
            }
        }
    }

    impl TestDatabase {
        fn take_events(&self) -> Vec<String> {
            std::mem::take(&mut self.events.lock().unwrap())
        }
    }

    #[salsa::db]
    impl salsa::Database for TestDatabase {}

    #[salsa::db]
    impl SourceDatabase for TestDatabase {
        fn file_text_input(&self, file_id: FileId) -> FileTextInput {
            self.files.file_text(file_id)
        }

        fn try_file_text_input(&self, file_id: FileId) -> Option<FileTextInput> {
            self.files.try_file_text(file_id)
        }

        fn file_revision_input(&self, file_id: FileId) -> FileRevisionInput {
            self.files.file_revision(file_id)
        }

        fn try_file_revision_input(&self, file_id: FileId) -> Option<FileRevisionInput> {
            self.files.try_file_revision(file_id)
        }

        fn file_text(&self, file_id: FileId) -> Arc<str> {
            self.file_text_ref(file_id).clone()
        }

        fn file_text_ref(&self, file_id: FileId) -> &Arc<str> {
            let input = FileIdInput::new(self, file_id);
            file_text_query(self, input)
        }

        fn set_file_revision_from_disk(&mut self, file_id: FileId, revision: u64) {
            let files = self.files.clone();
            files.set_file_revision_from_disk(self, file_id, revision);
        }

        fn source_root_input(&self, source_root_id: SourceRootId) -> SourceRootInput {
            self.files.source_root(source_root_id)
        }

        fn file_source_root_input(&self, file_id: FileId) -> FileSourceRootInput {
            self.files.file_source_root(file_id)
        }

        fn set_file_text(&mut self, file_id: FileId, text: &str) {
            let files = self.files.clone();
            files.set_file_text(self, file_id, text);
        }

        fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
            let files = self.files.clone();
            files.set_file_source_root(self, file_id, source_root_id);
        }

        fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot) {
            let files = self.files.clone();
            files.set_source_root(self, source_root_id, source_root);
        }

        fn resolve_vfs_path(
            &self,
            source_root_id: SourceRootId,
            vfs_path: &VfsPath,
        ) -> Option<FileId> {
            let source_root_input = self.source_root_input(source_root_id);
            let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
            resolve_vfs_path_query(self, source_root_input, vfs_path_str)
        }
    }

    #[salsa::db]
    impl RootQueryDb for TestDatabase {
        fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
            self.parse_ref(file_id).clone()
        }

        fn parse_ref(&self, file_id: FileId) -> &syntax::Parse<syntax::SyntaxNode> {
            let input = FileIdInput::new(self, file_id);
            parse_query(self, input)
        }

        fn method_regions(
            &self,
            file_id: FileId,
        ) -> Arc<std::collections::HashMap<syntax::TextRange, String>> {
            let input = FileIdInput::new(self, file_id);
            method_regions_query(self, input)
        }
    }

    #[salsa::tracked(lru = 10, returns(copy))]
    fn test_fileid_query<'db>(
        db: &'db dyn salsa::Database,
        file_id_input: FileIdInput<'db>,
    ) -> u32 {
        file_id_input.file_id(db).0
    }

    #[test]
    fn test_fileid_salsa_compatible() {
        let db = TestDatabase::default();

        let file_id = FileId(42);
        let file_id_input = FileIdInput::new(&db, file_id);

        let result = test_fileid_query(&db, file_id_input);
        assert_eq!(result, 42);

        let result2 = test_fileid_query(&db, file_id_input);
        assert_eq!(result2, 42);

        let file_id2 = FileId(100);
        let file_id_input2 = FileIdInput::new(&db, file_id2);
        let result3 = test_fileid_query(&db, file_id_input2);
        assert_eq!(result3, 100);

        let file_id_input3 = FileIdInput::new(&db, file_id);
        assert_eq!(file_id_input, file_id_input3);
    }

    #[test]
    fn test_parse_query() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");

        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }

    #[test]
    fn test_incremental_reparse() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        let parse1 = db.parse(file_id);
        assert!(!parse1.has_errors());

        let parse2 = db.parse(file_id);
        assert!(!parse2.has_errors());
        assert_eq!(parse1.syntax_node().text(), parse2.syntax_node().text());

        db.set_file_text(file_id, "Процедура Тест2() КонецПроцедуры");
        let parse3 = db.parse(file_id);
        assert!(!parse3.has_errors());
        assert_ne!(parse1.syntax_node().text(), parse3.syntax_node().text());
    }

    #[test]
    fn test_file_change_apply() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut change = FileChange::new();
        change.change_file(file_id, Some(Arc::from("Процедура Тест() КонецПроцедуры")));

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        change.set_roots(vec![source_root]);

        change.apply(&mut db);

        let result = db.parse(file_id);
        assert!(!result.has_errors());
    }

    #[test]
    fn read_disk_text_preserves_bom_verbatim() {
        let dir = std::env::temp_dir().join(format!("bsl_rdt_bom_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bom.bsl");
        // A leading BOM plus body; read_disk_text must not strip it, so the
        // revision computed here matches what file_text_query recomputes on read.
        let raw = "\u{FEFF}Процедура Т() КонецПроцедуры";
        std::fs::write(&path, raw).unwrap();

        let got = queries::read_disk_text(&path).unwrap();
        assert_eq!(got, raw);
        assert_eq!(input::content_revision(&got), input::content_revision(raw));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decode_disk_bytes_matches_read_disk_text_verbatim() {
        // The VFS loader decodes watcher bytes via `decode_disk_bytes`; its output
        // must hash identically to `read_disk_text`'s disk re-read, or a BOM-led
        // file's recorded revision (from the loader) diverges from the on-read hash
        // and `file_text_query` trips `assert_revision`. 1C BSL files are saved with
        // a UTF-8 BOM, so the BOM must survive both paths.
        let raw = "\u{FEFF}Процедура Т() КонецПроцедуры";
        let decoded = queries::decode_disk_bytes(raw.as_bytes()).unwrap();
        assert_eq!(decoded, raw);
        assert_eq!(input::content_revision(&decoded), input::content_revision(raw));
    }

    #[test]
    fn content_revision_folds_in_length() {
        assert_eq!(input::content_revision("abc"), input::content_revision("abc"));
        assert_ne!(input::content_revision("ab"), input::content_revision("ba"));
        // length is folded in so a prefix is not aliased with the longer text
        assert_ne!(input::content_revision("a"), input::content_revision("aa"));
    }

    #[test]
    fn file_text_query_returns_overlay() {
        let mut db = TestDatabase::default();
        let file_id = FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/ov.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
        assert_eq!(&*db.file_text(file_id), "Процедура Тест() КонецПроцедуры");
    }

    #[test]
    fn file_text_query_reads_disk_without_overlay() {
        let dir = std::env::temp_dir().join(format!("bsl_ft_disk_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("disk.bsl");
        let content = "Функция Ф() Возврат 1; КонецФункции";
        std::fs::write(&path, content).unwrap();

        let mut db = TestDatabase::default();
        let file_id = FileId(7);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new(path.clone()));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        // disk-backed: no overlay text, only the content revision
        db.set_file_revision_from_disk(file_id, input::content_revision(content));

        assert_eq!(&*db.file_text(file_id), content);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A memo whose whole input cone lives under the metadata root must survive a
    /// LOW-durability write with a shallow O(1) verification — no walk into its
    /// dependencies — while the same memo shape under a local root deep-verifies.
    /// Deep verification is observable as a `DidValidateMemoizedValue` event for
    /// the nested `file_text_query` memo when `parse_query` is re-validated.
    #[test]
    fn metadata_root_cone_shallow_verifies_after_low_write() {
        use salsa::Database;

        let dir = std::env::temp_dir().join(format!("bsl_durability_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml_path = dir.join("Товары.xml");
        let bsl_path = dir.join("Модуль.bsl");
        let xml_content = "<MetaDataObject/>";
        let bsl_content = "Процедура А() КонецПроцедуры";
        std::fs::write(&xml_path, xml_content).unwrap();
        std::fs::write(&bsl_path, bsl_content).unwrap();

        let mut db = TestDatabase::default();
        let xml_file = FileId(0);
        let bsl_file = FileId(1);

        let mut metadata_set = FileSet::new();
        metadata_set.insert(xml_file, VfsPath::new(xml_path.clone()));
        db.set_source_root(METADATA_SOURCE_ROOT, SourceRoot::new_metadata(metadata_set));
        let mut bsl_set = FileSet::new();
        bsl_set.insert(bsl_file, VfsPath::new(bsl_path.clone()));
        db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(bsl_set));

        db.set_file_source_root(xml_file, METADATA_SOURCE_ROOT);
        db.set_file_source_root(bsl_file, BSL_SOURCE_ROOT);
        db.set_file_revision_from_disk(xml_file, input::content_revision(xml_content));
        db.set_file_revision_from_disk(bsl_file, input::content_revision(bsl_content));

        let _ = db.parse(xml_file);
        let _ = db.parse(bsl_file);

        db.synthetic_write(salsa::Durability::LOW);

        db.take_events();
        let _ = db.parse(xml_file);
        let xml_events = db.take_events();
        assert!(
            !xml_events.iter().any(|e| {
                e.contains("DidValidateMemoizedValue") && e.contains("file_text_query")
            }),
            "metadata-rooted parse must shallow-verify after a LOW write, \
             but its file_text dependency was walked: {xml_events:#?}"
        );

        db.take_events();
        let _ = db.parse(bsl_file);
        let bsl_events = db.take_events();
        assert!(
            bsl_events.iter().any(|e| {
                e.contains("DidValidateMemoizedValue") && e.contains("file_text_query")
            }),
            "local-rooted parse is LOW and must deep-verify its file_text dependency \
             after a LOW write: {bsl_events:#?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[should_panic(expected = "revision mismatch")]
    fn file_text_query_panics_on_disk_drift() {
        let dir = std::env::temp_dir().join(format!("bsl_ft_drift_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("drift.bsl");
        std::fs::write(&path, "actual on-disk bytes").unwrap();

        let mut db = TestDatabase::default();
        let file_id = FileId(3);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new(path));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        // revision computed from DIFFERENT content than what is on disk → drift
        db.set_file_revision_from_disk(file_id, input::content_revision("a stale snapshot"));

        let _ = db.file_text(file_id);
    }
}
