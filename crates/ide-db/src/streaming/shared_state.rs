use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crossbeam_utils::CachePadded;
use dashmap::DashMap;
use hir::{
    ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree, WorkspaceSymbols,
};
use parking_lot::{Condvar, Mutex};
use rustc_hash::FxBuildHasher;
use syntax::{Parse, SyntaxNode};
use vfs::{file_set::FileSet, FileId};

use super::{FileReader, GlobalContext};

pub struct ParsedFile {
    pub text: Arc<str>,
    pub parse: Arc<Parse<SyntaxNode>>,
    pub item_tree: Arc<ItemTree>,
    module_id: ModuleId,
    file_path: Option<Arc<str>>,
    module_bodies: OnceLock<Arc<ModuleBodies>>,
    module_cfgs: OnceLock<Arc<hir::cfg::ModuleCfgs>>,
    sdbl_hir: OnceLock<crate::SdblHirEntries>,
    module_metadata: OnceLock<Arc<ModuleMetadata>>,
    call_summary: OnceLock<Arc<hir::call_graph::ModuleCallSummary>>,
    line_index: OnceLock<Arc<line_index::LineIndex>>,
}

impl std::fmt::Debug for ParsedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParsedFile")
            .field("text_len", &self.text.len())
            .field("module_id", &self.module_id)
            .field("has_bodies", &self.module_bodies.get().is_some())
            .field("has_cfgs", &self.module_cfgs.get().is_some())
            .field("has_sdbl_hir", &self.sdbl_hir.get().is_some())
            .field("has_metadata", &self.module_metadata.get().is_some())
            .field("has_call_summary", &self.call_summary.get().is_some())
            .finish()
    }
}

impl ParsedFile {
    pub fn new(
        text: Arc<str>,
        parse: Arc<Parse<SyntaxNode>>,
        item_tree: Arc<ItemTree>,
        module_id: ModuleId,
        file_path: Option<Arc<str>>,
    ) -> Self {
        Self {
            text,
            parse,
            item_tree,
            module_id,
            file_path,
            module_bodies: OnceLock::new(),
            module_cfgs: OnceLock::new(),
            sdbl_hir: OnceLock::new(),
            module_metadata: OnceLock::new(),
            call_summary: OnceLock::new(),
            line_index: OnceLock::new(),
        }
    }

    pub fn line_index(&self) -> Arc<line_index::LineIndex> {
        self.line_index.get_or_init(|| Arc::new(line_index::LineIndex::new(&self.text))).clone()
    }

    pub fn module_bodies(&self) -> Arc<ModuleBodies> {
        self.module_bodies
            .get_or_init(|| Arc::new(ModuleBodies::from_parse(&self.parse, self.module_id)))
            .clone()
    }

    pub fn module_cfgs(&self) -> Arc<hir::cfg::ModuleCfgs> {
        self.module_cfgs
            .get_or_init(|| {
                let bodies = self.module_bodies();
                let mut cfgs = rustc_hash::FxHashMap::default();

                for (local_id, body) in bodies.iter_bodies() {
                    let source_map = bodies.source_map(local_id);
                    let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(
                        body.body_stmts_typed(),
                        body,
                        source_map,
                    );
                    cfgs.insert(local_id, Arc::new(cfg));
                }

                Arc::new(hir::cfg::ModuleCfgs::new(cfgs))
            })
            .clone()
    }

