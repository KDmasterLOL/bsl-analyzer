//! High-level Intermediate Representation for bsl-analyzer.
//!
//! This crate provides a high-level API for semantic analysis.

mod definition;

pub use definition::Definition;

// Re-export core types
pub use hir_def::{BindingId, ExprId, IdConversion, ModuleMetadata, Name, PathResolution, StmtId};
pub use hir_def::{ExecutionContext, QualifiedName};
pub use hir_def::{MethodId, ModuleData, ModuleId, VariableId};
pub use hir_def::{RedundantAccessKind, SdblExprId};

// Re-export HIR body types for diagnostics
pub use hir_def::body::{DeprecatedKind8312, ExternalRef, MagicNumberContext};
pub use hir_def::{Body, BodyDiagnostic, BodySourceMap, ModuleBodies};

// Re-export HIR expression/statement types
pub use hir_def::hir::{BinaryOp, Expr, ExprIdx, Literal, Stmt, UnaryOp};

// Re-export item tree types
pub use hir_def::item_tree::{Annotation, AnnotationKind, Function, ModItem, Param, Procedure};

// Re-export tree types
pub use hir_def::region_tree::{RegionIdx, RegionTree};
pub use hir_def::symbol_tree::MethodSymbol;
pub use hir_def::{ConditionalTree, ItemTree, ModuleIndex, SymbolTree, WorkspaceSymbols};

// Re-export database and resolution
pub use hir_def::resolver::Resolver;
pub use hir_def::scope::{ExprScopes, ScopeDef};
pub use hir_def::DefDatabase;

// Re-export documentation types
pub use hir_def::docs::{MethodDocs, ParameterDoc};

// Re-export analysis modules
pub use hir_def::cognitive_complexity;
pub use hir_def::cyclomatic_complexity;

/// CFG analysis — stable public API
pub mod cfg {
    pub use ::cfg::{
        BasicBlockVertex, CfgBuilder, CfgEdgeType, CfgVertex, ConditionalVertex, ControlFlowGraph,
        ForEachLoopVertex, ForLoopVertex, LabelVertex, ModuleCfgs, NodeIndex,
        PreprocConditionVertex, TryExceptVertex, WhileLoopVertex,
    };
}

/// Dataflow analysis — stable public API
pub mod dataflow {
    pub use ::dataflow::{DataflowResult, DataflowSolver, Direction, DEFAULT_MAX_ITERATIONS};

    pub mod liveness {
        pub use ::dataflow::liveness::{
            liveness_analysis_direct, Liveness, LivenessTransfer, ModuleLiveness, VariableIndex,
        };
    }

    pub mod reaching_defs {
        pub use ::dataflow::reaching_defs::{
            DefSite, Definition, DefinitionIndex, ModuleReachingDefs, ReachingDefs,
            ReachingDefsResult, ReachingDefsTransfer,
        };
    }
}

// Re-export additional types needed by ide-db
pub use hir_def::compute_execution_context;
pub use hir_def::region_tree::lower_regions;
pub use hir_def::resolver::Resolution;
pub use hir_def::workspace_index::WorkspaceIndex;
pub use hir_def::MethodIdInput;

// Re-export Salsa query functions
pub use hir_def::{
    conditional_tree_query, file_dependencies_query, file_external_refs_query, item_tree_query,
    module_bodies_query, module_data_query, module_index_query, region_tree_query,
    symbol_tree_query, workspace_index_query, workspace_symbols_query,
};

// Re-export hir-ty types and queries
pub use hir_ty::db::HirDatabase;
pub use hir_ty::infer::{infer_query, type_of_expr_query};
pub use hir_ty::{InferenceResult, Ty};

use syntax::{ast::AstNode, TextRange};
use vfs::FileId;

/// A module in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Module<'db, DB> {
    db: &'db DB,
    id: ModuleId,
}

impl<'db, DB: DefDatabase> Module<'db, DB> {
    pub(crate) fn new(db: &'db DB, id: ModuleId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> ModuleId {
        self.id
    }

    /// Get all procedures in this module.
    pub fn procedures(&self) -> Vec<Method<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.procedures.iter().map(|&id| Method::new(self.db, id)).collect()
    }

    /// Get all functions in this module.
    pub fn functions(&self) -> Vec<Method<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.functions.iter().map(|&id| Method::new(self.db, id)).collect()
    }

    /// Get all module variables in this module.
    pub fn variables(&self) -> Vec<Variable<'db, DB>> {
        let data = self.db.module_data(self.id);
        data.variables.iter().map(|&id| Variable::new(self.db, id)).collect()
    }
}

