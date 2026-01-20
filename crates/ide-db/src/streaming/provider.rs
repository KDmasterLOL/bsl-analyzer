//! StreamingProvider implementation.

use std::sync::Arc;

use base_db::SourceRootId;
use bsl_metadata::Configuration;
use hir_def::{
    ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree, WorkspaceSymbols,
};
use rustc_hash::FxHashMap;
use syntax::{Parse, SyntaxNode};
use vfs::FileId;

use crate::metadata::get_module_type_from_uri;
use crate::provider::AnalysisProvider;

use super::global_context::GlobalContext;
use super::shared_state::SharedState;

/// Streaming analysis provider.
///
/// Implements `AnalysisProvider` by:
/// - Using `GlobalContext` for shared data (configuration, symbol trees)
/// - Using `SharedState` for per-file cache (parse, item_tree) during processing
/// - Computing on-the-fly when cache miss
pub struct StreamingProvider {
    /// Shared global context.
    global: Arc<GlobalContext>,

    /// Optional shared state for caching.
    /// When Some, provider checks cache first for parse/item_tree.
    shared_state: Option<Arc<SharedState>>,
}

impl StreamingProvider {
    /// Create a new StreamingProvider with the given global context.
    pub fn new(global: Arc<GlobalContext>) -> Self {
        Self { global, shared_state: None }
    }

    /// Create a new StreamingProvider with shared state for caching.
    pub fn with_shared_state(global: Arc<GlobalContext>, shared_state: Arc<SharedState>) -> Self {
        Self { global, shared_state: Some(shared_state) }
    }

    /// Get the global context.
    pub fn global(&self) -> &GlobalContext {
        &self.global
    }
}

impl AnalysisProvider for StreamingProvider {
    // ========================================================================
    // Global Data (from GlobalContext)
    // ========================================================================

    fn configuration(&self) -> Option<Arc<Configuration>> {
        self.global.configuration.clone()
    }

    fn workspace_symbols(&self, _source_root_id: SourceRootId) -> Arc<WorkspaceSymbols> {
        self.global.workspace_symbols.clone()
    }

    fn module_index(&self, _source_root_id: SourceRootId) -> Arc<ModuleIndex> {
        self.global.module_index.clone()
    }

