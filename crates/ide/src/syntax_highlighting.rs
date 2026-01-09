//! Semantic syntax highlighting for BSL.
//!
//! This module provides semantic highlighting by analyzing the HIR
//! and assigning semantic token types and modifiers to each token.
//!
//! ## Architecture
//!
//! 1. **Syntactic highlighting** - Keywords, literals, comments, operators (by token kind)
//! 2. **Semantic highlighting** - Function calls, variables, parameters (via HIR + name resolution)
//! 3. **SDBL highlighting** - SDBL keywords, operators, functions in string literals (via sdbl-hir)
//!
//! Semantic highlighting requires name resolution to distinguish:
//! - Function calls from variables
//! - Parameters from local variables
//! - Module variables from local variables
//! - Builtin functions from user-defined (TODO)

mod sdbl;

use either::Either;
use ide_db::{
    hir_def::{resolver::Resolver, scope::ExprScopes, MethodId, ModuleId, Name},
    RootDatabase, TextRange,
};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use syntax::{
    ast::{self, AstNode},
    SyntaxKind, SyntaxNode, SyntaxToken,
};
use vfs::FileId;

/// Semantic token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlTag {
    /// Keywords (Процедура, Функция, Если, etc.)
    Keyword,
    /// Function names
    Function,
    /// Procedure names
    Procedure,
    /// Parameter names
    Parameter,
    /// Variable names
    Variable,
    /// String literals
    StringLiteral,
    /// Number literals
    NumberLiteral,
    /// Boolean literals (Истина, Ложь, True, False)
    BooleanLiteral,
    /// Comments
    Comment,
    /// Preprocessor directives (#Если, #Область, etc.)
    Preprocessor,
    /// Annotations (&НаКлиенте, &НаСервере, etc.)
    Annotation,
    /// Property access (Object.Property)
    Property,
    /// Operators (+, -, *, /, =, etc.)
    Operator,
    /// Unresolved reference (identifier not found in metadata)
    UnresolvedReference,
}

impl HlTag {
    /// Returns the LSP semantic token type name.
    pub fn as_str(&self) -> &'static str {
        match self {
            HlTag::Keyword => "keyword",
            HlTag::Function => "function",
            HlTag::Procedure => "function",
            HlTag::Parameter => "parameter",
            HlTag::Variable => "variable",
            HlTag::StringLiteral => "string",
            HlTag::NumberLiteral => "number",
            HlTag::BooleanLiteral => "keyword",
            HlTag::Comment => "comment",
            HlTag::Preprocessor => "macro",
            HlTag::Annotation => "decorator",
            HlTag::Property => "property",
            HlTag::Operator => "operator",
            HlTag::UnresolvedReference => "unresolvedReference",
        }
    }
}

/// Semantic token modifiers (bitflags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HlMod(u32);

impl HlMod {
    pub const EXPORT: HlMod = HlMod(1 << 0);
    pub const DEPRECATED: HlMod = HlMod(1 << 1);
    pub const ASYNC: HlMod = HlMod(1 << 2);
    pub const DECLARATION: HlMod = HlMod(1 << 3);
    pub const DEFINITION: HlMod = HlMod(1 << 4);

    pub const fn new() -> Self {
        HlMod(0)
    }

    pub const fn with(mut self, modifier: HlMod) -> Self {
        self.0 |= modifier.0;
        self
    }

    pub const fn contains(self, modifier: HlMod) -> bool {
        (self.0 & modifier.0) != 0
    }

    /// Returns LSP semantic token modifier names.
    pub fn as_strings(&self) -> Vec<&'static str> {
        let mut result = Vec::new();
        if self.contains(HlMod::EXPORT) {
            result.push("defaultLibrary");
        }
        if self.contains(HlMod::DEPRECATED) {
            result.push("deprecated");
        }
        if self.contains(HlMod::ASYNC) {
            result.push("async");
        }
        if self.contains(HlMod::DECLARATION) {
            result.push("declaration");
        }
        if self.contains(HlMod::DEFINITION) {
            result.push("definition");
        }
        result
    }
}

/// A highlighted range with semantic token type and modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlRange {
    pub range: TextRange,
    pub tag: HlTag,
    pub modifiers: HlMod,
}

