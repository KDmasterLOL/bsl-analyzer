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

use ide_db::{RootDatabase, TextRange};
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
    /// Built-in platform function (НачатьТранзакцию, Формат, Сообщить, etc.)
    BuiltinFunction,
    /// Type names / Class names (SDBL table names: Справочник.Валюты)
    Type,
    /// Enum member / Constant (SDBL field aliases: КАК АлиасПоля)
    EnumMember,
    /// Namespace / Module (SDBL table aliases: Валюты.Наименование)
    Namespace,
    /// Class (SDBL MDO types: Справочник, Документ, Перечисление)
    Class,
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
            HlTag::BuiltinFunction => "function",
            HlTag::Type => "type",
            HlTag::EnumMember => "enumMember",
            HlTag::Namespace => "namespace",
            HlTag::Class => "class",
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
pub(crate) struct HighlightContext<'db, DB: RootDatabase> {
    pub(crate) db: &'db DB,
    pub(crate) file_id: FileId,

    /// Line index for SDBL position mapping optimization (built once for entire file)
    /// Eliminates 100× rebuilds for files with many SDBL queries
    pub(crate) line_index: Option<Vec<usize>>,

    /// SDBL literal ranges to skip STRING token highlighting
    /// When a literal contains SDBL, its tokens are highlighted by sdbl module
    pub(crate) sdbl_literal_ranges: rustc_hash::FxHashSet<TextRange>,
}

