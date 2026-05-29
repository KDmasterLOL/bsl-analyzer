use std::sync::Arc;

use hir::{ItemTree, ModuleId, SymbolTree};
use syntax::Parse;
use vfs::FileId;

use crate::provider::AnalysisProvider;

use super::shared_state::{ClaimResult, ParsedFile, ProcessError, SharedState};

pub fn get_or_process_symbol_tree(
    target_file: FileId,
    provider: &dyn AnalysisProvider,
    shared_state: &SharedState,
) -> Result<Arc<SymbolTree>, ProcessError> {
    let _span = tracing::debug_span!("get_or_process_symbol_tree", ?target_file).entered();

    if shared_state.is_symbol_tree_ready(target_file) {
        tracing::trace!(target_file = ?target_file, "SymbolTree already ready (fast path)");
        return shared_state.get_symbol_tree(target_file).ok_or_else(|| {
            ProcessError::DependencyFailed(
                target_file,
                Arc::from("SymbolTree marked ready but not found"),
            )
        });
    }

    match shared_state.try_claim(target_file) {
        ClaimResult::ByUs => {
            tracing::debug!(target_file = ?target_file, "Claimed for recursive processing");
            process_symbol_tree_only(target_file, provider, shared_state)?;
        }
        ClaimResult::ByOther | ClaimResult::NotReady => {
            tracing::debug!(target_file = ?target_file, "Waiting for other worker to finish");
            shared_state.wait_for_symbol_tree(target_file)?;
        }
        ClaimResult::AlreadyDone => {
            tracing::trace!(target_file = ?target_file, "Race condition: already done");
        }
    }

    shared_state.get_symbol_tree(target_file).ok_or_else(|| {
        ProcessError::DependencyFailed(
            target_file,
            Arc::from("SymbolTree should be ready but not found"),
        )
    })
}