/// Context for semantic highlighting that caches ExprScopes for methods.
///
/// This struct lives only during a single `highlight()` call and provides:
/// - Cached ExprScopes for each method (avoids rebuilding for every token)
/// - Database and file context
/// - SDBL context (line index, tracked literals)
pub(crate) struct HighlightContext<'db> {
    pub(crate) db: &'db dyn RootDatabase,
    pub(crate) module_id: ModuleId,
    pub(crate) file_id: FileId,
    /// Cache: method source_range -> (MethodId, ExprScopes, root ScopeId)
    pub(crate) expr_scopes_cache: FxHashMap<TextRange, (MethodId, Arc<ExprScopes>)>,

    /// Line index for SDBL position mapping optimization (built once for entire file)
    /// Eliminates 100× rebuilds for files with many SDBL queries
    pub(crate) line_index: Option<Vec<usize>>,

    /// SDBL literal ranges to skip STRING token highlighting
    /// When a literal contains SDBL, its tokens are highlighted by sdbl module
    pub(crate) sdbl_literal_ranges: rustc_hash::FxHashSet<TextRange>,
}

impl<'db> HighlightContext<'db> {
    fn new(db: &'db dyn RootDatabase, file_id: FileId, line_index: Option<Vec<usize>>) -> Self {
        Self {
            db,
            module_id: ModuleId::new(file_id),
            file_id,
            expr_scopes_cache: FxHashMap::default(),
            line_index,
            sdbl_literal_ranges: rustc_hash::FxHashSet::default(),
        }
    }

    /// Get or build ExprScopes for a method.
    ///
    /// # Arguments
    /// - `method_range` - source_range of the method definition
    /// - `method_def` - AST node (ProcedureDef or FunctionDef)
    /// - `method_id` - HIR MethodId
    ///
    /// # Returns
    /// Cached or newly built ExprScopes for the method
    fn get_expr_scopes(
        &mut self,
        method_range: TextRange,
        method_def: Either<ast::ProcedureDef, ast::FunctionDef>,
        method_id: MethodId,
    ) -> Arc<ExprScopes> {
        // Check cache
        if let Some((_, scopes)) = self.expr_scopes_cache.get(&method_range) {
            return scopes.clone();
        }

        // Build ExprScopes from AST
        let scopes = match method_def {
            Either::Left(proc) => ExprScopes::from_procedure(&proc),
            Either::Right(func) => ExprScopes::from_function(&func),
        };
        let scopes = Arc::new(scopes);

        // Cache for future tokens in the same method
        self.expr_scopes_cache.insert(method_range, (method_id, scopes.clone()));
        scopes
    }
}

/// Find the method (procedure or function) containing this token.
///
/// # Algorithm
/// 1. Walk up AST ancestors from token's parent
/// 2. Find first ProcedureDef or FunctionDef node
/// 3. Match its source_range in ItemTree to get MethodId
///
/// # Returns
/// Some((MethodId, Either<ProcedureDef, FunctionDef>)) if token is inside a method
fn find_method_for_token(
    db: &dyn RootDatabase,
    file_id: FileId,
    token: &SyntaxToken,
) -> Option<(MethodId, Either<ast::ProcedureDef, ast::FunctionDef>)> {
    // Walk up ancestors to find containing method
    for ancestor in token.parent()?.ancestors() {
        if let Some(proc) = ast::ProcedureDef::cast(ancestor.clone()) {
            let method_id = find_method_id_by_range(db, file_id, proc.syntax().text_range())?;
            return Some((method_id, Either::Left(proc)));
        }
        if let Some(func) = ast::FunctionDef::cast(ancestor.clone()) {
            let method_id = find_method_id_by_range(db, file_id, func.syntax().text_range())?;
            return Some((method_id, Either::Right(func)));
        }
    }
    None
}

/// Find MethodId by matching source_range in ItemTree.
///
/// # Algorithm
/// 1. Get ItemTree for file
/// 2. Iterate through top_level_items
/// 3. Match source_range of each Procedure/Function with given range
/// 4. Return MethodId with matched index
///
/// # Note
/// This is O(M) where M = number of methods, typically 10-50 per file.
fn find_method_id_by_range(
    db: &dyn RootDatabase,
    file_id: FileId,
    range: TextRange,
) -> Option<MethodId> {
    let item_tree = db.item_tree(file_id);
    let module_id = ModuleId::new(file_id);

    for (idx, item) in item_tree.top_level_items().iter().enumerate() {
        match item {
            ide_db::hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                let proc = item_tree.procedure(*proc_idx);
                if proc.source_range == range {
                    return Some(MethodId { module: module_id, local_id: idx as u32 });
                }
            }
            ide_db::hir_def::item_tree::ModItem::Function(func_idx) => {
                let func = item_tree.function(*func_idx);
                if func.source_range == range {
                    return Some(MethodId { module: module_id, local_id: idx as u32 });
                }
            }
            _ => {}
        }
    }
    None
}