/// A method (procedure or function) in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Method<'db, DB> {
    db: &'db DB,
    id: MethodId,
}

pub(crate) struct MethodInfo {
    pub(crate) name: Name,
    pub(crate) is_export: bool,
    pub(crate) is_function: bool,
    pub(crate) source_range: TextRange,
    pub(crate) name_range: TextRange,
}

pub(crate) fn get_method_info(id: &MethodId, db: &dyn DefDatabase) -> Option<MethodInfo> {
    let tree = db.item_tree(id.module.file_id);
    let item = tree.top_level_items().get(id.local_id as usize)?;
    match item {
        hir_def::item_tree::ModItem::Procedure(proc_idx) => {
            let proc = tree.procedure(*proc_idx);
            Some(MethodInfo {
                name: proc.name.clone(),
                is_export: proc.is_export,
                is_function: false,
                source_range: proc.source_range,
                name_range: proc.name_range,
            })
        }
        hir_def::item_tree::ModItem::Function(func_idx) => {
            let func = tree.function(*func_idx);
            Some(MethodInfo {
                name: func.name.clone(),
                is_export: func.is_export,
                is_function: true,
                source_range: func.source_range,
                name_range: func.name_range,
            })
        }
        _ => None,
    }
}

impl<'db, DB: DefDatabase> Method<'db, DB> {
    /// Create a new Method from database and method ID.
    pub fn new(db: &'db DB, id: MethodId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> MethodId {
        self.id
    }

    fn method_info(&self) -> Option<MethodInfo> {
        get_method_info(&self.id, self.db)
    }

    /// Get the method name.
    pub fn name(&self) -> Name {
        self.method_info().map_or_else(Name::missing, |i| i.name)
    }

    /// Check if this is an export method.
    pub fn is_export(&self) -> bool {
        self.method_info().is_some_and(|i| i.is_export)
    }

    /// Check if this is a function (as opposed to a procedure).
    pub fn is_function(&self) -> bool {
        self.method_info().is_some_and(|i| i.is_function)
    }

    /// Get the source range of this method.
    pub fn source_range(&self) -> Option<TextRange> {
        self.method_info().map(|i| i.source_range)
    }

    /// Get the name range of this method.
    ///
    /// Returns the text range of the method name (identifier only).
    pub fn name_range(&self) -> Option<TextRange> {
        self.method_info().map(|i| i.name_range)
    }

    /// Get parsed documentation for this method.
    pub fn docs(&self) -> Option<std::sync::Arc<hir_def::docs::MethodDocs>> {
        self.db.method_docs(self.id)
    }
}

/// A variable in the HIR.
#[derive(Debug, Clone, Copy)]
pub struct Variable<'db, DB> {
    db: &'db DB,
    id: VariableId,
}

pub(crate) struct VariableInfo {
    pub(crate) name: Name,
    pub(crate) is_export: bool,
    pub(crate) source_range: TextRange,
}

pub(crate) fn get_variable_info(id: &VariableId, db: &dyn DefDatabase) -> Option<VariableInfo> {
    let tree = db.item_tree(id.module.file_id);
    let item = tree.top_level_items().get(id.local_id as usize)?;
    if let hir_def::item_tree::ModItem::Variable(var_idx) = item {
        let var = tree.variable(*var_idx);
        Some(VariableInfo {
            name: var.name.clone(),
            is_export: var.is_export,
            source_range: var.source_range,
        })
    } else {
        None
    }
}

impl<'db, DB: DefDatabase> Variable<'db, DB> {
    pub(crate) fn new(db: &'db DB, id: VariableId) -> Self {
        Self { db, id }
    }

    pub fn id(&self) -> VariableId {
        self.id
    }

    fn variable_info(&self) -> Option<VariableInfo> {
        get_variable_info(&self.id, self.db)
    }

    /// Get the variable name.
    pub fn name(&self) -> Name {
        self.variable_info().map_or_else(Name::missing, |i| i.name)
    }

    /// Check if this is an export variable.
    pub fn is_export(&self) -> bool {
        self.variable_info().is_some_and(|i| i.is_export)
    }

    pub fn source_range(&self) -> Option<TextRange> {
        self.variable_info().map(|i| i.source_range)
    }
}

/// Semantics API for IDE features.
///
/// Entry point for semantic analysis. Provides high-level queries
/// for IDE features like Go to Definition, Hover, Find References.
#[derive(Debug)]
pub struct Semantics<'db, DB> {
    db: &'db DB,
}

impl<'db, DB: DefDatabase + base_db::RootQueryDb> Semantics<'db, DB> {
    pub fn new(db: &'db DB) -> Self {
        Self { db }
    }

