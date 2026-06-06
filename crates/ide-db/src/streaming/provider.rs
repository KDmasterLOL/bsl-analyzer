use std::sync::Arc;

use base_db::SourceRootId;
use bsl_metadata::Configuration;
use hir::{
    InferenceResult, ItemTree, ModuleBodies, ModuleId, ModuleIndex, ModuleMetadata, SymbolTree,
    WorkspaceSymbols,
};
use rustc_hash::FxHashMap;
use syntax::{Parse, SyntaxNode};
use vfs::FileId;

use crate::provider::{AnalysisProvider, VisibleConfigWithRoot};

use super::global_context::GlobalContext;
use super::shared_state::SharedState;

pub struct StreamingProvider {
    global: Arc<GlobalContext>,
    shared_state: Option<Arc<SharedState>>,
}

impl StreamingProvider {
    pub fn new(global: Arc<GlobalContext>) -> Self {
        Self { global, shared_state: None }
    }

    pub fn with_shared_state(global: Arc<GlobalContext>, shared_state: Arc<SharedState>) -> Self {
        Self { global, shared_state: Some(shared_state) }
    }

    pub fn global(&self) -> &GlobalContext {
        &self.global
    }
}

impl AnalysisProvider for StreamingProvider {
    fn configuration(&self) -> Option<Arc<Configuration>> {
        self.global.configuration.clone()
    }

    fn visible_configurations(&self, _file_id: FileId) -> Vec<VisibleConfigWithRoot> {
        let (Some(configuration), Some(root)) =
            (self.global.configuration.clone(), self.global.config_root.clone())
        else {
            return Vec::new();
        };
        vec![VisibleConfigWithRoot {
            config: bsl_config::VisibleConfig { name: None, configuration },
            root,
        }]
    }

    fn workspace_symbols(&self, _source_root_id: SourceRootId) -> Arc<WorkspaceSymbols> {
        self.global.workspace_symbols.clone()
    }