    // ========================================================================
    // Per-file Data (cached via SharedState or computed on-the-fly)
    // ========================================================================

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        // Check cache first
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return (*parsed.parse).clone();
            }
        }

        // Cache miss - parse from disk
        // Use SharedState's file_reader if available (for tracking), otherwise global's
        let text = if let Some(ref shared_state) = self.shared_state {
            shared_state.file_reader().read(file_id).unwrap_or_default()
        } else {
            self.global.file_reader.read(file_id).unwrap_or_default()
        };
        parser::parse(&text)
    }

    fn file_text(&self, file_id: FileId) -> String {
        // Check cache first
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return parsed.text.to_string();
            }
        }

        // Cache miss - read from disk
        // Use SharedState's file_reader if available (for tracking), otherwise global's
        if let Some(ref shared_state) = self.shared_state {
            shared_state.file_reader().read(file_id).unwrap_or_default()
        } else {
            self.global.file_reader.read(file_id).unwrap_or_default()
        }
    }

    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        // Check cache first
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return Arc::clone(&parsed.item_tree);
            }
        }

        // Cache miss - build from parse
        let parse = self.parse(file_id);
        Arc::new(ItemTree::from_parse(&parse))
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        // Check SharedState first (published during streaming processing)
        if let Some(ref shared_state) = self.shared_state {
            if let Some(tree) = shared_state.get_symbol_tree(module_id.file_id) {
                return tree;
            }
        }

        // Check pre-built symbol tree from global context
        if let Some(tree) = self.global.symbol_trees.get(&module_id.file_id) {
            return tree.clone();
        }

        // Build on-the-fly if not available
        let item_tree = self.item_tree(module_id.file_id);
        Arc::new(SymbolTree::from_item_tree(&item_tree, module_id))
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        // Check ParsedFile cache (lazy computation via OnceLock)
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(module_id.file_id) {
                return parsed.module_bodies();
            }
        }

        // Fallback - compute on-the-fly
        let parse = self.parse(module_id.file_id);
        Arc::new(ModuleBodies::from_parse(&parse, module_id))
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        let file_path = self.file_path(module_id.file_id);
        let module_type = file_path
            .as_ref()
            .and_then(|p| get_module_type_from_uri(p))
            .unwrap_or(bsl_metadata::ModuleType::Unknown);

        // If it's a CommonModule, try to get execution context from configuration
        if module_type == bsl_metadata::ModuleType::CommonModule {
            if let (Some(config), Some(path)) = (&self.global.configuration, &file_path) {
                if let Some(name) = extract_common_module_name(path) {
                    if let Some(common_module) = config.find_common_module(&name) {
                        let execution_context = hir_def::compute_execution_context(common_module);
                        return Arc::new(ModuleMetadata {
                            module_type,
                            execution_context: Some(execution_context),
                            common_module: Some(Arc::new(common_module.clone())),
                            mdo: None,
                            register: None,
                            form: None,
                        });
                    }
                }
            }
        }

        Arc::new(ModuleMetadata::unknown(module_type))
    }

    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex> {
        let text = self.file_text(file_id);
        Arc::new(line_index::LineIndex::new(&text))
    }

    fn file_path(&self, file_id: FileId) -> Option<String> {
        self.global
            .file_set
            .path_for_file(&file_id)
            .map(|p| p.as_path().to_string_lossy().to_string())
    }

    fn file_source_root_id(&self, _file_id: FileId) -> SourceRootId {
        // In streaming mode, all files belong to a single synthetic source root
        SourceRootId(0)
    }

    // ========================================================================
    // Dataflow Analysis (computed on-the-fly)
    // ========================================================================

    fn module_cfgs(&self, file_id: FileId) -> Arc<cfg::ModuleCfgs> {
        // Check ParsedFile cache (lazy computation via OnceLock)
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return parsed.module_cfgs();
            }
        }

        // Fallback - compute on-the-fly
        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut cfgs = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let source_map = module_bodies.source_map(local_id);
            let cfg = cfg::CfgBuilder::new().build_graph_from_hir(
                body.body_stmts_typed(),
                body,
                source_map,
            );
            cfgs.insert(local_id, Arc::new(cfg));
        }

        Arc::new(cfg::ModuleCfgs::new(cfgs))
    }

    fn module_liveness_analysis(&self, file_id: FileId) -> Arc<dataflow::liveness::ModuleLiveness> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut results = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(cfg) => cfg,
                None => continue,
            };

            let var_index = dataflow::liveness::VariableIndex::from_body(body);

            if let Some(liveness_result) = dataflow::liveness::liveness_analysis_direct(
                body, cfg, var_index, 10000, // max_iterations
            ) {
                results.insert(local_id, Arc::new(liveness_result));
            }
        }

        Arc::new(dataflow::liveness::ModuleLiveness::new(results))
    }

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<dataflow::reaching_defs::ModuleReachingDefs> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut results = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(cfg) => cfg.clone(),
                None => continue,
            };

            // Build definition index from body with parameters
            let params: Vec<_> = body
                .params()
                .map(|param_id| {
                    let binding = body.binding(param_id);
                    (binding.name.clone(), param_id)
                })
                .collect();
            let def_index =
                dataflow::reaching_defs::DefinitionIndex::from_body_with_params(body, params);

            // Initialize entry state with parameters
            let mut initial_defs = dataflow::reaching_defs::ReachingDefs::new(def_index.clone());
            for param_id in body.params() {
                let binding = body.binding(param_id);
                let def = dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
                initial_defs.insert(&def);
            }

            // Run dataflow analysis
            let transfer = dataflow::reaching_defs::ReachingDefsTransfer;
            let mut solver = dataflow::DataflowSolver::new(cfg, body.clone(), transfer);
            solver.set_max_iterations(10000);
            solver.set_bottom_factory(|| {
                dataflow::reaching_defs::ReachingDefs::new(def_index.clone())
            });
            solver.set_initial_state(initial_defs);

            if let Some(dataflow_result) = solver.solve() {
                let result = dataflow::reaching_defs::ReachingDefsResult::new(dataflow_result);
                results.insert(local_id, Arc::new(result));
            }
        }

        Arc::new(dataflow::reaching_defs::ModuleReachingDefs::new(results))
    }

    fn region_tree(&self, file_id: FileId) -> Arc<hir_def::RegionTree> {
        let parse = self.parse(file_id);
        Arc::new(hir_def::region_tree::lower_regions(&parse.syntax_node()))
    }

    fn module_level_regions(&self, file_id: FileId) -> Arc<Vec<base_db::RegionInfo>> {
        use syntax::{
            ast::{self, AstNode},
            SyntaxKind, TextRange, TextSize,
        };

        let parse = self.parse(file_id);
        let root = parse.syntax_node();

        let mut regions = Vec::new();
        for child in root.children() {
            if child.kind() == SyntaxKind::PRE_REGION_DIR {
                if let Some(region) = ast::PreRegionDir::cast(child.clone()) {
                    if region.is_start() {
                        if let Some(name) = region.name() {
                            let text = child.text().to_string();
                            let first_line = text.lines().next().unwrap_or(&text);
                            let first_line_len = first_line.len();

                            let start = child.text_range().start();
                            let end = start + TextSize::from(first_line_len as u32);
                            let range = TextRange::new(start, end);

                            regions.push(base_db::RegionInfo { name, range });
                        }
                    }
                }
            }
        }

        Arc::new(regions)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> crate::SdblHirEntries {
        // Check ParsedFile cache (lazy computation via OnceLock)
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                let configuration = self.configuration();
                return parsed.sdbl_hir(configuration.as_ref());
            }
        }

        // Fallback - compute on-the-fly (when not in streaming mode or cache miss)
        let sdbl_queries = self.all_sdbl_in_file(file_id);
        let configuration = self.configuration();
        let config_ref = configuration.as_deref();

        let mut result = Vec::with_capacity(sdbl_queries.len());
        for (sdbl_expr_id, query_info) in sdbl_queries.iter() {
            if let Some(ref sdbl_ast) = query_info.query_ast {
                let sdbl_package = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, config_ref);
                result.push((*sdbl_expr_id, Arc::new(sdbl_package)));
            }
        }

        Arc::new(result)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir_def::SdblExprId, syntax::SdblQueryInfo)>> {
        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut result = Vec::new();

        // Collect from all method bodies (procedures and functions)
        for (local_id, body) in module_bodies.iter_bodies() {
            for (expr_id, query_info) in body.sdbl_exprs() {
                let sdbl_expr_id = hir_def::SdblExprId::from_method(local_id, expr_id);
                result.push((sdbl_expr_id, query_info.clone()));
            }
        }

        // Collect from module-level code (statements outside methods)
        if let Some(module_code) = module_bodies.module_code() {
            for (expr_id, query_info) in module_code.sdbl_exprs() {
                let sdbl_expr_id = hir_def::SdblExprId::from_module_code(expr_id);
                result.push((sdbl_expr_id, query_info.clone()));
            }
        }

        // Sort by position in file for deterministic output
        result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

        Arc::new(result)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<hir_def::ModuleData> {
        let item_tree = self.item_tree(module_id.file_id);
        Arc::new(hir_def::ModuleData::from_item_tree(module_id, item_tree))
    }

    fn method_docs(&self, method_id: hir_def::MethodId) -> Option<Arc<hir_def::docs::MethodDocs>> {
        let parse = self.parse(method_id.module.file_id);
        let tree = self.item_tree(method_id.module.file_id);
        let file_text = self.file_text(method_id.module.file_id);

        hir_def::docs::compute_method_docs(&parse, &tree, method_id, &file_text)
    }

    fn reaching_definitions(
        &self,
        method_id: hir_def::MethodId,
    ) -> Option<Arc<dataflow::reaching_defs::ReachingDefsResult>> {
        // Get module-level reaching definitions
        let module_reaching_defs = self.module_reaching_definitions(method_id.module.file_id);

        // Extract result for specific method
        module_reaching_defs.get(method_id.local_id).cloned()
    }

    fn resolve_vfs_path(
        &self,
        _source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        // In streaming mode, use global file_set directly (no Salsa)
        self.global.file_set.file_for_path(vfs_path).copied()
    }
}

