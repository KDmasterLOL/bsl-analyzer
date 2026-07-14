pub mod body;
pub mod call_graph;
pub mod call_hierarchy_index;
pub mod catch_class;
pub mod common_module_ref;
pub mod conditional_tree;
pub mod configs;
pub mod docs;
pub mod effective_module;
pub mod execution_env;
pub mod extension_merge;
pub mod graph_index;
pub(crate) mod heap_estimate;
pub mod hir;
pub mod item_tree;
pub mod method_body;
pub mod metrics;
pub mod module_index;
pub mod module_structure;
pub mod name;
pub mod name_usage_index;
pub mod path;
pub mod preproc_condition;
pub mod queries;
pub mod region_tree;
pub mod resolver;
pub mod scope;
pub mod sdbl_cache;
pub mod symbol_tree;
pub mod ty;
pub mod type_ref;
pub mod weaving;
pub mod workspace;
pub mod workspace_index;

use std::sync::Arc;
use stdx::case::CaseExt;

use base_db::SourceRootId;
use vfs::FileId;

pub use body::{
    lower_method, lower_module_code, Body, BodyDiagnostic, BodySourceMap, DeprecatedKind8312,
    ExistingBindingKind, ExternalRef, LowerResult, ManagerType, RedundantAccessKind,
};
pub use call_hierarchy_index::{CallHierarchyReverseIndex, MethodCallPair};
pub use hir::{BinaryOp, Binding, Expr, IfStmt, Literal, Stmt, UnaryOp};

pub use cfg_types::{BindingId, ExprId, IdConversion, StmtId};

pub use conditional_tree::{ConditionalData, ConditionalIdx, ConditionalKind, ConditionalTree};
pub use configs::ConfigsDatabase;
pub use item_tree::ItemTree;
pub use module_index::{
    module_key_for_path, parse_form_module_path, FormKey, ModuleIndex, ModuleKey,
};
pub use name::Name;
pub use name_usage_index::{
    file_name_offsets_query, file_name_usage_query, normalize_match_name, normalize_name,
    source_root_name_usage_query, FileNameOffsets, FileNameUsage, SourceRootNameUsage,
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
    method_outbound_facts, module_bodies_query, module_call_summary_query, module_data_query,
    module_index_query, region_tree_query, resolved_module_summary_query,
    set_module_bodies_lru_sweep_mode, symbol_tree_query, workspace_call_graph_query,
    workspace_index_query, workspace_symbols_query, ManagerRef, MethodOutboundFacts,
};

#[salsa::db]
pub trait DefDatabase: base_db::RootQueryDb {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree>;

    /// Borrowed variant of [`item_tree`](Self::item_tree) for read-only paths:
    /// no `Arc` refcount traffic per read (`.clone()` the result if ownership
    /// is needed after all).
    fn item_tree_ref(&self, file_id: FileId) -> &Arc<ItemTree>;

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree>;

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree>;

    /// Borrowed variant of [`conditional_tree`](Self::conditional_tree); see
    /// [`item_tree_ref`](Self::item_tree_ref).
    fn conditional_tree_ref(&self, file_id: FileId) -> &Arc<ConditionalTree>;

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData>;

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree>;

    /// Borrowed variant of [`symbol_tree`](Self::symbol_tree); see
    /// [`item_tree_ref`](Self::item_tree_ref).
    fn symbol_tree_ref(&self, module_id: ModuleId) -> &Arc<SymbolTree>;

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies>;

    /// Borrowed variant of [`module_bodies`](Self::module_bodies); see
    /// [`item_tree_ref`](Self::item_tree_ref).
    fn module_bodies_ref(&self, module_id: ModuleId) -> &Arc<ModuleBodies>;