    pub fn sdbl_hir(
        &self,
        configuration: Option<&Arc<bsl_metadata::Configuration>>,
    ) -> crate::SdblHirEntries {
        self.sdbl_hir
            .get_or_init(|| {
                let module_bodies = self.module_bodies();
                let config_arc = configuration.cloned();

                let mut queries_with_pos: Vec<_> = Vec::new();

                for (local_id, body) in module_bodies.iter_bodies() {
                    for (expr_id, query_info) in body.sdbl_exprs() {
                        if let Some(ref sdbl_ast) = query_info.query_ast {
                            let pos = query_info.bsl_literal_range.start();
                            let sdbl_expr_id = hir::SdblExprId::from_method(local_id, expr_id);
                            queries_with_pos.push((pos, sdbl_expr_id, sdbl_ast.clone()));
                        }
                    }
                }

                if let Some(module_code) = module_bodies.module_code() {
                    for (expr_id, query_info) in module_code.sdbl_exprs() {
                        if let Some(ref sdbl_ast) = query_info.query_ast {
                            let pos = query_info.bsl_literal_range.start();
                            let sdbl_expr_id = hir::SdblExprId::from_module_code(expr_id);
                            queries_with_pos.push((pos, sdbl_expr_id, sdbl_ast.clone()));
                        }
                    }
                }

                queries_with_pos.sort_by_key(|(pos, _, _)| *pos);

                let result: Vec<_> = queries_with_pos
                    .into_iter()
                    .map(|(_, sdbl_expr_id, sdbl_ast)| {
                        let sdbl_package =
                            sdbl_hir::lower_sdbl_to_hir(&sdbl_ast, config_arc.clone());
                        (sdbl_expr_id, Arc::new(sdbl_package))
                    })
                    .collect();

                Arc::new(result)
            })
            .clone()
    }

    pub fn module_metadata(
        &self,
        configuration: Option<&bsl_metadata::Configuration>,
    ) -> Arc<ModuleMetadata> {
        self.module_metadata
            .get_or_init(|| {
                let file_path = match &self.file_path {
                    Some(path) => std::path::Path::new(path.as_ref()),
                    None => {
                        return Arc::new(ModuleMetadata::unknown(
                            bsl_metadata::ModuleType::Unknown,
                        ));
                    }
                };
                Arc::new(crate::build_module_metadata(file_path, configuration))
            })
            .clone()
    }