/// Extract CommonModule name from file path.
fn extract_common_module_name(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/");

    // Try English pattern
    if let Some(idx) = normalized.find("CommonModules/") {
        let after = &normalized[idx + "CommonModules/".len()..];
        return after.split('/').next().map(String::from);
    }

    // Try Russian pattern
    if let Some(idx) = normalized.find("ОбщиеМодули/") {
        let after = &normalized[idx + "ОбщиеМодули/".len()..];
        return after.split('/').next().map(String::from);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{FileReader, GlobalContext};
    use vfs::{file_set::FileSet, VfsPath};

    #[test]
    fn test_extract_common_module_name_english() {
        let path = "/project/CommonModules/MyModule/Ext/Module.bsl";
        assert_eq!(extract_common_module_name(path), Some("MyModule".to_string()));
    }

    #[test]
    fn test_extract_common_module_name_russian() {
        let path = "/project/ОбщиеМодули/МойМодуль/Ext/Module.bsl";
        assert_eq!(extract_common_module_name(path), Some("МойМодуль".to_string()));
    }

    #[test]
    fn test_extract_common_module_name_not_found() {
        let path = "/project/Catalogs/MyCatalog/Ext/Module.bsl";
        assert_eq!(extract_common_module_name(path), None);
    }

    #[test]
    fn test_streaming_provider_parse() {
        let mut files = FxHashMap::default();
        let file_id = FileId(0);
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = StreamingProvider::new(global);
        let parse = provider.parse(file_id);
        assert!(!parse.has_errors());
    }

    #[test]
    fn test_streaming_provider_item_tree() {
        let mut files = FxHashMap::default();
        let file_id = FileId(0);
        files.insert(file_id, "Процедура Тест() КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = StreamingProvider::new(global);
        let item_tree = provider.item_tree(file_id);
        assert_eq!(item_tree.top_level_items().len(), 1);
    }

    #[test]
    fn test_streaming_provider_module_bodies() {
        let mut files = FxHashMap::default();
        let file_id = FileId(0);
        files.insert(file_id, "Процедура Тест()\n  А = 1;\nКонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
        });

        let provider = StreamingProvider::new(global);
        let module_id = ModuleId::new(file_id);
        let bodies = provider.module_bodies(module_id);
        assert_eq!(bodies.iter_bodies().count(), 1);
    }
}