/// Highlight a local symbol (parameter or local variable) using ExprScopes.
///
/// # Algorithm
/// 1. Find the containing method for this token
/// 2. Get or build ExprScopes for that method (cached in context)
/// 3. Resolve the name in the root scope
/// 4. Map ScopeDef to HlTag (Parameter or Variable)
///
/// # Returns
/// Some(HlRange) if the identifier resolves to a local symbol
fn highlight_local_symbol(
    ctx: &mut HighlightContext,
    token: &SyntaxToken,
    name: &Name,
) -> Option<HlRange> {
    // Find containing method
    let (method_id, method_def) = find_method_for_token(ctx.db, ctx.file_id, token)?;
    let method_range = match &method_def {
        Either::Left(proc) => proc.syntax().text_range(),
        Either::Right(func) => func.syntax().text_range(),
    };

    // Get or build ExprScopes (cached)
    let scopes = ctx.get_expr_scopes(method_range, method_def, method_id);
    let root_scope = scopes.root_scope();

    // Resolve name in ExprScopes
    let def = scopes.resolve_name(root_scope, name)?;

    // Map ScopeDef to HlTag
    let range = token.text_range();
    let tag = match def {
        ide_db::hir_def::scope::ScopeDef::Parameter => HlTag::Parameter,
        ide_db::hir_def::scope::ScopeDef::LocalVariable => HlTag::Variable,
    };

    Some(HlRange { range, tag, modifiers: HlMod::new() })
}

/// Generates semantic highlighting for a file.
pub fn highlight(db: &dyn RootDatabase, file_id: FileId) -> Vec<HlRange> {
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Build line index ONCE for entire file (optimization for SDBL position mapping)
    let input = db.file_text_input(file_id);
    let bsl_source = input.text(db);
    let line_index = ide_diagnostics::sdbl_utils::build_line_index_shared(&bsl_source);

    let mut ctx = HighlightContext::new(db, file_id, Some(line_index));
    let mut highlights = Vec::new();

    traverse_node(&mut ctx, &root, &mut highlights);
    highlights
}

/// Recursively traverse AST and collect highlights.
fn traverse_node(ctx: &mut HighlightContext, node: &SyntaxNode, highlights: &mut Vec<HlRange>) {
    // Highlight tokens based on their type
    for token in node.children_with_tokens() {
        match token {
            syntax::NodeOrToken::Token(token) => {
                // Try semantic highlighting for IDENT tokens first
                if token.kind() == SyntaxKind::IDENT {
                    if let Some(hl) = highlight_ident_semantic(ctx, &token) {
                        highlights.push(hl);
                        continue;
                    }
                }

                // Fall back to syntactic highlighting
                if let Some(hl) = highlight_token(&token, ctx) {
                    highlights.push(hl);
                }
            }
            syntax::NodeOrToken::Node(node) => {
                // Check for SDBL in string literals BEFORE other processing
                if node.kind() == SyntaxKind::LITERAL {
                    if let Some(sdbl_highlights) = sdbl::highlight_sdbl_in_literal(ctx, &node) {
                        // Track this literal as containing SDBL to skip STRING token highlighting
                        ctx.sdbl_literal_ranges.insert(node.text_range());
                        highlights.extend(sdbl_highlights);
                        continue; // Skip children - SDBL tokens override STRING highlighting
                    }
                }

                // Check for special nodes that need specific highlighting
                highlight_node(&node, highlights);

                // Recurse into children
                traverse_node(ctx, &node, highlights);
            }
        }
    }
}

