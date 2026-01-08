//! Semantic syntax highlighting for BSL.
//!
//! This module provides semantic highlighting by analyzing the HIR
//! and assigning semantic token types and modifiers to each token.
//!
//! ## Architecture
//!
//! 1. **Syntactic highlighting** - Keywords, literals, comments, operators (by token kind)
//! 2. **Semantic highlighting** - Function calls, variables, parameters (via HIR + name resolution)
//!
//! Semantic highlighting requires name resolution to distinguish:
//! - Function calls from variables
//! - Parameters from local variables
//! - Module variables from local variables
//! - Builtin functions from user-defined (TODO)

use ide_db::{
    hir_def::{resolver::Resolver, ModuleId, Name},
    RootDatabase, TextRange,
};

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

/// Generates semantic highlighting for a file.
pub fn highlight(db: &dyn RootDatabase, file_id: FileId) -> Vec<HlRange> {
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let module_id = ModuleId::new(file_id);

    let mut highlights = Vec::new();
    traverse_node(db, module_id, file_id, &root, &mut highlights);
    highlights
}

/// Recursively traverse AST and collect highlights.
fn traverse_node(
    db: &dyn RootDatabase,
    module_id: ModuleId,
    file_id: FileId,
    node: &SyntaxNode,
    highlights: &mut Vec<HlRange>,
) {
    // Highlight tokens based on their type
    for token in node.children_with_tokens() {
        match token {
            syntax::NodeOrToken::Token(token) => {
                // Try semantic highlighting for IDENT tokens first
                if token.kind() == SyntaxKind::IDENT {
                    if let Some(hl) = highlight_ident_semantic(db, module_id, file_id, &token) {
                        highlights.push(hl);
                        continue;
                    }
                }

                // Fall back to syntactic highlighting
                if let Some(hl) = highlight_token(&token) {
                    highlights.push(hl);
                }
            }
            syntax::NodeOrToken::Node(node) => {
                // Check for special nodes that need specific highlighting
                highlight_node(&node, highlights);

                // Recurse into children
                traverse_node(db, module_id, file_id, &node, highlights);
            }
        }
    }
}

/// Highlight an IDENT token using semantic analysis (name resolution).
///
/// This function resolves the identifier to determine if it's a:
/// - Function/procedure call
/// - Parameter reference
/// - Local variable reference
/// - Module variable reference
fn highlight_ident_semantic(
    db: &dyn RootDatabase,
    module_id: ModuleId,
    _file_id: FileId,
    token: &SyntaxToken,
) -> Option<HlRange> {
    let range = token.text_range();
    let name_text = token.text();
    let name = Name::new(name_text);

    // Try module-level resolution first (methods and module variables)
    let resolver = Resolver::for_module(module_id);

    if let Some(_method_id) = resolver.resolve_module_method(db, &name) {
        return Some(HlRange { range, tag: HlTag::Function, modifiers: HlMod::new() });
    }

    if let Some(_var_id) = resolver.resolve_module_variable(db, &name) {
        return Some(HlRange { range, tag: HlTag::Variable, modifiers: HlMod::new() });
    }

    // TODO: Resolve local variables and parameters
    // This requires knowing which method we're inside and building ExprScopes
    // For now, we don't highlight unresolved identifiers semantically

    None
}

/// Highlight a single token based on its syntax kind.
fn highlight_token(token: &SyntaxToken) -> Option<HlRange> {
    let kind = token.kind();
    let range = token.text_range();

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
}
