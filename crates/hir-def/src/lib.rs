pub mod body;
pub mod call_graph;
pub mod catch_class;
pub mod conditional_tree;
pub mod configs;
pub mod docs;
pub mod hir;
pub mod item_tree;
pub mod method_body;
pub mod metrics;
pub mod module_index;
pub mod module_structure;
pub mod name;
pub mod name_usage_index;
pub mod path;
pub mod queries;
pub mod region_tree;
pub mod resolver;
pub mod scope;
pub mod sdbl_cache;
pub mod symbol_tree;
pub mod ty;
pub mod type_ref;
pub mod workspace;
pub mod workspace_index;

use std::sync::Arc;

use base_db::SourceRootId;
use vfs::FileId;

pub use body::{
    lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, DeprecatedKind8312,
    ExistingBindingKind, ExternalRef, LowerResult, ManagerType, RedundantAccessKind,
};
pub use hir::{BinaryOp, Binding, Expr, IfStmt, Literal, Stmt, UnaryOp};

pub use cfg_types::{BindingId, ExprId, IdConversion, StmtId};

pub use conditional_tree::{ConditionalData, ConditionalIdx, ConditionalKind, ConditionalTree};
pub use configs::ConfigsDatabase;
pub use item_tree::ItemTree;
pub use module_index::ModuleIndex;
pub use name::Name;
pub use name_usage_index::{
    file_name_usage_query, normalize_name, source_root_name_usage_query, FileNameUsage,
    SourceRootNameUsage,
};
pub use path::{PathResolution, QualifiedName};
pub use region_tree::{RegionData, RegionIdx, RegionTree};
pub use sdbl_cache::{all_sdbl_in_file_query, sdbl_hir_for_file_query, SdblHirEntries, SdblInFile};
pub use symbol_tree::{MethodSymbol, ParamSymbol, SymbolTree, VariableSymbol};
pub use ty::FunctionSignature;
pub use type_ref::{BuiltinTypeRef, TypeRef};
pub use workspace::{is_bsl_source, CommonModuleInfo, WorkspaceSymbols};
pub use workspace_index::{SymbolInfo, SymbolKind, WorkspaceIndex};

pub use method_body::{method_body_query, method_body_with_source_map_query};
pub use queries::{
    conditional_tree_query, file_dependencies_query, file_external_refs_query, item_tree_query,
    module_bodies_query, module_call_summary_query, module_data_query, module_index_query,
    region_tree_query, resolved_module_summary_query, symbol_tree_query,
    workspace_call_graph_query, workspace_index_query, workspace_symbols_query,
};