/// Highlight an IDENT token using semantic analysis (name resolution).
///
/// This function resolves the identifier to determine if it's a:
/// - Parameter reference (via ExprScopes)
/// - Local variable reference (via ExprScopes)
/// - Function/procedure call (via module-level Resolver)
/// - Module variable reference (via module-level Resolver)
///
/// Resolution priority: Local -> Module
fn highlight_ident_semantic(ctx: &mut HighlightContext, token: &SyntaxToken) -> Option<HlRange> {
    let range = token.text_range();
    let name_text = token.text();
    let name = Name::new(name_text);

    // Try local resolution FIRST (parameters and local variables)
    if let Some(hl) = highlight_local_symbol(ctx, token, &name) {
        return Some(hl);
    }

    // Fall back to module-level resolution (methods and module variables)
    let resolver = Resolver::for_module(ctx.module_id);

    if let Some(_method_id) = resolver.resolve_module_method(ctx.db, &name) {
        return Some(HlRange { range, tag: HlTag::Function, modifiers: HlMod::new() });
    }

    if let Some(_var_id) = resolver.resolve_module_variable(ctx.db, &name) {
        return Some(HlRange { range, tag: HlTag::Variable, modifiers: HlMod::new() });
    }

    None
}

/// Highlight a single token based on its syntax kind.
fn highlight_token(token: &SyntaxToken, ctx: &HighlightContext) -> Option<HlRange> {
    let kind = token.kind();
    let range = token.text_range();

    // Skip STRING tokens if parent is a SDBL literal (already highlighted by sdbl module)
    if matches!(
        kind,
        SyntaxKind::STRING
            | SyntaxKind::STRING_START
            | SyntaxKind::STRING_TAIL
            | SyntaxKind::STRING_PART
    ) {
        if let Some(parent) = token.parent() {
            if ctx.sdbl_literal_ranges.contains(&parent.text_range()) {
                return None;
            }
        }
    }

    let tag = match kind {
        // Keywords
        SyntaxKind::KW_PROCEDURE
        | SyntaxKind::KW_END_PROCEDURE
        | SyntaxKind::KW_FUNCTION
        | SyntaxKind::KW_END_FUNCTION
        | SyntaxKind::KW_IF
        | SyntaxKind::KW_THEN
        | SyntaxKind::KW_ELSIF
        | SyntaxKind::KW_ELSE
        | SyntaxKind::KW_END_IF
        | SyntaxKind::KW_FOR
        | SyntaxKind::KW_EACH
        | SyntaxKind::KW_IN
        | SyntaxKind::KW_TO
        | SyntaxKind::KW_DO
        | SyntaxKind::KW_END_DO
        | SyntaxKind::KW_WHILE
        | SyntaxKind::KW_RETURN
        | SyntaxKind::KW_VAR
        | SyntaxKind::KW_EXPORT
        | SyntaxKind::KW_NEW
        | SyntaxKind::KW_TRY
        | SyntaxKind::KW_EXCEPT
        | SyntaxKind::KW_END_TRY
        | SyntaxKind::KW_RAISE
        | SyntaxKind::KW_EXECUTE
        | SyntaxKind::KW_BREAK
        | SyntaxKind::KW_CONTINUE
        | SyntaxKind::KW_AND
        | SyntaxKind::KW_OR
        | SyntaxKind::KW_NOT
        | SyntaxKind::KW_GOTO
        | SyntaxKind::KW_ADD_HANDLER
        | SyntaxKind::KW_REMOVE_HANDLER
        | SyntaxKind::KW_ASYNC
        | SyntaxKind::KW_AWAIT => HlTag::Keyword,

        // Boolean literals
        SyntaxKind::KW_TRUE | SyntaxKind::KW_FALSE => HlTag::BooleanLiteral,

        // Literals
        SyntaxKind::STRING
        | SyntaxKind::STRING_START
        | SyntaxKind::STRING_TAIL
        | SyntaxKind::STRING_PART => HlTag::StringLiteral,
        SyntaxKind::DECIMAL | SyntaxKind::FLOAT | SyntaxKind::DATE => HlTag::NumberLiteral,

        // Comments
        SyntaxKind::COMMENT => HlTag::Comment,

        // Preprocessor
        SyntaxKind::PRE_IF
        | SyntaxKind::PRE_ELSIF
        | SyntaxKind::PRE_ELSE
        | SyntaxKind::PRE_END_IF
        | SyntaxKind::PRE_REGION
        | SyntaxKind::PRE_END_REGION
        | SyntaxKind::PRE_USE => HlTag::Preprocessor,

        // Annotations
        SyntaxKind::ANN_AT_CLIENT
        | SyntaxKind::ANN_AT_SERVER
        | SyntaxKind::ANN_AT_SERVER_NO_CONTEXT
        | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
        | SyntaxKind::ANN_AT_CLIENT_AT_SERVER
        | SyntaxKind::ANN_BEFORE
        | SyntaxKind::ANN_AFTER
        | SyntaxKind::ANN_AROUND
        | SyntaxKind::ANN_CHANGE_AND_VALIDATE
        | SyntaxKind::ANN_CUSTOM => HlTag::Annotation,

        // Operators
        SyntaxKind::PLUS
        | SyntaxKind::MINUS
        | SyntaxKind::STAR
        | SyntaxKind::SLASH
        | SyntaxKind::PERCENT
        | SyntaxKind::EQ
        | SyntaxKind::NEQ
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE => HlTag::Operator,

        _ => return None,
    };

    Some(HlRange { range, tag, modifiers: HlMod::new() })
}