    pub fn call_summary(
        &self,
        configuration: Option<&bsl_metadata::Configuration>,
    ) -> Arc<hir::call_graph::ModuleCallSummary> {
        self.call_summary
            .get_or_init(|| {
                let item_tree = &self.item_tree;
                let module_bodies = self.module_bodies();
                let metadata = self.module_metadata(configuration);
                let form_handlers: &[bsl_metadata::FormEventHandler] =
                    metadata.form.as_ref().map(|f| f.event_handlers.as_slice()).unwrap_or(&[]);
                Arc::new(hir::call_graph::extract_call_summary(
                    item_tree,
                    &module_bodies,
                    form_handlers,
                ))
            })
            .clone()
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FileStatus {
    NotStarted = 0,
    Parsing = 1,
    SymbolTreeReady = 2,
    DiagnosticsInProgress = 3,
    Completed = 4,
}

impl FileStatus {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(FileStatus::NotStarted),
            1 => Some(FileStatus::Parsing),
            2 => Some(FileStatus::SymbolTreeReady),
            3 => Some(FileStatus::DiagnosticsInProgress),
            4 => Some(FileStatus::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClaimResult {
    ByUs,
    ByOther,
    AlreadyDone,
    NotReady,
}

#[derive(Debug, Clone)]
pub enum ProcessError {
    ParseError(FileId, Arc<str>),
    LoweringError(FileId, Arc<str>),
    DependencyFailed(FileId, Arc<str>),
    WorkerPanic(FileId, Arc<str>),
    IoError(FileId, Arc<str>),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::ParseError(file_id, msg) => {
                write!(f, "Parse error in {:?}: {}", file_id, msg)
            }
            ProcessError::LoweringError(file_id, msg) => {
                write!(f, "Lowering error in {:?}: {}", file_id, msg)
            }
            ProcessError::DependencyFailed(file_id, msg) => {
                write!(f, "Dependency {:?} failed: {}", file_id, msg)
            }
            ProcessError::WorkerPanic(file_id, msg) => {
                write!(f, "Worker panic in {:?}: {}", file_id, msg)
            }
            ProcessError::IoError(file_id, msg) => {
                write!(f, "I/O error in {:?}: {}", file_id, msg)
            }
        }
    }
}

impl std::error::Error for ProcessError {}

pub struct SharedState {
    file_statuses: Box<[AtomicU8]>,
    symbol_trees: Arc<DashMap<FileId, Arc<SymbolTree>, FxBuildHasher>>,
    parsed_files: DashMap<FileId, Arc<ParsedFile>, FxBuildHasher>,
    sorted_files: Arc<Vec<FileId>>,
    next_file_idx: CachePadded<AtomicUsize>,
    condvars: Box<[Condvar]>,
    mutexes: Box<[Mutex<()>]>,
    configuration: Option<Arc<bsl_metadata::Configuration>>,
    module_index: Arc<ModuleIndex>,
    workspace_symbols: Arc<WorkspaceSymbols>,
    file_set: Arc<FileSet>,
    file_reader: FileReader,
    failed_files: Arc<DashMap<FileId, Arc<str>, FxBuildHasher>>,
}

impl SharedState {
    pub fn new(global: GlobalContext, sorted_files: Vec<FileId>) -> Arc<Self> {
        let max_file_id = global
            .file_set
            .iter()
            .map(|f| f.index() as usize)
            .max()
            .map(|m| m + 1)
            .unwrap_or(sorted_files.len());

        Arc::new(Self {
            file_statuses: (0..max_file_id)
                .map(|_| AtomicU8::new(FileStatus::NotStarted as u8))
                .collect::<Vec<_>>()
                .into_boxed_slice(),

            symbol_trees: Arc::new(DashMap::with_hasher_and_shard_amount(FxBuildHasher, 16)),

            parsed_files: DashMap::with_hasher_and_shard_amount(FxBuildHasher, 16),

            sorted_files: Arc::new(sorted_files),
            next_file_idx: CachePadded::new(AtomicUsize::new(0)),

            condvars: (0..max_file_id)
                .map(|_| Condvar::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),

            mutexes: (0..max_file_id)
                .map(|_| Mutex::new(()))
                .collect::<Vec<_>>()
                .into_boxed_slice(),

            configuration: global.configuration,
            module_index: global.module_index,
            workspace_symbols: global.workspace_symbols,
            file_set: global.file_set,
            file_reader: global.file_reader,

            failed_files: Arc::new(DashMap::with_hasher(FxBuildHasher)),
        })
    }

    pub fn try_claim(&self, file_id: FileId) -> ClaimResult {
        let idx = file_id.index() as usize;

        match self.file_statuses[idx].compare_exchange(
            FileStatus::NotStarted as u8,
            FileStatus::Parsing as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => ClaimResult::ByUs,
            Err(current) => {
                if current >= FileStatus::Completed as u8 {
                    ClaimResult::AlreadyDone
                } else {
                    ClaimResult::ByOther
                }
            }
        }
    }

    pub fn claim_next_file(&self) -> Option<FileId> {
        loop {
            let idx = self.next_file_idx.fetch_add(1, Ordering::Relaxed);

            if idx >= self.sorted_files.len() {
                return None;
            }

            let file_id = self.sorted_files[idx];

            match self.try_claim(file_id) {
                ClaimResult::ByUs => return Some(file_id),
                ClaimResult::ByOther | ClaimResult::AlreadyDone | ClaimResult::NotReady => {
                    continue;
                }
            }
        }
    }

    pub fn publish_symbol_tree(&self, file_id: FileId, tree: Arc<SymbolTree>) {
        let idx = file_id.index() as usize;

        let _guard = self.mutexes[idx].lock();

        self.symbol_trees.insert(file_id, tree);

        self.file_statuses[idx].store(FileStatus::SymbolTreeReady as u8, Ordering::SeqCst);

        self.condvars[idx].notify_all();
    }

    pub fn publish_symbol_tree_and_start_diagnostics(
        &self,
        file_id: FileId,
        tree: Arc<SymbolTree>,
    ) {
        let idx = file_id.index() as usize;

        let _guard = self.mutexes[idx].lock();

        self.symbol_trees.insert(file_id, tree);

        self.file_statuses[idx].store(FileStatus::DiagnosticsInProgress as u8, Ordering::SeqCst);

        self.condvars[idx].notify_all();
    }

    pub fn get_symbol_tree(&self, file_id: FileId) -> Option<Arc<SymbolTree>> {
        self.symbol_trees.get(&file_id).map(|r| r.clone())
    }

    #[inline]
    pub fn is_symbol_tree_ready(&self, file_id: FileId) -> bool {
        let idx = file_id.index() as usize;
        self.file_statuses[idx].load(Ordering::SeqCst) >= FileStatus::SymbolTreeReady as u8
    }

    pub fn cache_parsed_file(&self, file_id: FileId, parsed: Arc<ParsedFile>) {
        self.parsed_files.insert(file_id, parsed);
    }

    pub fn get_parsed_file(&self, file_id: FileId) -> Option<Arc<ParsedFile>> {
        self.parsed_files.get(&file_id).map(|r| r.clone())
    }

    pub fn remove_parsed_file(&self, file_id: FileId) {
        self.parsed_files.remove(&file_id);
    }

    pub fn parsed_cache_len(&self) -> usize {
        self.parsed_files.len()
    }

    pub fn has_parsed_file(&self, file_id: FileId) -> bool {
        self.parsed_files.contains_key(&file_id)
    }

    pub fn wait_for_symbol_tree(&self, file_id: FileId) -> Result<(), ProcessError> {
        let idx = file_id.index() as usize;

        if self.is_symbol_tree_ready(file_id) {
            return Ok(());
        }

        let mut guard = self.mutexes[idx].lock();

        while !self.is_symbol_tree_ready(file_id) {
            if let Some(error) = self.failed_files.get(&file_id) {
                return Err(ProcessError::DependencyFailed(file_id, error.clone()));
            }

            self.condvars[idx].wait(&mut guard);
        }

        Ok(())
    }

    pub fn mark_completed(&self, file_id: FileId) {
        let idx = file_id.index() as usize;

        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);
    }

    pub fn mark_failed(&self, file_id: FileId, error: Arc<str>) {
        let idx = file_id.index() as usize;

        let _guard = self.mutexes[idx].lock();

        self.failed_files.insert(file_id, error);

        self.file_statuses[idx].store(FileStatus::Completed as u8, Ordering::SeqCst);

        self.condvars[idx].notify_all();
    }

    pub fn try_claim_for_diagnostics(&self, file_id: FileId) -> ClaimResult {
        let idx = file_id.index() as usize;

        match self.file_statuses[idx].compare_exchange(
            FileStatus::SymbolTreeReady as u8,
            FileStatus::DiagnosticsInProgress as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => ClaimResult::ByUs,
            Err(current) => {
                if current >= FileStatus::Completed as u8 {
                    ClaimResult::AlreadyDone
                } else if current == FileStatus::DiagnosticsInProgress as u8 {
                    ClaimResult::ByOther
                } else {
                    ClaimResult::NotReady
                }
            }
        }
    }

    pub fn num_files(&self) -> usize {
        self.sorted_files.len()
    }

    pub fn file_id_at(&self, idx: usize) -> Option<FileId> {
        self.sorted_files.get(idx).copied()
    }

    pub fn configuration(&self) -> Option<&Arc<bsl_metadata::Configuration>> {
        self.configuration.as_ref()
    }

    pub fn module_index(&self) -> &Arc<ModuleIndex> {
        &self.module_index
    }

    pub fn workspace_symbols(&self) -> &Arc<WorkspaceSymbols> {
        &self.workspace_symbols
    }

    pub fn file_set(&self) -> &Arc<FileSet> {
        &self.file_set
    }

    pub fn file_reader(&self) -> &FileReader {
        &self.file_reader
    }

    pub fn file_status(&self, file_id: FileId) -> FileStatus {
        let idx = file_id.index() as usize;
        let status = self.file_statuses[idx].load(Ordering::SeqCst);
        FileStatus::from_u8(status).unwrap_or(FileStatus::NotStarted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn create_test_state(num_files: usize) -> Arc<SharedState> {
        let file_ids: Vec<FileId> = (0..num_files).map(|i| FileId(i as u32)).collect();
        let global = GlobalContext::empty();
        SharedState::new(global, file_ids)
    }

    #[test]
    fn test_file_status_transitions() {
        assert_eq!(FileStatus::NotStarted as u8, 0);
        assert!(FileStatus::NotStarted < FileStatus::Parsing);
        assert!(FileStatus::Parsing < FileStatus::SymbolTreeReady);
        assert!(FileStatus::SymbolTreeReady < FileStatus::DiagnosticsInProgress);
        assert!(FileStatus::DiagnosticsInProgress < FileStatus::Completed);
    }

    #[test]
    fn test_try_claim_by_us() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        assert_eq!(state.try_claim(file_id), ClaimResult::ByUs);
        assert_eq!(state.file_status(file_id), FileStatus::Parsing);
        assert_eq!(state.try_claim(file_id), ClaimResult::ByOther);
    }

    #[test]
    fn test_claim_next_file() {
        let state = create_test_state(5);

        assert_eq!(state.claim_next_file(), Some(FileId(0)));
        assert_eq!(state.claim_next_file(), Some(FileId(1)));
        assert_eq!(state.claim_next_file(), Some(FileId(2)));
        assert_eq!(state.claim_next_file(), Some(FileId(3)));
        assert_eq!(state.claim_next_file(), Some(FileId(4)));

        assert_eq!(state.claim_next_file(), None);
    }

    #[test]
    fn test_publish_and_get_symbol_tree() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        assert_eq!(state.try_claim(file_id), ClaimResult::ByUs);

        let module_id = hir::ModuleId::new(file_id);
        let text = "";
        let parse = parser::parse(text);
        let item_tree = hir::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));