    fn module_index(&self, _source_root_id: SourceRootId) -> Arc<ModuleIndex> {
        self.global.module_index.clone()
    }

    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return (*parsed.parse).clone();
            }
        }

        let text = if let Some(ref shared_state) = self.shared_state {
            shared_state.file_reader().read(file_id).unwrap_or_default()
        } else {
            self.global.file_reader.read(file_id).unwrap_or_default()
        };
        parser::parse(&text)
    }

    fn file_text(&self, file_id: FileId) -> String {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return parsed.text.to_string();
            }
        }

        if let Some(ref shared_state) = self.shared_state {
            shared_state.file_reader().read(file_id).unwrap_or_default()
        } else {
            self.global.file_reader.read(file_id).unwrap_or_default()
        }
    }

    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return Arc::clone(&parsed.item_tree);
            }
        }

        let parse = self.parse(file_id);
        Arc::new(ItemTree::from_parse(&parse))
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(tree) = shared_state.get_symbol_tree(module_id.file_id) {
                return tree;
            }
        }

        if let Some(tree) = self.global.symbol_trees.get(&module_id.file_id) {
            return tree.clone();
        }

        let item_tree = self.item_tree(module_id.file_id);
        let parse = self.parse(module_id.file_id);
        let source_text = self.file_text(module_id.file_id);
        Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &source_text))
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(module_id.file_id) {
                return parsed.module_bodies();
            }
        }

        let parse = self.parse(module_id.file_id);
        Arc::new(ModuleBodies::from_parse(&parse, module_id))
    }

    fn kernel_type_display(
        &self,
        _id: bsl_types::kind::TypeId,
        _locale: base_db::Locale,
    ) -> String {
        String::new()
    }

    fn infer(&self, _file_id: FileId) -> Arc<InferenceResult> {
        Arc::new(InferenceResult::default())
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(module_id.file_id) {
                let configuration = self.global.configuration.as_deref();
                return parsed.module_metadata(configuration);
            }
        }

        let file_path_str = match self.file_path(module_id.file_id) {
            Some(path) => path,
            None => {
                tracing::debug!("Could not determine file path for metadata");
                return Arc::new(ModuleMetadata::unknown(bsl_metadata::ModuleType::Unknown));
            }
        };

        let file_path = std::path::Path::new(&file_path_str);
        let configuration = self.global.configuration.as_deref();

        Arc::new(crate::build_module_metadata(file_path, configuration))
    }

    fn call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(module_id.file_id) {
                let configuration = self.global.configuration.as_deref();
                return parsed.call_summary(configuration);
            }
        }

        let item_tree = self.item_tree(module_id.file_id);
        let module_bodies = self.module_bodies(module_id);
        let metadata = self.module_metadata(module_id);
        let form_handlers: &[bsl_metadata::FormEventHandler] =
            metadata.form.as_ref().map(|f| f.event_handlers.as_slice()).unwrap_or(&[]);
        Arc::new(hir::call_graph::extract_call_summary(&item_tree, &module_bodies, form_handlers))
    }

    fn line_index(&self, file_id: FileId) -> Arc<line_index::LineIndex> {
        if let Some(shared) = &self.shared_state {
            if let Some(parsed) = shared.get_parsed_file(file_id) {
                return parsed.line_index();
            }
        }
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
        SourceRootId(0)
    }

    fn module_cfgs(&self, file_id: FileId) -> Arc<hir::cfg::ModuleCfgs> {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                return parsed.module_cfgs();
            }
        }

        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut cfgs = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let source_map = module_bodies.source_map(local_id);
            let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(
                body.body_stmts_typed(),
                body,
                source_map,
            );
            cfgs.insert(local_id, Arc::new(cfg));
        }

        Arc::new(hir::cfg::ModuleCfgs::new(cfgs))
    }

    fn module_liveness_analysis(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut results = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(cfg) => cfg,
                None => continue,
            };

            let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);

            if let Some(liveness_result) = hir::dataflow::liveness::liveness_analysis_direct(
                body,
                cfg,
                var_index,
                hir::dataflow::DEFAULT_MAX_ITERATIONS,
            ) {
                results.insert(local_id, Arc::new(liveness_result));
            }
        }

        Arc::new(hir::dataflow::liveness::ModuleLiveness::new(results))
    }

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut results = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(cfg) => cfg.clone(),
                None => continue,
            };

            let params: Vec<_> = body
                .params()
                .map(|param_id| {
                    let binding = body.binding(param_id);
                    (binding.name.clone(), param_id)
                })
                .collect();
            let def_index =
                hir::dataflow::reaching_defs::DefinitionIndex::from_body_with_params(body, params);

            let mut initial_defs =
                hir::dataflow::reaching_defs::ReachingDefs::new(def_index.clone());
            for param_id in body.params() {
                let binding = body.binding(param_id);
                let def =
                    hir::dataflow::reaching_defs::Definition::parameter(&binding.name, param_id);
                initial_defs.insert(&def);
            }

            let transfer = hir::dataflow::reaching_defs::ReachingDefsTransfer;
            let mut solver = hir::dataflow::DataflowSolver::new(cfg, body.clone(), transfer);
            solver.set_max_iterations(hir::dataflow::DEFAULT_MAX_ITERATIONS);
            solver.set_bottom_factory(|| {
                hir::dataflow::reaching_defs::ReachingDefs::new(def_index.clone())
            });
            solver.set_initial_state(initial_defs);

            if let Some(dataflow_result) = solver.solve() {
                let result = hir::dataflow::reaching_defs::ReachingDefsResult::new(dataflow_result);
                results.insert(local_id, Arc::new(result));
            }
        }

        Arc::new(hir::dataflow::reaching_defs::ModuleReachingDefs::new(results))
    }

    fn module_security_state(&self, file_id: FileId) -> Arc<crate::effects::ModuleSecurityState> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut methods = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(c) => c.clone(),
                None => continue,
            };
            if let Some(result) = hir::dataflow::security_state::analyze(cfg, body.clone()) {
                methods.insert(local_id, Arc::new(result));
            }
        }
        let module_level = module_bodies
            .module_code()
            .filter(|body| !body.body_stmts_typed().is_empty())
            .and_then(|body| {
                let cfg = Arc::new(hir::cfg::CfgBuilder::new().build_graph_from_hir(
                    body.body_stmts_typed(),
                    body,
                    None,
                ));
                hir::dataflow::security_state::analyze(cfg, body.clone()).map(Arc::new)
            });
        Arc::new(crate::effects::ModuleSecurityState::from_methods_with_module_level(
            methods,
            module_level,
        ))
    }

    fn module_hir_metrics(&self, file_id: FileId) -> Arc<crate::queries::ModuleHirMetrics> {
        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);
        let mut methods: FxHashMap<u32, Arc<hir::metrics::HirMethodMetrics>> = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let mut metrics = hir::metrics::compute_hir_metrics(body);
            if let Some(lower_result) = module_bodies.lower_result(local_id) {
                metrics.size_lines = lower_result.size_lines;
            }
            methods.insert(local_id, Arc::new(metrics));
        }
        let module_code = module_bodies
            .module_code()
            .map(|body| Arc::new(hir::metrics::compute_hir_metrics(body)));
        Arc::new(crate::queries::ModuleHirMetrics::from_methods(methods, module_code))
    }

    fn method_hir_metrics(&self, method: hir::MethodId) -> Arc<hir::metrics::HirMethodMetrics> {
        let batch = self.module_hir_metrics(method.module.file_id);
        batch
            .get(method.local_id)
            .unwrap_or_else(|| Arc::new(hir::metrics::HirMethodMetrics::default()))
    }

    fn module_cyclomatic(&self, file_id: FileId) -> Arc<crate::queries::ModuleCyclomatic> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);
        let module_metrics = self.module_hir_metrics(file_id);
        let mut methods: FxHashMap<u32, u32> = FxHashMap::default();
        for (local_id, _body) in module_bodies.iter_bodies() {
            let Some(cfg) = module_cfgs.get(local_id) else { continue };
            let base = hir::cfg::cyclomatic_complexity(cfg.as_ref());
            let extras = module_metrics
                .get(local_id)
                .map(|m| m.boolean_ops_count + m.ternary_count)
                .unwrap_or(0);
            methods.insert(local_id, base + extras);
        }
        Arc::new(crate::queries::ModuleCyclomatic::from_methods(methods))
    }

    fn method_cyclomatic(&self, method: hir::MethodId) -> u32 {
        self.module_cyclomatic(method.module.file_id).get(method.local_id)
    }

    fn module_path_terminates(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        let module_id = ModuleId::new(file_id);
        let module_cfgs = self.module_cfgs(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut results = FxHashMap::default();
        for (local_id, body) in module_bodies.iter_bodies() {
            let cfg = match module_cfgs.get(local_id) {
                Some(cfg) => cfg,
                None => continue,
            };

            if let Some(result) = hir::dataflow::path_terminates::analyze_path_terminates(
                body,
                cfg,
                hir::dataflow::path_terminates::PathTerminatesConfig::default(),
                hir::dataflow::DEFAULT_MAX_ITERATIONS,
            ) {
                results.insert(local_id, Arc::new(result));
            }
        }

        Arc::new(hir::dataflow::path_terminates::ModulePathTerminates::new(results))
    }

    fn region_tree(&self, file_id: FileId) -> Arc<hir::RegionTree> {
        let parse = self.parse(file_id);
        Arc::new(hir::lower_regions(&parse.syntax_node()))
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> crate::SdblHirEntries {
        if let Some(ref shared_state) = self.shared_state {
            if let Some(parsed) = shared_state.get_parsed_file(file_id) {
                let configuration = self.configuration();
                return parsed.sdbl_hir(configuration.as_ref());
            }
        }

        let sdbl_queries = self.all_sdbl_in_file(file_id);
        let configuration = self.configuration();

        let mut result = Vec::with_capacity(sdbl_queries.len());
        for (sdbl_expr_id, query_info) in sdbl_queries.iter() {
            if let Some(ref sdbl_ast) = query_info.query_ast {
                let sdbl_package = sdbl_hir::lower_sdbl_to_hir(sdbl_ast, configuration.clone());
                result.push((*sdbl_expr_id, Arc::new(sdbl_package)));
            }
        }

        Arc::new(result)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        let module_id = ModuleId::new(file_id);
        let module_bodies = self.module_bodies(module_id);

        let mut result = Vec::new();

        for (local_id, body) in module_bodies.iter_bodies() {
            for (expr_id, query_info) in body.sdbl_exprs() {
                let sdbl_expr_id = hir::SdblExprId::from_method(local_id, expr_id);
                result.push((sdbl_expr_id, query_info.clone()));
            }
        }

        if let Some(module_code) = module_bodies.module_code() {
            for (expr_id, query_info) in module_code.sdbl_exprs() {
                let sdbl_expr_id = hir::SdblExprId::from_module_code(expr_id);
                result.push((sdbl_expr_id, query_info.clone()));
            }
        }

        result.sort_by_key(|(_, query_info)| query_info.bsl_literal_range.start());

        Arc::new(result)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<hir::ModuleData> {
        let item_tree = self.item_tree(module_id.file_id);
        Arc::new(hir::ModuleData::from_item_tree(module_id, item_tree))
    }

    fn method_docs(&self, method_id: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        let symbol_tree = self.symbol_tree(method_id.module);
        let method = symbol_tree.find_method_by_id(method_id)?;
        method.docs.clone()
    }

    fn variable_docs(&self, variable_id: hir::VariableId) -> Option<Arc<hir::VariableDocs>> {
        let symbol_tree = self.symbol_tree(variable_id.module);
        let variable = symbol_tree.find_variable_by_id(variable_id)?;
        variable.docs.clone()
    }

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        let module_reaching_defs = self.module_reaching_definitions(method_id.module.file_id);
        module_reaching_defs.get(method_id.local_id).cloned()
    }

    fn file_external_refs(&self, _module_id: hir::ModuleId) -> Arc<Vec<hir::ExternalRef>> {
        tracing::debug!("file_external_refs not supported in streaming mode");
        Arc::new(Vec::new())
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
        let module_bodies = self.module_bodies(module_id);
        let body = module_bodies.module_code()?;

        let cfg =
            hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, None);
        let var_index = hir::dataflow::liveness::VariableIndex::from_body(body);

        let result = hir::dataflow::liveness::liveness_analysis_direct(
            body,
            &cfg,
            var_index,
            hir::dataflow::DEFAULT_MAX_ITERATIONS,
        )?;
        Some(Arc::new(result))
    }

    fn resolve_vfs_path(
        &self,
        _source_root_id: base_db::SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        self.global.file_set.file_for_path(vfs_path).copied()
    }

    fn resolve_module_file(&self, relative_uri: &str) -> Option<FileId> {
        let config_root = self.global.config_root.as_ref()?;
        let full_path = config_root.join(relative_uri);
        let vfs_path = vfs::VfsPath::new(full_path.to_string_lossy().into_owned());
        self.global.file_set.file_for_path(&vfs_path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{FileReader, GlobalContext};
    use vfs::{file_set::FileSet, VfsPath};

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
            config_root: None,
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
            config_root: None,
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
            config_root: None,
        });

        let provider = StreamingProvider::new(global);
        let module_id = ModuleId::new(file_id);
        let bodies = provider.module_bodies(module_id);
        assert_eq!(bodies.iter_bodies().count(), 1);
    }

    fn provider_with_source(source: &str) -> (StreamingProvider, FileId) {
        let mut files = FxHashMap::default();
        let file_id = FileId(0);
        files.insert(file_id, source.to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: None,
        });

        (StreamingProvider::new(global), file_id)
    }

    #[test]
    fn module_level_liveness_present_for_module_code() {
        let (provider, file_id) = provider_with_source("А = 1;\nБ = А + 2;");
        let module_id = ModuleId::new(file_id);
        assert!(
            provider.module_level_liveness_analysis(module_id).is_some(),
            "module-level liveness must be computed in streaming mode"
        );
    }

    #[test]
    fn module_level_liveness_present_for_procedure_only_module() {
        let (provider, file_id) =
            provider_with_source("Процедура Тест()\n  А = 1;\nКонецПроцедуры");
        let module_id = ModuleId::new(file_id);
        assert!(
            provider.module_level_liveness_analysis(module_id).is_some(),
            "module-level code region is always lowered, so liveness is computed even with no top-level statements"
        );
    }

    #[test]
    fn test_resolve_module_file_with_config_root() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "Процедура Тест() Экспорт КонецПроцедуры".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(
            file_id,
            VfsPath::new("/project/src/cf/CommonModules/МойМодуль/Ext/Module.bsl"),
        );

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: Some(std::path::PathBuf::from("/project/src/cf")),
        });

        let provider = StreamingProvider::new(global);

        let resolved = provider.resolve_module_file("CommonModules/МойМодуль/Ext/Module.bsl");
        assert_eq!(resolved, Some(file_id));
    }

    #[test]
    fn test_resolve_module_file_without_config_root() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: None,
        });

        let provider = StreamingProvider::new(global);

        let resolved = provider.resolve_module_file("CommonModules/МойМодуль/Ext/Module.bsl");
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_resolve_module_file_not_in_vfs() {
        let file_id = FileId(0);
        let mut files = FxHashMap::default();
        files.insert(file_id, "".to_string());

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/project/src/cf/other.bsl"));

        let global = Arc::new(GlobalContext {
            configuration: None,
            symbol_trees: FxHashMap::default(),
            workspace_symbols: Arc::new(WorkspaceSymbols::default()),
            module_index: Arc::new(ModuleIndex::new()),
            file_set: Arc::new(file_set),
            file_reader: FileReader::in_memory(files),
            config_root: Some(std::path::PathBuf::from("/project/src/cf")),
        });

        let provider = StreamingProvider::new(global);

        let resolved =
            provider.resolve_module_file("CommonModules/НесуществующийМодуль/Ext/Module.bsl");
        assert_eq!(resolved, None);
    }
}