/// Highlight specific AST nodes that need special handling.
fn highlight_node(node: &SyntaxNode, highlights: &mut Vec<HlRange>) {
    // Highlight function/procedure names
    if let Some(func) = ast::FunctionDef::cast(node.clone()) {
        if let Some(name) = func.name() {
            highlights.push(HlRange {
                range: name.text_range(),
                tag: HlTag::Function,
                modifiers: if func.export_keyword().is_some() {
                    HlMod::new().with(HlMod::EXPORT).with(HlMod::DEFINITION)
                } else {
                    HlMod::new().with(HlMod::DEFINITION)
                },
            });
        }
    }

    if let Some(proc) = ast::ProcedureDef::cast(node.clone()) {
        if let Some(name) = proc.name() {
            highlights.push(HlRange {
                range: name.text_range(),
                tag: HlTag::Procedure,
                modifiers: if proc.export_keyword().is_some() {
                    HlMod::new().with(HlMod::EXPORT).with(HlMod::DEFINITION)
                } else {
                    HlMod::new().with(HlMod::DEFINITION)
                },
            });
        }
    }

    // Highlight parameters
    if let Some(param) = ast::Param::cast(node.clone()) {
        if let Some(name) = param.name() {
            highlights.push(HlRange {
                range: name.text_range(),
                tag: HlTag::Parameter,
                modifiers: HlMod::new().with(HlMod::DECLARATION),
            });
        }
    }

    // Highlight variable declarations
    if let Some(var_def) = ast::VarDef::cast(node.clone()) {
        for name in var_def.names() {
            highlights.push(HlRange {
                range: name.text_range(),
                tag: HlTag::Variable,
                modifiers: HlMod::new().with(HlMod::DECLARATION),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};

    fn create_db_with_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file contents
        db.set_file_text(file_id, source);

        (db, file_id)
    }

    #[test]
    fn test_hl_tag_as_str() {
        assert_eq!(HlTag::Keyword.as_str(), "keyword");
        assert_eq!(HlTag::Function.as_str(), "function");
        assert_eq!(HlTag::StringLiteral.as_str(), "string");
    }

    #[test]
    fn test_hl_mod() {
        let mods = HlMod::new().with(HlMod::EXPORT).with(HlMod::DEPRECATED);

        assert!(mods.contains(HlMod::EXPORT));
        assert!(mods.contains(HlMod::DEPRECATED));
        assert!(!mods.contains(HlMod::ASYNC));

        let strings = mods.as_strings();
        assert!(strings.contains(&"defaultLibrary"));
        assert!(strings.contains(&"deprecated"));
    }

    #[test]
    fn test_highlight_parameter() {
        let code = r#"
Функция Тест(Параметр1, Параметр2)
    Результат = Параметр1 + Параметр2;
    Возврат Результат;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Check that parameters are highlighted as Parameter
        let param_highlights: Vec<_> =
            highlights.iter().filter(|hl| hl.tag == HlTag::Parameter).collect();

        // Should find 4 parameter usages: 2 declarations + 2 usages
        assert!(
            param_highlights.len() >= 4,
            "Expected at least 4 parameter highlights, got {}",
            param_highlights.len()
        );
    }

    #[test]
    fn test_highlight_local_variable() {
        let code = r#"
Процедура Тест()
    Перем ЛокальнаяПеременная;
    ЛокальнаяПеременная = 42;
КонецПроцедуры
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Check that local variables are highlighted as Variable
        let var_highlights: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Variable
                    && (hl.modifiers.contains(HlMod::DECLARATION)
                        || !hl.modifiers.contains(HlMod::DEFINITION))
            })
            .collect();

        // Should find at least 2 variable highlights: 1 declaration + 1 usage
        assert!(
            var_highlights.len() >= 2,
            "Expected at least 2 variable highlights, got {}",
            var_highlights.len()
        );
    }

    #[test]
    fn test_highlight_parameter_vs_local_variable() {
        let code = r#"
Функция Тест(Параметр)
    Перем ЛокальнаяПеременная;
    ЛокальнаяПеременная = СокрЛП(Параметр);
    Возврат ЛокальнаяПеременная;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Check parameters
        let param_count = highlights.iter().filter(|hl| hl.tag == HlTag::Parameter).count();

        // Check local variables (excluding module-level)
        let local_var_count = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Variable
                    && (hl.modifiers.contains(HlMod::DECLARATION)
                        || !hl.modifiers.contains(HlMod::DEFINITION))
            })
            .count();

        // Should find 2 parameter usages and 3 local variable usages
        assert!(param_count >= 2, "Expected at least 2 parameter highlights");
        assert!(local_var_count >= 3, "Expected at least 3 local variable highlights");
    }

    #[test]
    fn test_highlight_case_insensitive() {
        let code = r#"
Функция Тест(Параметр)
    Результат = параметр + ПАРАМЕТР;
    Возврат результат;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // All three variants of "Параметр" should be highlighted
        let param_count = highlights.iter().filter(|hl| hl.tag == HlTag::Parameter).count();

        assert!(
            param_count >= 3,
            "Expected at least 3 parameter highlights (case-insensitive), got {}",
            param_count
        );
    }

    #[test]
    fn test_highlight_module_variable_vs_local() {
        let code = r#"
Перем МодульнаяПеременная;

Функция Тест()
    Перем ЛокальнаяПеременная;
    ЛокальнаяПеременная = МодульнаяПеременная;
    Возврат ЛокальнаяПеременная;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Both module and local variables should be highlighted
        let var_highlights: Vec<_> =
            highlights.iter().filter(|hl| hl.tag == HlTag::Variable).collect();

        // Should find at least 5 variable highlights:
        // 1 module var declaration + 1 module var usage
        // + 1 local var declaration + 2 local var usages
        assert!(
            var_highlights.len() >= 5,
            "Expected at least 5 variable highlights (module + local), got {}",
            var_highlights.len()
        );
    }

    #[test]
    fn test_highlight_multiple_methods() {
        let code = r#"
Функция Функция1(Параметр1)
    Перем Локальная1;
    Локальная1 = Параметр1;
    Возврат Локальная1;
КонецФункции

Функция Функция2(Параметр2)
    Перем Локальная2;
    Локальная2 = Параметр2;
    Возврат Локальная2;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Each method's parameters and locals should be independently highlighted
        let param_count = highlights.iter().filter(|hl| hl.tag == HlTag::Parameter).count();

        let var_count = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Variable
                    && (hl.modifiers.contains(HlMod::DECLARATION)
                        || !hl.modifiers.contains(HlMod::DEFINITION))
            })
            .count();

        // Should find 4 parameter usages (2 per method) and 6 variable usages (3 per method)
        assert!(param_count >= 4, "Expected at least 4 parameter highlights");
        assert!(var_count >= 6, "Expected at least 6 local variable highlights");
    }

    #[test]
    fn test_sdbl_keyword_highlighting() {
        let code = r#"
Функция Тест()
    Запрос = "SELECT Код FROM Справочник.Валюты";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find SELECT keyword - should be highlighted as Keyword
        let select_kw = highlights.iter().find(|hl| {
            hl.tag == HlTag::Keyword
                && code[hl.range.start().into()..hl.range.end().into()] == *"SELECT"
        });

        assert!(select_kw.is_some(), "SELECT should be highlighted as Keyword");

        // Find FROM keyword - should be highlighted as Keyword
        let from_kw = highlights.iter().find(|hl| {
            hl.tag == HlTag::Keyword
                && code[hl.range.start().into()..hl.range.end().into()] == *"FROM"
        });

        assert!(from_kw.is_some(), "FROM should be highlighted as Keyword");
    }

    #[test]
    fn test_sdbl_multiline_highlighting() {
        let code = r#"
Функция Тест()
    Запрос = "ВЫБРАТЬ
             |    Код
             |ИЗ
             |    Справочник.Валюты";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find ВЫБРАТЬ keyword
        let select_kw = highlights.iter().find(|hl| {
            hl.tag == HlTag::Keyword && {
                let text = &code[hl.range.start().into()..hl.range.end().into()];
                text.contains("ВЫБРАТЬ")
            }
        });

        assert!(select_kw.is_some(), "ВЫБРАТЬ should be highlighted as Keyword");

        // Find ИЗ keyword
        let from_kw = highlights.iter().find(|hl| {
            hl.tag == HlTag::Keyword && {
                let text = &code[hl.range.start().into()..hl.range.end().into()];
                text.contains("ИЗ")
            }
        });

        assert!(from_kw.is_some(), "ИЗ should be highlighted as Keyword");
    }

    #[test]
    fn test_sdbl_aggregate_functions() {
        let code = r#"
Функция Тест()
    Запрос = "SELECT SUM(Сумма) FROM Документ.Продажи";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find SUM function - should be highlighted as Function
        let sum_fn = highlights.iter().find(|hl| {
            hl.tag == HlTag::Function
                && code[hl.range.start().into()..hl.range.end().into()] == *"SUM"
        });

        assert!(sum_fn.is_some(), "SUM should be highlighted as Function");
    }

    #[test]
    fn test_sdbl_operators() {
        let code = r#"
Функция Тест()
    Запрос = "SELECT * FROM Таблица WHERE А = Б AND В <> Г";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find = operator - should be highlighted as Operator
        let eq_op = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Operator
                    && code[hl.range.start().into()..hl.range.end().into()] == *"="
            })
            .count();

        // Should find at least one = operator in SDBL (ignore the BSL assignment)
        assert!(eq_op >= 1, "= should be highlighted as Operator");

        // Find AND operator - should be highlighted as Operator
        let and_op = highlights.iter().find(|hl| {
            hl.tag == HlTag::Operator
                && code[hl.range.start().into()..hl.range.end().into()] == *"AND"
        });

        assert!(and_op.is_some(), "AND should be highlighted as Operator");
    }

    #[test]
    fn test_no_sdbl_highlight_for_short_strings() {
        let code = r#"
Функция Тест()
    Строка = "SELECT";
    Возврат Строка;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // The string "SELECT" is too short (< 15 chars) so should be highlighted as StringLiteral
        let string_highlights: Vec<_> =
            highlights.iter().filter(|hl| hl.tag == HlTag::StringLiteral).collect();

        // Should have at least one string literal
        assert!(!string_highlights.is_empty(), "Short strings should remain as StringLiteral");
    }

    #[test]
    fn test_sdbl_as_keyword_highlighting() {
        let code = r#"
Функция Тест()
    Запрос = "ВЫБРАТЬ
             |    Очередь.ОтметкаВремени КАК ОтметкаВремени,
             |    Очередь.Попыток КАК Попыток
             |ИЗ
             |    РегистрСведений.ОчередьОбновленияКэширующихДанных КАК Очередь";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find all КАК (AS) keywords - should be highlighted as Keyword
        let as_keywords: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Keyword && {
                    let text = &code[hl.range.start().into()..hl.range.end().into()];
                    text.contains("КАК")
                }
            })
            .collect();

        // Should find 3 КАК keywords (2 in field aliases + 1 in table alias)
        assert_eq!(as_keywords.len(), 3, "Expected 3 КАК keywords, got {}", as_keywords.len());
    }

    #[test]
    fn test_sdbl_as_keyword_english() {
        let code = r#"
Функция Тест()
    Query = "SELECT Name AS ProductName FROM Products AS P";
    Возврат Query;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find all AS keywords - should be highlighted as Keyword
        let as_keywords: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Keyword
                    && code[hl.range.start().into()..hl.range.end().into()] == *"AS"
            })
            .collect();

        // Should find 2 AS keywords (1 in field alias + 1 in table alias)
        assert_eq!(as_keywords.len(), 2, "Expected 2 AS keywords, got {}", as_keywords.len());
    }
}