impl<'db, DB: RootDatabase> HighlightContext<'db, DB> {
    fn new(db: &'db DB, file_id: FileId, line_index: Option<Vec<usize>>) -> Self {
        Self { db, file_id, line_index, sdbl_literal_ranges: rustc_hash::FxHashSet::default() }
    }
}
/// Generates semantic highlighting for a file.
pub fn highlight<DB: RootDatabase>(db: &DB, file_id: FileId) -> Vec<HlRange> {
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
fn traverse_node<DB: RootDatabase>(
    ctx: &mut HighlightContext<DB>,
    node: &SyntaxNode,
    highlights: &mut Vec<HlRange>,
) {
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

/// Highlight an IDENT token using semantic analysis (Definition API + ExprScopes).
///
/// Resolution order (properly handles shadowing):
/// 1. Builtin functions (always highlighted even if shadowed - important for user awareness)
/// 2. Local symbols via ExprScopes (parameters and local variables - shadow everything else)
/// 3. Other symbols via Semantics API (module methods, variables, MDO types, etc.)
/// 4. MDO plural forms as fallback (if not resolved by Semantics API)
fn highlight_ident_semantic<DB: RootDatabase>(
    ctx: &mut HighlightContext<DB>,
    token: &SyntaxToken,
) -> Option<HlRange> {
    let range = token.text_range();

    tracing::debug!("highlight_ident_semantic: processing token={}", token.text());

    // Use unified Semantics API for ALL resolution
    // It handles the correct priority order:
    // 1. Qualified names (X.Y.Z)
    // 2. Local symbols (parameters, local variables)
    // 3. Builtin platform functions
    // 4. MDO plural forms
    // 5. Module-level symbols (methods, variables)
    let sema = hir::Semantics::new(ctx.db);
    let definition = sema.resolve_name_to_definition(ctx.file_id, token)?;

    tracing::debug!("highlight_ident_semantic: {} resolved to {:?}", token.text(), definition);

    // Convert Definition to HlTag + HlModifiers
    let tag = match &definition {
        hir::Definition::Method(_) => HlTag::Function,
        hir::Definition::Variable(_) => HlTag::Variable,
        hir::Definition::Parameter { .. } => HlTag::Parameter,
        hir::Definition::Local { .. } => HlTag::Variable,
        hir::Definition::BuiltinFunction(_) => HlTag::BuiltinFunction,
        hir::Definition::BuiltinMethod { .. } => HlTag::Function,
        hir::Definition::MdoCollectionType(_) => HlTag::Class,
        hir::Definition::MdoObject { .. } => HlTag::Type,
        hir::Definition::MdoManagerModule { .. } => HlTag::Namespace,
        hir::Definition::Module(_) => HlTag::Namespace,
        hir::Definition::VirtualTableField { .. } => HlTag::Property,
        hir::Definition::Unresolved => return None,
    };

    let mut modifiers = HlMod::new();

    // Add EXPORT modifier for exported symbols
    if definition.is_export(ctx.db) {
        modifiers = modifiers.with(HlMod::EXPORT);
    }

    // Add EXPORT modifier for builtin functions
    if matches!(
        definition,
        hir::Definition::BuiltinFunction(_) | hir::Definition::BuiltinMethod { .. }
    ) {
        modifiers = modifiers.with(HlMod::EXPORT);
    }

    Some(HlRange { range, tag, modifiers })
}

/// Highlight a single token based on its syntax kind.
fn highlight_token<DB: RootDatabase>(
    token: &SyntaxToken,
    ctx: &HighlightContext<DB>,
) -> Option<HlRange> {
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
    fn test_sdbl_field_alias_highlighting() {
        let code = r#"
Функция Тест()
    Запрос = "ВЫБРАТЬ
             |    Валюты.Наименование КАК СимвольныйКод,
             |    Валюты.Код КАК КодВалюты
             |ИЗ
             |    Справочник.Валюты КАК Валюты";
    Возврат Запрос;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Find field aliases - should be highlighted as EnumMember
        let field_aliases: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::EnumMember && {
                    let text = &code[hl.range.start().into()..hl.range.end().into()];
                    text == "СимвольныйКод" || text == "КодВалюты"
                }
            })
            .collect();

        assert_eq!(
            field_aliases.len(),
            2,
            "Expected 2 field aliases (СимвольныйКод, КодВалюты) highlighted as EnumMember, got {}",
            field_aliases.len()
        );

        // Find table alias - should be highlighted as Namespace
        let table_aliases: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Namespace && {
                    let text = &code[hl.range.start().into()..hl.range.end().into()];
                    text == "Валюты"
                }
            })
            .collect();

        // Should find table alias after КАК
        assert!(
            !table_aliases.is_empty(),
            "Expected table alias 'Валюты' highlighted as Namespace"
        );

        // Find table name - should be highlighted as Type
        let table_names: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Type && {
                    let text = &code[hl.range.start().into()..hl.range.end().into()];
                    text == "Справочник" || text == "Валюты"
                }
            })
            .collect();

        assert!(
            !table_names.is_empty(),
            "Expected table names (Справочник, Валюты) highlighted as Type"
        );
    }

    #[test]
    fn test_sdbl_user_example_highlighting() {
        let code = r#"
Функция Тест()
    ТекстЗапроса =
    "ВЫБРАТЬ
    |    Валюты.Наименование КАК СимвольныйКод
    |ИЗ
    |    Справочник.Валюты КАК Валюты";
    Возврат ТекстЗапроса;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        println!("\n=== Highlights for user example ===");
        for hl in &highlights {
            let text = &code[hl.range.start().into()..hl.range.end().into()];
            if text.contains("Валюты")
                || text.contains("Наименование")
                || text.contains("СимвольныйКод")
                || text.contains("Справочник")
            {
                println!("{:?}: '{}'", hl.tag, text);
            }
        }

        // Check that different elements have different tags
        let mut has_type = false;
        let mut has_namespace = false;
        let mut has_enum_member = false;
        let mut has_property_or_unresolved = false;

        for hl in &highlights {
            let text = &code[hl.range.start().into()..hl.range.end().into()];
            match hl.tag {
                HlTag::Type if text.contains("Справочник") || text.contains("Валюты") =>
                {
                    has_type = true;
                }
                HlTag::Namespace if text == "Валюты" => {
                    has_namespace = true;
                }
                HlTag::EnumMember if text == "СимвольныйКод" => {
                    has_enum_member = true;
                }
                HlTag::Property | HlTag::UnresolvedReference if text == "Наименование" =>
                {
                    has_property_or_unresolved = true;
                }
                _ => {}
            }
        }

        assert!(has_type, "Table names should be highlighted as Type");
        assert!(has_namespace, "Table alias should be highlighted as Namespace");
        assert!(has_enum_member, "Field alias should be highlighted as EnumMember");
        assert!(
            has_property_or_unresolved,
            "Field name should be highlighted as Property or UnresolvedReference"
        );
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

    #[test]
    fn test_builtin_function_highlighting() {
        let code = r#"
Функция Тест()
    НачатьТранзакцию();
    Сообщить("Привет");
    Результат = Формат(123, "ЧГ=0");
    Возврат Результат;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Debug: print all highlights
        eprintln!("\n=== All highlights ===");
        for hl in highlights.iter() {
            let text = &code[hl.range.start().into()..hl.range.end().into()];
            if !text.trim().is_empty() && text.chars().all(|c| c.is_alphabetic() || c == '_') {
                eprintln!("{:?}: {:?} {:?}", text, hl.tag, hl.modifiers);
            }
        }
        eprintln!("======================\n");

        // Check НачатьТранзакцию - should be highlighted as BuiltinFunction with EXPORT modifier
        let begin_trans = highlights.iter().find(|hl| {
            hl.tag == HlTag::BuiltinFunction
                && code[hl.range.start().into()..hl.range.end().into()] == *"НачатьТранзакцию"
        });

        assert!(begin_trans.is_some(), "НачатьТранзакцию should be highlighted as BuiltinFunction");
        assert!(
            begin_trans.unwrap().modifiers.contains(HlMod::EXPORT),
            "BuiltinFunction should have EXPORT modifier (defaultLibrary)"
        );

        // Check Сообщить
        let message_fn = highlights.iter().find(|hl| {
            hl.tag == HlTag::BuiltinFunction
                && code[hl.range.start().into()..hl.range.end().into()] == *"Сообщить"
        });

        assert!(message_fn.is_some(), "Сообщить should be highlighted as BuiltinFunction");

        // Check Формат
        let format_fn = highlights.iter().find(|hl| {
            hl.tag == HlTag::BuiltinFunction
                && code[hl.range.start().into()..hl.range.end().into()] == *"Формат"
        });

        assert!(format_fn.is_some(), "Формат should be highlighted as BuiltinFunction");
    }

    #[test]
    fn test_builtin_vs_user_function() {
        let code = r#"
Функция МояФункция()
    Возврат 42;
КонецФункции

Функция Тест()
    // User-defined function call - should be Function (not BuiltinFunction)
    Результат1 = МояФункция();

    // Built-in platform function - should be BuiltinFunction
    Результат2 = НачатьТранзакцию();

    Возврат Результат1 + Результат2;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // МояФункция should be Function (user-defined)
        let my_func_call = highlights.iter().find(|hl| {
            hl.tag == HlTag::Function
                && code[hl.range.start().into()..hl.range.end().into()] == *"МояФункция"
                && !hl.modifiers.contains(HlMod::EXPORT)
        });

        assert!(
            my_func_call.is_some(),
            "МояФункция should be highlighted as Function (not builtin)"
        );

        // НачатьТранзакцию should be BuiltinFunction
        let builtin_call = highlights.iter().find(|hl| {
            hl.tag == HlTag::BuiltinFunction
                && code[hl.range.start().into()..hl.range.end().into()] == *"НачатьТранзакцию"
        });

        assert!(
            builtin_call.is_some(),
            "НачатьТранзакцию should be highlighted as BuiltinFunction"
        );
    }

    #[test]
    fn test_highlight_mdo_plural_forms_russian() {
        let code = r#"
Функция ПолучитьДокумент()
    Ссылка = Документы.ПКО.НайтиПоНомеру("001");
    Возврат Ссылка;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // "Документы" should be highlighted as Class (MDO plural form)
        let documents_highlight = highlights.iter().find(|hl| {
            hl.tag == HlTag::Class
                && code[hl.range.start().into()..hl.range.end().into()] == *"Документы"
        });

        assert!(documents_highlight.is_some(), "Документы should be highlighted as Class");
    }

    #[test]
    fn test_highlight_mdo_plural_forms_english() {
        let code = r#"
Function GetCatalog()
    Ref = Catalogs.Currencies.FindByCode("USD");
    Return Ref;
EndFunction
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        let catalogs_highlight = highlights.iter().find(|hl| {
            hl.tag == HlTag::Class
                && code[hl.range.start().into()..hl.range.end().into()] == *"Catalogs"
        });

        assert!(catalogs_highlight.is_some(), "Catalogs should be highlighted as Class");
    }

    #[test]
    fn test_highlight_mdo_plural_case_insensitive() {
        let code = r#"
Функция Тест()
    Результат1 = ДОКУМЕНТЫ.ПКО.Создать();
    Результат2 = документы.ПКО.Создать();
    Результат3 = Документы.ПКО.Создать();
    Возврат Результат1;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // Count all Class highlights that match "документы" in any case
        let documents_highlights: Vec<_> = highlights
            .iter()
            .filter(|hl| {
                if hl.tag != HlTag::Class {
                    return false;
                }
                let text = &code[hl.range.start().into()..hl.range.end().into()];
                text == "ДОКУМЕНТЫ" || text == "документы" || text == "Документы"
            })
            .collect();

        assert_eq!(
            documents_highlights.len(),
            3,
            "Expected 3 'Документы' highlights as Class (case-insensitive)"
        );
    }

    #[test]
    fn test_highlight_all_mdo_plural_forms() {
        let code = r#"
Функция ТестВсехТипов()
    А = Документы.ПКО.Создать();
    Б = Справочники.Валюты.НайтиПоКоду("USD");
    В = РегистрыСведений.КурсыВалют.СоздатьНаборЗаписей();
    Г = РегистрыНакопления.Продажи.СоздатьНаборЗаписей();
    Д = Перечисления.ВидыДвижений.Приход;
    Е = Обработки.ЗагрузкаДанных.Создать();
    Ж = Отчеты.ОстаткиТоваров.Создать();
    Возврат А;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        let expected_classes = vec![
            "Документы",
            "Справочники",
            "РегистрыСведений",
            "РегистрыНакопления",
            "Перечисления",
            "Обработки",
            "Отчеты",
        ];

        for expected in expected_classes {
            let found = highlights.iter().any(|hl| {
                hl.tag == HlTag::Class
                    && code[hl.range.start().into()..hl.range.end().into()] == *expected
            });

            assert!(found, "{} should be highlighted as Class (MDO plural)", expected);
        }
    }

    #[test]
    fn test_mdo_plural_shadowed_by_local_variable() {
        let code = r#"
Функция Тест()
    Перем Документы;
    Документы = 42;
    Возврат Документы;
КонецФункции
"#;
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // All occurrences of "Документы" should be Variable, not Class
        // because local variable shadows the global MDO plural form
        let documents_as_variable = highlights
            .iter()
            .filter(|hl| {
                hl.tag == HlTag::Variable && {
                    let text = &code[hl.range.start().into()..hl.range.end().into()];
                    text == "Документы"
                }
            })
            .count();

        assert!(
            documents_as_variable >= 3,
            "Local variable 'Документы' should shadow global MDO plural (expected >=3 Variable highlights)"
        );

        // Ensure NO Class highlights for "Документы"
        let documents_as_class = highlights.iter().any(|hl| {
            hl.tag == HlTag::Class && {
                let text = &code[hl.range.start().into()..hl.range.end().into()];
                text == "Документы"
            }
        });

        assert!(
            !documents_as_class,
            "Local variable should prevent 'Документы' from being highlighted as Class"
        );
    }

    #[test]
    #[ignore = "Requires complex metadata configuration setup"]
    fn test_highlight_metadata_object_name_with_config() {
        // TODO: Add test with metadata configuration once test infrastructure is ready
        // Should highlight: РегистрыСведений.ОчередьЗапросовERP
        //                                   ^^^^^^^^^^^^^^^^^ as Type
    }

    #[test]
    fn test_highlight_metadata_object_without_config() {
        let code = r#"
Функция Тест()
    Ссылка = Документы.ПКО.НайтиПоНомеру("001");
    Возврат Ссылка;
КонецФункции
"#;

        // No metadata configuration loaded
        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // "ПКО" should NOT be highlighted as Type (no configuration)
        let metadata_name_highlight = highlights.iter().any(|hl| {
            hl.tag == HlTag::Type && code[hl.range.start().into()..hl.range.end().into()] == *"ПКО"
        });

        assert!(
            !metadata_name_highlight,
            "ПКО should not be highlighted as Type (no configuration loaded)"
        );

        // "Документы" should still be highlighted as Class
        let plural_highlight = highlights.iter().any(|hl| {
            hl.tag == HlTag::Class
                && code[hl.range.start().into()..hl.range.end().into()] == *"Документы"
        });

        assert!(plural_highlight, "Документы should still be highlighted as Class");
    }

    #[test]
    #[ignore = "Requires workspace scope and manager module infrastructure"]
    fn test_highlight_manager_module_method() {
        // This test verifies that manager module methods are properly highlighted
        // Example: РегистрыСведений.ОчередьЗапросовERP.ДобавитьВОчередь()
        //          ^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^^^^^
        //          Class            Type              Function (manager method)
        //
        // NOTE: This currently requires:
        // 1. Workspace scope in Semantics::resolve_name_to_definition()
        // 2. Manager module file registered in workspace
        // 3. resolve_three_level() implementation in resolver
        //
        // When these are implemented, this test should pass and demonstrate that
        // the Definition API correctly resolves manager module methods.
        let code = r#"
Процедура Тест()
    РегистрыСведений.ОчередьЗапросовERP.ДобавитьВОчередь();
КонецПроцедуры
        "#;

        let (db, file_id) = create_db_with_file(code);
        let highlights = highlight(&db, file_id);

        // РегистрыСведений → Class ✅
        let plural_highlight = highlights.iter().any(|hl| {
            hl.tag == HlTag::Class
                && code[hl.range.start().into()..hl.range.end().into()] == *"РегистрыСведений"
        });
        assert!(plural_highlight, "РегистрыСведений should be highlighted as Class");

        // ОчередьЗапросовERP → Type (would require configuration)
        // ДобавитьВОчередь → Function (manager method - requires workspace scope)
        let method_highlight = highlights.iter().any(|hl| {
            hl.tag == HlTag::Function
                && code[hl.range.start().into()..hl.range.end().into()] == *"ДобавитьВОчередь"
        });
        assert!(
            method_highlight,
            "ДобавитьВОчередь should be highlighted as Function (manager method)"
        );
    }
}