        state.publish_symbol_tree(file_id, symbol_tree.clone());

        assert_eq!(state.file_status(file_id), FileStatus::SymbolTreeReady);
        assert!(state.is_symbol_tree_ready(file_id));

        let retrieved = state.get_symbol_tree(file_id).unwrap();
        assert!(Arc::ptr_eq(&symbol_tree, &retrieved));
    }

    #[test]
    fn test_mark_completed() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        state.try_claim(file_id);
        let module_id = hir::ModuleId::new(file_id);
        let text = "";
        let parse = parser::parse(text);
        let item_tree = hir::ItemTree::from_parse(&parse);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));
        state.publish_symbol_tree(file_id, symbol_tree);

        state.mark_completed(file_id);
        assert_eq!(state.file_status(file_id), FileStatus::Completed);
        assert_eq!(state.try_claim(file_id), ClaimResult::AlreadyDone);
    }

    #[test]
    fn test_concurrent_claim() {
        let state = create_test_state(100);
        let state_clone1 = Arc::clone(&state);
        let state_clone2 = Arc::clone(&state);

        let handle1 = thread::spawn(move || {
            let mut claimed = vec![];
            while let Some(file_id) = state_clone1.claim_next_file() {
                claimed.push(file_id);
            }
            claimed
        });

        let handle2 = thread::spawn(move || {
            let mut claimed = vec![];
            while let Some(file_id) = state_clone2.claim_next_file() {
                claimed.push(file_id);
            }
            claimed
        });

        let claimed1 = handle1.join().unwrap();
        let claimed2 = handle2.join().unwrap();

        assert_eq!(claimed1.len() + claimed2.len(), 100);

        let mut all_claimed = claimed1;
        all_claimed.extend(claimed2);
        all_claimed.sort_by_key(|f| f.index());
        all_claimed.dedup();
        assert_eq!(all_claimed.len(), 100);
    }

    #[test]
    fn test_mark_failed() {
        let state = create_test_state(10);
        let file_id = FileId(0);

        state.try_claim(file_id);

        let error: Arc<str> = Arc::from("Test error");
        state.mark_failed(file_id, error.clone());

        assert_eq!(state.file_status(file_id), FileStatus::Completed);

        assert_eq!(state.failed_files.get(&file_id).unwrap().as_ref(), "Test error");
    }

    #[test]
    fn test_cache_parsed_file() {
        use hir::{ItemTree, ModuleId};
        use syntax::Parse;

        let state = create_test_state(10);
        let file_id = FileId(0);

        let text: Arc<str> = Arc::from("Процедура Тест() КонецПроцедуры");
        let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text));
        let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
        let module_id = ModuleId::new(file_id);

        let parsed = Arc::new(ParsedFile::new(text.clone(), parse, item_tree, module_id, None));

        state.cache_parsed_file(file_id, Arc::clone(&parsed));

        let retrieved = state.get_parsed_file(file_id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().text.as_ref(), text.as_ref());

        state.remove_parsed_file(file_id);
        assert!(state.get_parsed_file(file_id).is_none());
    }

    #[test]
    fn test_try_claim_for_diagnostics() {
        use hir::{ItemTree, ModuleId, SymbolTree};
        use syntax::Parse;

        let state = create_test_state(10);
        let file_id = FileId(0);

        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::NotReady);

        state.try_claim(file_id);

        let text = "Процедура Тест() КонецПроцедуры";
        let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(text));
        let item_tree = ItemTree::from_parse(&parse);
        let module_id = ModuleId::new(file_id);
        let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, text));

        state.publish_symbol_tree(file_id, symbol_tree);

        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::ByUs);
        assert_eq!(state.file_status(file_id), FileStatus::DiagnosticsInProgress);

        assert_eq!(state.try_claim_for_diagnostics(file_id), ClaimResult::ByOther);
    }

    #[test]
    fn test_num_files_and_file_id_at() {
        let state = create_test_state(5);

        assert_eq!(state.num_files(), 5);
        assert_eq!(state.file_id_at(0), Some(FileId(0)));
        assert_eq!(state.file_id_at(4), Some(FileId(4)));
        assert_eq!(state.file_id_at(5), None);
    }

    #[test]
    fn test_cache_lifecycle() {
        use hir::{ItemTree, ModuleId};
        use syntax::Parse;

        let state = create_test_state(3);
        let file_0 = FileId(0);
        let file_1 = FileId(1);

        assert_eq!(state.parsed_cache_len(), 0);
        assert!(!state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));

        let text_0: Arc<str> = Arc::from("Процедура Тест0() КонецПроцедуры");
        let parse_0: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text_0));
        let item_tree_0: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse_0));
        let module_id_0 = ModuleId::new(file_0);
        let parsed_0 = Arc::new(ParsedFile::new(text_0, parse_0, item_tree_0, module_id_0, None));
        state.cache_parsed_file(file_0, parsed_0);

        assert_eq!(state.parsed_cache_len(), 1);
        assert!(state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));

        let text_1: Arc<str> = Arc::from("Процедура Тест1() КонецПроцедуры");
        let parse_1: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text_1));
        let item_tree_1: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse_1));
        let module_id_1 = ModuleId::new(file_1);
        let parsed_1 = Arc::new(ParsedFile::new(text_1, parse_1, item_tree_1, module_id_1, None));
        state.cache_parsed_file(file_1, parsed_1);

        assert_eq!(state.parsed_cache_len(), 2);
        assert!(state.has_parsed_file(file_0));
        assert!(state.has_parsed_file(file_1));

        state.remove_parsed_file(file_0);
        assert_eq!(state.parsed_cache_len(), 1);
        assert!(!state.has_parsed_file(file_0));
        assert!(state.has_parsed_file(file_1));

        state.remove_parsed_file(file_1);
        assert_eq!(state.parsed_cache_len(), 0);
        assert!(!state.has_parsed_file(file_0));
        assert!(!state.has_parsed_file(file_1));
    }
}