    fn method_body(&self, method: MethodIdInput<'_>) -> Arc<body::Body>;

    /// Borrowed variant of [`method_body`](Self::method_body); see
    /// [`item_tree_ref`](Self::item_tree_ref).
    fn method_body_ref<'db>(&'db self, method: MethodIdInput<'db>) -> &'db Arc<body::Body>;

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

    fn file_name_offsets(&self, file_id: FileId) -> Arc<crate::name_usage_index::FileNameOffsets>;

    /// Borrowed variant of [`file_name_offsets`](Self::file_name_offsets); see
    /// [`item_tree_ref`](Self::item_tree_ref).
    fn file_name_offsets_ref(
        &self,
        file_id: FileId,
    ) -> &Arc<crate::name_usage_index::FileNameOffsets>;

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
    #[returns(copy)]
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
    pub integration_service: Option<Arc<bsl_metadata::IntegrationService>>,
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
            integration_service: None,
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

    /// Like [`Self::from_parse`] but with a line index built from `source_text`, so
    /// line-dependent lowering (method size, complexity metrics) matches the
    /// disk-backed [`lower_module_bodies`] path. Used when the parse comes from
    /// assembled text that has no `file_id` to read through `db.file_text`.
    pub fn from_parse_with_text(
        parse: &syntax::Parse<syntax::SyntaxNode>,
        module_id: ModuleId,
        source_text: &str,
    ) -> Self {
        let root = parse.syntax_node();
        let line_index = std::sync::Arc::new(line_index::LineIndex::new(source_text));
        Self::lower_from_root(&root, module_id, Some(line_index))
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
                        top_level_idx += node
                            .children_with_tokens()
                            .filter_map(|element| element.into_token())
                            .filter(|token| token.kind() == SyntaxKind::IDENT)
                            .count() as u32;
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
                let key = var.name.fold_lower();
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

    /// Approximate live heap bytes for Salsa's `memory_usage` report. This is the
    /// heavy lowered-HIR memo: it sums each per-method (and module-level)
    /// [`body::Body`] and its source map, the diagnostics and external-ref side
    /// tables, the `IndexMap` backbone, and the module-var declarations. The
    /// `BodyDiagnostic` enum's owned `String` payloads are counted at element
    /// granularity only (the variant strings are not summed), a small undercount.
    pub fn estimated_heap(&self) -> usize {
        use crate::heap_estimate::{map_table_bytes, vec_bytes};

        let lower_result_heap = |r: &body::LowerResult| {
            let mut b =
                crate::body::body_heap(&r.body) + crate::body::source_map_heap(&r.source_map);
            b += vec_bytes::<BodyDiagnostic>(r.diagnostics.len());
            b += map_table_bytes::<String, ()>(r.referenced_externals.len());
            for s in &r.referenced_externals {
                b += s.capacity();
            }
            b += vec_bytes::<body::ExternalRef>(r.external_refs.len());
            for ext in &r.external_refs {
                b += external_ref_name_heap(ext);
            }
            b
        };

        // `IndexMap` backbone: a `Vec` of entries plus a hashbrown index table.
        let mut bytes = vec_bytes::<(u32, body::LowerResult)>(self.bodies.len());
        bytes += map_table_bytes::<u32, usize>(self.bodies.len());
        for r in self.bodies.values() {
            bytes += lower_result_heap(r);
        }

        bytes += vec_bytes::<(MethodId, BodyDiagnostic)>(self.all_diagnostics.len());
        bytes += vec_bytes::<ModuleVarDecl>(self.module_vars.len());
        for var in &self.module_vars {
            bytes += var.name.capacity();
        }
        if let Some(module_code) = &self.module_code {
            bytes += lower_result_heap(module_code);
        }

        bytes
    }
}

/// Heap bytes owned by an [`body::ExternalRef`]'s `Name` fields (their `SmolStr`
/// payloads when not inlined).
fn external_ref_name_heap(ext: &body::ExternalRef) -> usize {
    use crate::heap_estimate::name_bytes;
    match ext {
        body::ExternalRef::QualifiedCall { receiver, method, .. } => {
            name_bytes(receiver) + name_bytes(method)
        }
        body::ExternalRef::ManagerAccess { object_name, method, .. } => {
            name_bytes(object_name) + method.as_ref().map_or(0, name_bytes)
        }
    }
}

impl Default for ModuleBodies {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify a common module's effective client/server dispatch capability from its
/// metadata flags. This is the *dispatch* model: the result is consumed only through
/// [`call_graph::MethodDispatch::from_execution_context`], which collapses it to
/// can-run-on-client / can-run-on-server.
///
/// Deliberately distinct from the richer naming-rule predicates in
/// `ide-diagnostics::common_module_helpers` (`is_client`/`is_server`/…): those take a
/// configuration-level `ordinary_app_support` flag and gate on the legacy
/// `ClientOrdinaryApplication`, because they answer "what should this module be *named*".
/// Here we answer "where does its code *run*", for which the managed-application boundary
/// (`ClientManagedApplication` for client, `Server`/`ExternalConnection` for server) is
/// what matters. The two never decide the same thing for the same module. Threading
/// `ordinary_app_support` to fully unify them is a worthwhile follow-up but out of scope.
pub fn compute_execution_context(common_module: &bsl_metadata::CommonModule) -> ExecutionContext {
    // `ServerCall` (вызов сервера) executes on the server regardless of which client
    // contexts may also be set, so it short-circuits. Real 1C never emits `ServerCall`
    // alongside client flags; for malformed XML this stays on the safe (server) side.
    if common_module.is_server_call() {
        return ExecutionContext::ServerCall;
    }

    // Capability model, not mutually-exclusive buckets: a module compiles into
    // every context whose flag is set, and `ExternalConnection` is a *server-side*
    // (non-interactive) context — not a third axis. `ClientOrdinaryApplication` (the
    // legacy thick client) is intentionally NOT treated as client-capable: server
    // modules like `ОбщегоНазначения` carry `ClientOrdinaryApplication=true`, so
    // honouring it would wrongly make them client-capable. The previous `!is_external`
    // guards collapsed the common `Server + ExternalConnection` server module to
    // `Unknown`, which then fell back to the client-only annotation default.
    let is_server = common_module.is_server();
    let is_external = common_module.is_external_connection();
    let runs_on_client = common_module.is_client_managed_application();
    let runs_on_server = is_server || is_external;

    match (runs_on_client, runs_on_server) {
        (true, true) => ExecutionContext::ClientServer,
        // A non-interactive server-side module: `Server` when the explicit server
        // flag is set, otherwise external-connection-only.
        (false, true) if is_server => ExecutionContext::Server,
        (false, true) => ExecutionContext::ExternalConnection,
        (true, false) => ExecutionContext::Client,
        (false, false) => ExecutionContext::Unknown,
    }
}

pub fn lower_module_bodies(db: &dyn base_db::RootQueryDb, module_id: ModuleId) -> ModuleBodies {
    let parse = db.parse_ref(module_id.file_id);
    let root = parse.syntax_node();

    let file_text = db.file_text(module_id.file_id);
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

    #[test]
    fn method_after_multi_name_var_decl_uses_item_tree_local_id() {
        let bodies = lower("Перем A, B; Процедура P() КонецПроцедуры");

        assert!(bodies.body(2).is_some());
        assert!(bodies.body(1).is_none());
    }
}

#[cfg(test)]
mod execution_context_tests {
    use super::*;
    use crate::call_graph::MethodDispatch;

    /// Flags from the named real-world shape; everything not mentioned is `false`.
    fn cm(
        server: bool,
        external: bool,
        client_managed: bool,
        client_ordinary: bool,
        server_call: bool,
    ) -> bsl_metadata::CommonModule {
        bsl_metadata::CommonModule::builder()
            .name("Модуль")
            .server(server)
            .external_connection(external)
            .client_managed_application(client_managed)
            .client_ordinary_application(client_ordinary)
            .server_call(server_call)
            .build()
    }

    #[test]
    fn server_module_with_external_connection_is_server() {
        // The `ОбщегоНазначения` shape: Server + ExternalConnection + ClientOrdinary,
        // no managed client. Must be server-only — the `!is_external` guard used to
        // collapse this to `Unknown` → client-only dispatch.
        let ctx = compute_execution_context(&cm(true, true, false, true, false));
        assert_eq!(ctx, ExecutionContext::Server);
        let d = MethodDispatch::from_execution_context(ctx).unwrap();
        assert!(d.is_server_only(), "server common module must dispatch server-only");
    }

    #[test]
    fn server_without_external_is_still_server() {
        assert_eq!(
            compute_execution_context(&cm(true, false, false, false, false)),
            ExecutionContext::Server
        );
    }

    #[test]
    fn managed_client_and_server_is_client_server() {
        assert_eq!(
            compute_execution_context(&cm(true, true, true, true, false)),
            ExecutionContext::ClientServer
        );
    }

    #[test]
    fn managed_client_plus_external_runs_on_both() {
        // ExternalConnection is a server-side context, so a managed client that also
        // compiles for external connection is client+server, not client-only.
        assert_eq!(
            compute_execution_context(&cm(false, true, true, false, false)),
            ExecutionContext::ClientServer
        );
    }

    #[test]
    fn managed_client_only_is_client() {
        assert_eq!(
            compute_execution_context(&cm(false, false, true, true, false)),
            ExecutionContext::Client
        );
    }

    #[test]
    fn external_connection_only_is_external() {
        let ctx = compute_execution_context(&cm(false, true, false, false, false));
        assert_eq!(ctx, ExecutionContext::ExternalConnection);
        assert!(MethodDispatch::from_execution_context(ctx).unwrap().is_server_only());
    }

    #[test]
    fn server_call_takes_precedence() {
        assert_eq!(
            compute_execution_context(&cm(true, false, false, false, true)),
            ExecutionContext::ServerCall
        );
    }

    #[test]
    fn server_call_wins_even_with_client_flags() {
        // Malformed/mixed shape: ServerCall must still short-circuit to server-side.
        assert_eq!(
            compute_execution_context(&cm(true, true, true, true, true)),
            ExecutionContext::ServerCall
        );
    }

    #[test]
    fn ordinary_client_only_is_unknown_not_client() {
        // Legacy thick-client flag alone is intentionally not client-capable in the
        // dispatch model — it must not be classified as `Client` (which would force
        // module-level client-only dispatch); it stays `Unknown` and defers to the
        // per-method annotation default.
        assert_eq!(
            compute_execution_context(&cm(false, false, false, true, false)),
            ExecutionContext::Unknown
        );
    }

    #[test]
    fn all_flags_false_is_unknown() {
        let ctx = compute_execution_context(&cm(false, false, false, false, false));
        assert_eq!(ctx, ExecutionContext::Unknown);
        assert!(MethodDispatch::from_execution_context(ctx).is_none());
    }
}