#[salsa::db]
pub trait DefDatabase: base_db::RootQueryDb {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree>;

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree>;

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData>;

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    fn method_body(&self, method: MethodIdInput<'_>) -> Arc<body::Body>;

    fn method_body_with_source_map(
        &self,
        method: MethodIdInput<'_>,
    ) -> Arc<(body::Body, body::BodySourceMap)>;

    fn module_metadata(&self, module_id: ModuleId) -> Arc<ModuleMetadata>;

    fn module_call_summary(&self, module_id: ModuleId) -> Arc<call_graph::ModuleCallSummary>;

    fn method_docs(&self, method: MethodId) -> Option<Arc<crate::docs::MethodDocs>>;

    fn variable_docs(&self, variable: VariableId) -> Option<Arc<crate::docs::VariableDocs>>;

    fn workspace_symbols(&self, source_root_id: SourceRootId) -> Arc<WorkspaceSymbols>;

    fn workspace_index(
        &self,
        source_root_id: SourceRootId,
    ) -> Arc<crate::workspace_index::WorkspaceIndex>;

    fn name_usage_index(
        &self,
        source_root_id: SourceRootId,
    ) -> Arc<crate::name_usage_index::SourceRootNameUsage>;

    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<ExternalRef>>;

    fn module_index(&self, source_root_id: SourceRootId) -> Arc<ModuleIndex>;

    fn file_dependencies(&self, module_id: ModuleId) -> Arc<Vec<FileId>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId {
    pub file_id: FileId,
}

impl ModuleId {
    pub fn new(file_id: FileId) -> Self {
        Self { file_id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MethodId {
    pub module: ModuleId,
    pub local_id: u32,
}

#[salsa::interned(debug)]
pub struct MethodIdInput {
    pub method_id: MethodId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariableId {
    pub module: ModuleId,
    pub local_id: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DefWithBodyId {
    Method(u32),
    ModuleCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SdblExprId {
    pub owner: DefWithBodyId,
    pub expr_id: ExprId,
}

impl SdblExprId {
    pub fn from_method(local_id: u32, expr_id: ExprId) -> Self {
        Self { owner: DefWithBodyId::Method(local_id), expr_id }
    }

    pub fn from_module_code(expr_id: ExprId) -> Self {
        Self { owner: DefWithBodyId::ModuleCode, expr_id }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleData {
    pub file_id: FileId,
    pub name: Option<Name>,
    pub procedures: Vec<MethodId>,
    pub functions: Vec<MethodId>,
    pub variables: Vec<VariableId>,
}

impl ModuleData {
    pub fn from_item_tree(module_id: ModuleId, tree: Arc<ItemTree>) -> Self {
        let mut procedures = Vec::new();
        let mut functions = Vec::new();
        let mut variables = Vec::new();

        for (idx, item) in tree.top_level_items().iter().enumerate() {
            match item {
                item_tree::ModItem::Procedure(_) => {
                    procedures.push(MethodId { module: module_id, local_id: idx as u32 });
                }
                item_tree::ModItem::Function(_) => {
                    functions.push(MethodId { module: module_id, local_id: idx as u32 });
                }
                item_tree::ModItem::Variable(_) => {
                    variables.push(VariableId { module: module_id, local_id: idx as u32 });
                }
            }
        }

        ModuleData { file_id: module_id.file_id, name: None, procedures, functions, variables }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleVarDecl {
    pub name: String,
    pub range: text_size::TextRange,
    pub is_export: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionContext {
    Server,
    ServerCall,
    Client,
    ClientServer,
    ExternalConnection,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub module_type: bsl_metadata::ModuleType,
    pub execution_context: Option<ExecutionContext>,
    pub common_module: Option<Arc<bsl_metadata::CommonModule>>,
    pub mdo: Option<Arc<bsl_metadata::MetadataObject>>,
    pub register: Option<Arc<bsl_metadata::Register>>,
    pub form: Option<Arc<bsl_metadata::Form>>,
    pub http_service: Option<Arc<bsl_metadata::HTTPService>>,
    pub web_service: Option<Arc<bsl_metadata::WebService>>,
}

impl ModuleMetadata {
    pub fn unknown(module_type: bsl_metadata::ModuleType) -> Self {
        Self {
            module_type,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
            http_service: None,
            web_service: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleBodies {
    bodies: indexmap::IndexMap<u32, body::LowerResult>,
    all_diagnostics: Vec<(MethodId, BodyDiagnostic)>,
    module_vars: Vec<ModuleVarDecl>,
    module_code: Option<body::LowerResult>,
}

impl ModuleBodies {
    pub fn new() -> Self {
        Self {
            bodies: indexmap::IndexMap::new(),
            all_diagnostics: Vec::new(),
            module_vars: Vec::new(),
            module_code: None,
        }
    }

    pub fn from_parse(parse: &syntax::Parse<syntax::SyntaxNode>, module_id: ModuleId) -> Self {
        let root = parse.syntax_node();
        Self::lower_from_root(&root, module_id, None)
    }

    fn lower_from_root(
        root: &syntax::SyntaxNode,
        module_id: ModuleId,
        line_index: Option<std::sync::Arc<line_index::LineIndex>>,
    ) -> Self {
        use rustc_hash::FxHashSet;
        use syntax::SyntaxKind;

        let mut result = ModuleBodies::new();
        let mut method_nodes: Vec<(u32, syntax::SyntaxNode, bool)> = Vec::new();
        let mut top_level_idx: u32 = 0;

        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::VAR_DEF => {
                    let is_inside_method = node.ancestors().any(|n| {
                        matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF)
                    });
                    if !is_inside_method {
                        collect_module_vars(&node, &mut result.module_vars);
                        top_level_idx += 1;
                    }
                }
                SyntaxKind::PROCEDURE_DEF => {
                    method_nodes.push((top_level_idx, node, false));
                    top_level_idx += 1;
                }
                SyntaxKind::FUNCTION_DEF => {
                    method_nodes.push((top_level_idx, node, true));
                    top_level_idx += 1;
                }
                _ => {}
            }
        }

        {
            let mut seen_names: FxHashSet<String> = FxHashSet::default();
            result.module_vars.retain(|var| {
                let key = var.name.to_lowercase();
                seen_names.insert(key)
            });
        }

        for (item_tree_idx, node, is_function) in method_nodes.into_iter() {
            let lower_result =
                body::lower_method_with_externals(&node, is_function, line_index.clone());

            let method_id = MethodId { module: module_id, local_id: item_tree_idx };
            for diag in &lower_result.diagnostics {
                result.all_diagnostics.push((method_id, diag.clone()));
            }

            result.bodies.insert(item_tree_idx, lower_result);
        }

        let module_code_result = body::lower_module_code(root, line_index);
        let module_method_id = MethodId { module: module_id, local_id: u32::MAX };
        for diag in &module_code_result.diagnostics {
            result.all_diagnostics.push((module_method_id, diag.clone()));
        }
        result.module_code = Some(module_code_result);

        result
    }

    pub fn body(&self, local_id: u32) -> Option<&Body> {
        self.bodies.get(&local_id).map(|r| &r.body)
    }

    pub fn source_map(&self, local_id: u32) -> Option<&BodySourceMap> {
        self.bodies.get(&local_id).map(|r| &r.source_map)
    }

    pub fn diagnostics(&self, local_id: u32) -> Option<&[BodyDiagnostic]> {
        self.bodies.get(&local_id).map(|r| r.diagnostics.as_slice())
    }

    pub fn all_diagnostics(&self) -> &[(MethodId, BodyDiagnostic)] {
        &self.all_diagnostics
    }

    pub fn lower_result(&self, local_id: u32) -> Option<&body::LowerResult> {
        self.bodies.get(&local_id)
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    pub fn iter_bodies(&self) -> impl Iterator<Item = (u32, &Body)> {
        self.bodies.iter().map(|(local_id, lower_result)| (*local_id, &lower_result.body))
    }

    pub fn method_bodies(&self) -> impl Iterator<Item = (u32, &Body, &BodySourceMap)> {
        self.bodies.iter().map(|(local_id, lower_result)| {
            (*local_id, &lower_result.body, &lower_result.source_map)
        })
    }

    pub fn iter_lower_results(&self) -> impl Iterator<Item = (u32, &body::LowerResult)> {
        self.bodies.iter().map(|(local_id, lower_result)| (*local_id, lower_result))
    }

    pub fn module_code(&self) -> Option<&Body> {
        self.module_code.as_ref().map(|r| &r.body)
    }

    pub fn module_code_result(&self) -> Option<&body::LowerResult> {
        self.module_code.as_ref()
    }

    pub fn module_vars(&self) -> &[ModuleVarDecl] {
        &self.module_vars
    }
}

impl Default for ModuleBodies {
    fn default() -> Self {
        Self::new()
    }
}

pub fn compute_execution_context(common_module: &bsl_metadata::CommonModule) -> ExecutionContext {
    if common_module.is_server_call() {
        return ExecutionContext::ServerCall;
    }

    let is_server = common_module.is_server();
    let is_client_managed = common_module.is_client_managed_application();
    let is_external = common_module.is_external_connection();

    if is_server && is_client_managed && !is_external {
        return ExecutionContext::ClientServer;
    }

    if is_server && !is_client_managed && !is_external {
        return ExecutionContext::Server;
    }

    if is_client_managed && !is_server && !is_external {
        return ExecutionContext::Client;
    }

    if is_external && !is_server {
        return ExecutionContext::ExternalConnection;
    }

    ExecutionContext::Unknown
}

pub fn lower_module_bodies(db: &dyn base_db::RootQueryDb, module_id: ModuleId) -> ModuleBodies {
    let parse = db.parse(module_id.file_id);
    let root = parse.syntax_node();

    let file_text_input = db.file_text_input(module_id.file_id);
    let file_text = file_text_input.text(db);
    let line_index = std::sync::Arc::new(line_index::LineIndex::new(&file_text));

    ModuleBodies::lower_from_root(&root, module_id, Some(line_index))
}

fn collect_module_vars(var_def: &syntax::SyntaxNode, vars: &mut Vec<ModuleVarDecl>) {
    use syntax::SyntaxKind;

    let has_export = var_def
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_EXPORT);

    for token in var_def.children_with_tokens().filter_map(|el| el.into_token()) {
        if token.kind() == SyntaxKind::IDENT {
            vars.push(ModuleVarDecl {
                name: token.text().to_string(),
                range: token.text_range(),
                is_export: has_export,
            });
        }
    }
}

#[cfg(test)]
mod module_bodies_order_tests {
    use super::*;

    fn lower(code: &str) -> ModuleBodies {
        let parse = parser::parse(code);
        let module_id = ModuleId::new(vfs::FileId(0));
        ModuleBodies::from_parse(&parse, module_id)
    }

    #[test]
    fn iter_bodies_order_matches_item_tree_index() {
        let code = "\
Процедура Первая() КонецПроцедуры
Функция Вторая() КонецФункции
Процедура Третья() КонецПроцедуры
";
        let bodies = lower(code);
        let local_ids: Vec<u32> = bodies.iter_bodies().map(|(id, _)| id).collect();
        let mut sorted = local_ids.clone();
        sorted.sort();
        assert_eq!(local_ids, sorted, "insertion order must equal local_id-sorted order");
        assert!(local_ids.windows(2).all(|w| w[0] < w[1]), "ids strictly increasing");
    }

    #[test]
    fn iter_bodies_stable_across_repeated_calls() {
        let code = "\
Процедура Альфа() КонецПроцедуры
Процедура Бета() КонецПроцедуры
Процедура Гамма() КонецПроцедуры
Процедура Дельта() КонецПроцедуры
";
        let bodies = lower(code);
        let first: Vec<u32> = bodies.iter_bodies().map(|(id, _)| id).collect();
        for _ in 0..5 {
            let again: Vec<u32> = bodies.iter_bodies().map(|(id, _)| id).collect();
            assert_eq!(first, again, "iteration order must be stable across calls");
        }
    }

    #[test]
    fn method_bodies_and_lower_results_share_order() {
        let code = "\
Процедура А() КонецПроцедуры
Перем М;
Функция Б() КонецФункции
Процедура В() КонецПроцедуры
";
        let bodies = lower(code);
        let from_iter: Vec<u32> = bodies.iter_bodies().map(|(id, _)| id).collect();
        let from_method_bodies: Vec<u32> = bodies.method_bodies().map(|(id, _, _)| id).collect();
        let from_lower_results: Vec<u32> = bodies.iter_lower_results().map(|(id, _)| id).collect();
        assert_eq!(from_iter, from_method_bodies);
        assert_eq!(from_iter, from_lower_results);
    }
}