    pub fn module_from_file(&self, file_id: vfs::FileId) -> Module<'db, DB> {
        let module_id = ModuleId::new(file_id);
        Module::new(self.db, module_id)
    }

    pub fn find_method(&self, file_id: vfs::FileId, name: &str) -> Option<Method<'db, DB>> {
        let module = self.module_from_file(file_id);
        let search_name = Name::new(name);

        module
            .procedures()
            .into_iter()
            .chain(module.functions())
            .find(|method| method.name().eq_ignore_case(&search_name))
    }

    /// Resolve a name (identifier) to its definition.
    ///
    /// This is the CENTRAL unified resolution API for ALL IDE features.
    /// Use this for: goto definition, hover, find references, semantic highlighting, etc.
    ///
    /// # Resolution Priority (matches BSL semantics)
    ///
    /// 1. Local symbols (parameters, local variables) — highest priority (shadowing)
    /// 2. Builtin platform functions/methods
    /// 3. MDO plural forms (Справочники, Документы)
    /// 4. Module-level methods and variables
    /// 5. Cross-module qualified names (Module.Method)
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let token = /* token at cursor position */;
    /// let sema = Semantics::new(db);
    /// let def = sema.resolve_name_to_definition(file_id, &token)?;
    ///
    /// match def {
    ///     Definition::Method(id) => { /* goto method definition */ }
    ///     Definition::BuiltinFunction(name) => { /* show platform docs */ }
    ///     Definition::MdoObject { .. } => { /* show MDO info */ }
    ///     _ => {}
    /// }
    /// ```
    pub fn resolve_name_to_definition(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<crate::definition::Definition> {
        use crate::definition::Definition;

        let _span = tracing::info_span!("resolve_name_to_definition").entered();

        // Check if it's an identifier
        if token.kind() != syntax::SyntaxKind::IDENT {
            return None;
        }

        let token_text = token.text();
        let name = Name::new(token_text);

        // 1. Check if this is part of a qualified name (X.Y.Z)
        // This must be checked FIRST before local resolution to handle field access
        if let Some(def) = self.try_resolve_qualified_name_for_token(file_id, token) {
            tracing::debug!(?def, "resolved as qualified name");
            return Some(def);
        }

        // 2. Builtin platform functions
        // IMPORTANT: Builtins are NOT shadowed by local variables in BSL!
        // НачатьТранзакцию() is always a builtin even if there's a local var with that name
        if let Some(def) = self.try_resolve_builtin(token_text) {
            tracing::debug!(?def, "resolved as builtin");
            return Some(def);
        }

        // 3. Local symbols (parameters, local variables)
        // These shadow MDO types and module-level symbols, but NOT builtins
        if let Some(def) = self.resolve_local_to_definition(file_id, token) {
            tracing::debug!(?def, "resolved as local symbol");
            return Some(def);
        }

        // 4. MDO plural forms (Справочники, Документы, РегистрыСведений)
        if bsl_metadata::MdoType::is_plural_form(token_text) {
            if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(token_text) {
                tracing::debug!(?mdo_type, "resolved as MDO collection");
                return Some(Definition::MdoCollectionType(mdo_type));
            }
        }

        // 5. Module-level resolution (methods and variables)
        let module_id = ModuleId::new(file_id);
        let resolver = hir_def::resolver::Resolver::for_module(module_id);

        if let Some(method_id) = resolver.resolve_module_method(self.db, &name) {
            tracing::debug!(?method_id, "resolved as module method");
            return Some(Definition::Method(method_id));
        }

        if let Some(var_id) = resolver.resolve_module_variable(self.db, &name) {
            tracing::debug!(?var_id, "resolved as module variable");
            return Some(Definition::Variable(var_id));
        }

        // Unresolved
        tracing::debug!("unresolved identifier: {}", token_text);
        None
    }

    /// Try to resolve a qualified name (X.Y or X.Y.Z) from a token.
    ///
    /// This checks if the token is part of a FieldExpr and resolves the full path.
    fn try_resolve_qualified_name_for_token(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<crate::definition::Definition> {
        use crate::definition::Definition;

        // Walk up the syntax tree to find a FieldExpr ancestor
        let parent = token.parent()?;

        for ancestor in parent.ancestors() {
            if let Some(field_expr) = syntax::ast::FieldExpr::cast(ancestor.clone()) {
                // Extract qualified name
                let qualified_name = match self.extract_qualified_name_from_field_expr(field_expr) {
                    Some(qn) => qn,
                    None => continue,
                };
                tracing::debug!(?qualified_name, "extracted qualified name from field expr");

                // Resolve using workspace scope for cross-file resolution
                let module_id = ModuleId::new(file_id);
                let resolver = hir_def::resolver::Resolver::with_workspace_scope(module_id);
                let resolution = resolver.resolve_path(self.db, &qualified_name);

                tracing::debug!(?resolution, "resolved path");

                // Convert PathResolution to Definition
                return match resolution {
                    PathResolution::Method(method_id) => Some(Definition::Method(method_id)),
                    PathResolution::Variable(var_id) => Some(Definition::Variable(var_id)),
                    PathResolution::Unresolved(_) => None,
                };
            }

            // Stop at statement boundaries
            match ancestor.kind() {
                syntax::SyntaxKind::STMT_LIST
                | syntax::SyntaxKind::SOURCE_FILE
                | syntax::SyntaxKind::PROCEDURE_DEF
                | syntax::SyntaxKind::FUNCTION_DEF => break,
                _ => {}
            }
        }

        None
    }

    /// Resolve a token to a local definition (parameter or local variable).
    ///
    /// Uses ExprScopes to find parameters and local variables in the enclosing method.
    fn resolve_local_to_definition(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<crate::definition::Definition> {
        use crate::definition::Definition;
        use hir_def::scope::{ExprScopes, ScopeDef};

        let _span = tracing::debug_span!("resolve_local_to_definition").entered();

        let name = Name::new(token.text());
        let module_id = ModuleId::new(file_id);

        // Find the enclosing method
        let mut node = token.parent()?;
        loop {
            // Try procedure, then function
            let (scopes, method_range) =
                if let Some(proc_def) = syntax::ast::ProcedureDef::cast(node.clone()) {
                    (ExprScopes::from_procedure(&proc_def), proc_def.syntax().text_range())
                } else if let Some(func_def) = syntax::ast::FunctionDef::cast(node.clone()) {
                    (ExprScopes::from_function(&func_def), func_def.syntax().text_range())
                } else {
                    node = node.parent()?;
                    continue;
                };

            let root_scope = scopes.root_scope();
            let scope_def = scopes.resolve_name(root_scope, &name)?;

            // Find matching method in ItemTree by source_range
            let tree = self.db.item_tree(file_id);
            for (idx, item) in tree.top_level_items().iter().enumerate() {
                let (params, source_range) = match item {
                    hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                        let p = tree.procedure(*proc_idx);
                        (&p.params, p.source_range)
                    }
                    hir_def::item_tree::ModItem::Function(func_idx) => {
                        let f = tree.function(*func_idx);
                        (&f.params, f.source_range)
                    }
                    _ => continue,
                };
                if source_range != method_range {
                    continue;
                }
                let method_id = MethodId { module: module_id, local_id: idx as u32 };
                return Some(match scope_def {
                    ScopeDef::Parameter => {
                        let param_index =
                            params.iter().position(|p| p.name.eq_ignore_case(&name)).unwrap_or(0)
                                as u32;
                        Definition::Parameter { method_id, param_name: name.clone(), param_index }
                    }
                    ScopeDef::LocalVariable => {
                        Definition::Local { method_id, var_name: name.clone() }
                    }
                });
            }
            return None;
        }
    }

    /// Try to resolve a builtin platform function or method.
    ///
    /// Checks bsl_platform data for builtin functions like НачатьТранзакцию, Формат, etc.
    fn try_resolve_builtin(&self, name: &str) -> Option<crate::definition::Definition> {
        // Check if it's a builtin platform function using bsl_platform
        if bsl_platform::PlatformDataInner::instance().get_global_function(name).is_some() {
            return Some(Definition::BuiltinFunction(Name::new(name)));
        }

        None
    }

    /// Extract qualified name from a FieldExpr node by walking up the tree.
    ///
    /// Examples:
    /// - `Module.Method` → `QualifiedName([Module, Method])`
    /// - `Documents.PKO.Create` → `QualifiedName([Documents, PKO, Create])`
    ///
    /// Returns None if the FieldExpr structure is invalid or incomplete.
    fn extract_qualified_name_from_field_expr(
        &self,
        field_expr: syntax::ast::FieldExpr,
    ) -> Option<QualifiedName> {
        use syntax::SyntaxKind;

        let mut segments = Vec::new();

        // Extract the field name (rightmost segment)
        let field_token = field_expr
            .syntax()
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)?;
        segments.push(Name::new(field_token.text()));

        // Walk up to extract base segments
        let base = field_expr.syntax().children().next()?;
        extract_segments_from_expr(&base, &mut segments)?;

        // Reverse to get left-to-right order
        segments.reverse();

        Some(QualifiedName::from_segments(segments))
    }
}