fn process_symbol_tree_only(
    file_id: FileId,
    provider: &dyn AnalysisProvider,
    shared_state: &SharedState,
) -> Result<(), ProcessError> {
    let _span = tracing::debug_span!("process_symbol_tree_only", ?file_id).entered();

    let text: Arc<str> = Arc::from(provider.file_text(file_id));

    if text.is_empty() {
        tracing::warn!(file_id = ?file_id, "Empty file in recursive processing");
    }

    let parse: Arc<Parse<syntax::SyntaxNode>> = Arc::new(parser::parse(&text));
    if parse.has_errors() {
        let errors: Vec<_> = parse.errors().iter().take(3).map(|e| e.to_string()).collect();
        tracing::warn!(file_id = ?file_id, errors = ?errors, "Parse errors in recursive processing");
    }

    let item_tree: Arc<ItemTree> = Arc::new(ItemTree::from_parse(&parse));
    tracing::trace!(
        file_id = ?file_id,
        num_procedures = item_tree.procedures().count(),
        num_functions = item_tree.functions().count(),
        "ItemTree built in recursive processing"
    );

    let module_id = ModuleId::new(file_id);
    let symbol_tree = Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));

    let file_path: Option<Arc<str>> = provider.file_path(file_id).map(Arc::from);

    let parsed_file =
        Arc::new(ParsedFile::new(text, parse, Arc::clone(&item_tree), module_id, file_path));
    shared_state.cache_parsed_file(file_id, parsed_file);

    shared_state.publish_symbol_tree(file_id, symbol_tree);

    tracing::debug!(file_id = ?file_id, "SymbolTree published in recursive processing (Phase 2 pending)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{FileReader, GlobalContext, StreamingProvider};
    use rustc_hash::FxHashMap;
    use vfs::{file_set::FileSet, VfsPath};

    fn create_test_provider_with_files(
        files: FxHashMap<FileId, String>,
    ) -> (Arc<StreamingProvider>, Arc<SharedState>, Vec<FileId>) {
        let mut file_set = FileSet::default();
        for &file_id in files.keys() {
            file_set.insert(file_id, VfsPath::new(format!("/test{}.bsl", file_id.index())));
        }

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(hir::WorkspaceSymbols::default()),
            module_index: Arc::new(hir::ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files.clone()),
            config_root: None,
        });

        let provider = Arc::new(StreamingProvider::new(global));

        let sorted_files: Vec<FileId> = files.keys().copied().collect();
        let global_context = GlobalContext::empty();
        let shared_state = SharedState::new(global_context, sorted_files.clone());

        (provider, shared_state, sorted_files)
    }

    #[test]
    fn test_fast_path_already_ready() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        let module_id = ModuleId::new(file_id);
        let item_tree = provider.item_tree(file_id);
        let parse = provider.parse(file_id);
        let text = provider.file_text(file_id);
        let symbol_tree =
            Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));
        shared_state.publish_symbol_tree(file_id, symbol_tree.clone());

        let result = get_or_process_symbol_tree(file_id, &*provider, &shared_state);
        assert!(result.is_ok());
        let returned_tree = result.unwrap();

        assert_eq!(returned_tree.methods().count(), symbol_tree.methods().count());
    }

    #[test]
    fn test_recursive_processing_not_claimed() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Функция ТестФункция() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        let result = get_or_process_symbol_tree(file_id, &*provider, &shared_state);
        assert!(result.is_ok());
        let symbol_tree = result.unwrap();

        assert!(symbol_tree.find_method(&hir::Name::new("ТестФункция")).is_some());

        assert!(shared_state.is_symbol_tree_ready(file_id));
        assert_ne!(
            shared_state.file_status(file_id),
            super::super::shared_state::FileStatus::Completed
        );
    }

    #[test]
    fn test_process_symbol_tree_only() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура А() КонецПроцедуры\nФункция Б() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        let result = process_symbol_tree_only(file_id, &*provider, &shared_state);
        assert!(result.is_ok());

        assert!(shared_state.is_symbol_tree_ready(file_id));
        let symbol_tree = shared_state.get_symbol_tree(file_id).unwrap();

        assert!(symbol_tree.find_method(&hir::Name::new("А")).is_some());
        assert!(symbol_tree.find_method(&hir::Name::new("Б")).is_some());
    }

    #[test]
    fn test_cyclic_dependency_simulation() {
        let file_a = FileId(0);
        let file_b = FileId(1);

        let mut files = FxHashMap::default();
        files.insert(file_a, "Процедура ИзА() КонецПроцедуры".to_string());
        files.insert(file_b, "Функция ИзБ() КонецФункции".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        assert_eq!(shared_state.try_claim(file_a), super::super::shared_state::ClaimResult::ByUs);
        process_symbol_tree_only(file_a, &*provider, &shared_state).unwrap();

        assert_eq!(shared_state.try_claim(file_b), super::super::shared_state::ClaimResult::ByUs);
        process_symbol_tree_only(file_b, &*provider, &shared_state).unwrap();

        let result_b = get_or_process_symbol_tree(file_b, &*provider, &shared_state);
        assert!(result_b.is_ok());

        let result_a = get_or_process_symbol_tree(file_a, &*provider, &shared_state);
        assert!(result_a.is_ok());

        let tree_a = result_a.unwrap();
        let tree_b = result_b.unwrap();

        assert!(tree_a.find_method(&hir::Name::new("ИзА")).is_some());
        assert!(tree_b.find_method(&hir::Name::new("ИзБ")).is_some());
    }

    #[test]
    fn test_error_propagation() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура ОшибочнаяПроцедура( КонецПроцедуры".to_string());

        let (provider, shared_state, _sorted_files) = create_test_provider_with_files(files);

        assert_eq!(shared_state.try_claim(file_id), super::super::shared_state::ClaimResult::ByUs);

        let result = process_symbol_tree_only(file_id, &*provider, &shared_state);

        assert!(result.is_ok());
        assert!(shared_state.is_symbol_tree_ready(file_id));

        let symbol_tree = shared_state.get_symbol_tree(file_id);
        assert!(symbol_tree.is_some());
    }

    #[test]
    fn test_wait_scenario_with_threads() {
        use std::sync::mpsc;
        use std::sync::Barrier;
        use std::thread;
        use std::time::Duration;

        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура МногоРаботы() КонецПроцедуры".to_string());

        let (provider, shared_state, _) = create_test_provider_with_files(files);

        let provider = Arc::clone(&provider);
        let shared_state = Arc::clone(&shared_state);

        let barrier = Arc::new(Barrier::new(2));

        let (tx1, rx1) = mpsc::channel();
        let (tx2, rx2) = mpsc::channel();

        let barrier1 = Arc::clone(&barrier);
        let provider1 = Arc::clone(&provider);
        let state1 = Arc::clone(&shared_state);

        let barrier2 = Arc::clone(&barrier);
        let provider2 = Arc::clone(&provider);
        let state2 = Arc::clone(&shared_state);

        thread::spawn(move || {
            barrier1.wait();
            let result = get_or_process_symbol_tree(file_id, &*provider1, &state1);
            let _ = tx1.send(result);
        });

        thread::spawn(move || {
            barrier2.wait();
            let result = get_or_process_symbol_tree(file_id, &*provider2, &state2);
            let _ = tx2.send(result);
        });

        let timeout = Duration::from_secs(5);
        let result1 = rx1
            .recv_timeout(timeout)
            .expect("Thread 1 timed out after 5 seconds - possible deadlock!");
        let result2 = rx2
            .recv_timeout(timeout)
            .expect("Thread 2 timed out after 5 seconds - possible deadlock!");

        assert!(result1.is_ok(), "Thread 1 failed: {:?}", result1.err());
        assert!(result2.is_ok(), "Thread 2 failed: {:?}", result2.err());

        let tree1 = result1.unwrap();
        let tree2 = result2.unwrap();

        assert_eq!(tree1.methods().count(), tree2.methods().count());
    }
}