/// Recursively extract name segments from an expression node.
///
/// Appends segments in reverse order (rightmost first).
/// This is a free function to avoid clippy::only_used_in_recursion warning.
fn extract_segments_from_expr(
    expr_node: &syntax::SyntaxNode,
    segments: &mut Vec<Name>,
) -> Option<()> {
    use syntax::SyntaxKind;

    match expr_node.kind() {
        SyntaxKind::FIELD_EXPR => {
            // Nested field access (e.g., A.B in A.B.C)
            let field_expr = syntax::ast::FieldExpr::cast(expr_node.clone())?;

            // Extract the field name
            let field_token = field_expr
                .syntax()
                .children_with_tokens()
                .filter_map(|it| it.into_token())
                .find(|it| it.kind() == SyntaxKind::IDENT)?;
            segments.push(Name::new(field_token.text()));

            // Recurse on base
            let base = field_expr.syntax().children().next()?;
            extract_segments_from_expr(&base, segments)
        }
        SyntaxKind::IDENT | SyntaxKind::EXPR => {
            // Simple identifier or expression containing identifier (leftmost segment)
            let ident_token = expr_node
                .children_with_tokens()
                .filter_map(|it| it.into_token())
                .find(|it| it.kind() == SyntaxKind::IDENT)?;
            segments.push(Name::new(ident_token.text()));
            Some(())
        }
        _ => {
            // Fallback: check if this node directly contains an IDENT token
            let ident_token = expr_node
                .children_with_tokens()
                .filter_map(|it| it.into_token())
                .find(|it| it.kind() == SyntaxKind::IDENT)?;
            segments.push(Name::new(ident_token.text()));
            Some(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, FileId, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file text
        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_find_method_by_name() {
        let source = r#"
Процедура ПерваяПроцедура()
КонецПроцедуры

Функция ВтораяФункция() Экспорт
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Find procedure
        let method = sema.find_method(file_id, "ПерваяПроцедура");
        assert!(method.is_some());
        let method = method.unwrap();
        assert_eq!(method.name().as_str(), "ПерваяПроцедура");
        assert!(!method.is_function());
        assert!(!method.is_export());

        // Find function
        let method = sema.find_method(file_id, "ВтораяФункция");
        assert!(method.is_some());
        let method = method.unwrap();
        assert_eq!(method.name().as_str(), "ВтораяФункция");
        assert!(method.is_function());
        assert!(method.is_export());

        // Not found
        let method = sema.find_method(file_id, "НесуществующаяФункция");
        assert!(method.is_none());
    }

    #[test]
    fn test_case_insensitive_search() {
        let source = r#"
Процедура МояПроцедура()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);

        // Different cases
        assert!(sema.find_method(file_id, "мояпроцедура").is_some());
        assert!(sema.find_method(file_id, "МОЯПРОЦЕДУРА").is_some());
        assert!(sema.find_method(file_id, "МоЯпРоЦеДуРа").is_some());
    }

    #[test]
    fn test_list_all_procedures() {
        let source = r#"
Процедура Первая()
КонецПроцедуры

Процедура Вторая() Экспорт
КонецПроцедуры

Функция Третья()
КонецФункции
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        let procedures = module.procedures();
        assert_eq!(procedures.len(), 2);

        let functions = module.functions();
        assert_eq!(functions.len(), 1);

        // Check first procedure
        assert_eq!(procedures[0].name().as_str(), "Первая");
        assert!(!procedures[0].is_export());

        // Check second procedure
        assert_eq!(procedures[1].name().as_str(), "Вторая");
        assert!(procedures[1].is_export());

        // Check function
        assert_eq!(functions[0].name().as_str(), "Третья");
        assert!(!functions[0].is_export());
    }

    #[test]
    fn test_module_variables() {
        let source = r#"
Перем ПерваяПеременная;
Перем ВтораяПеременная Экспорт;

Процедура Тест()
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        let variables = module.variables();
        assert_eq!(variables.len(), 2);

        // Check first variable
        assert_eq!(variables[0].name().as_str(), "ПерваяПеременная");
        assert!(!variables[0].is_export());

        // Check second variable
        assert_eq!(variables[1].name().as_str(), "ВтораяПеременная");
        assert!(variables[1].is_export());
    }

    #[test]
    fn test_empty_module() {
        let source = "// Пустой модуль\n";

        let (db, file_id) = create_db_with_file(source);
        let sema = Semantics::new(&db);
        let module = sema.module_from_file(file_id);

        assert_eq!(module.procedures().len(), 0);
        assert_eq!(module.functions().len(), 0);
        assert_eq!(module.variables().len(), 0);
    }
}
