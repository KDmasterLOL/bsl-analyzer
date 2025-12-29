//! Typed AST wrappers for syntax nodes.
//!
//! This module provides typed access to syntax nodes.
//!
//! # Architecture
//!
//! AST nodes are zero-cost typed wrappers around untyped SyntaxNode.
//! They implement the AstNode trait which provides casting and syntax access.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Trait for AST nodes.
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

/// Source file node.
#[derive(Debug, Clone)]
pub struct SourceFile(SyntaxNode);

impl AstNode for SourceFile {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SOURCE_FILE
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Procedure definition.
#[derive(Debug, Clone)]
pub struct ProcedureDef(SyntaxNode);

impl AstNode for ProcedureDef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PROCEDURE_DEF
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ProcedureDef {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }

    pub fn export_keyword(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::KW_EXPORT)
    }

    pub fn param_list(&self) -> Option<ParamList> {
        self.0.children().find_map(ParamList::cast)
    }

    pub fn annotations(&self) -> impl Iterator<Item = Annotation> + '_ {
        self.0.children().filter_map(|node| {
            if node.kind() == SyntaxKind::COMPILER_DIRECTIVE {
                Some(Annotation(node))
            } else {
                None
            }
        })
    }

    pub fn body(&self) -> Option<StmtList> {
        self.0.children().find_map(StmtList::cast)
    }
}

/// Function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef(SyntaxNode);

impl AstNode for FunctionDef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FUNCTION_DEF
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl FunctionDef {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }

    pub fn export_keyword(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::KW_EXPORT)
    }

    pub fn param_list(&self) -> Option<ParamList> {
        self.0.children().find_map(ParamList::cast)
    }

    pub fn annotations(&self) -> impl Iterator<Item = Annotation> + '_ {
        self.0.children().filter_map(|node| {
            if node.kind() == SyntaxKind::COMPILER_DIRECTIVE {
                Some(Annotation(node))
            } else {
                None
            }
        })
    }

    pub fn body(&self) -> Option<StmtList> {
        self.0.children().find_map(StmtList::cast)
    }
}

// ==================== Variable declarations ====================

/// Variable definition.
#[derive(Debug, Clone)]
pub struct VarDef(SyntaxNode);

impl AstNode for VarDef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::VAR_DEF
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl VarDef {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }

    /// Get all variable names declared in this definition.
    ///
    /// BSL allows multiple variables in one declaration: Перем Имя1, Имя2, Имя3;
    pub fn names(&self) -> impl Iterator<Item = SyntaxToken> + '_ {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|it| it.kind() == SyntaxKind::IDENT)
    }

    pub fn export_keyword(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::KW_EXPORT)
    }
}

/// Parameter list.
#[derive(Debug, Clone)]
pub struct ParamList(SyntaxNode);

impl AstNode for ParamList {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PARAM_LIST
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ParamList {
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        self.0.children().filter_map(Param::cast)
    }
}

/// Parameter definition.
#[derive(Debug, Clone)]
pub struct Param(SyntaxNode);

impl AstNode for Param {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PARAM
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Param {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }

    pub fn val_keyword(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::KW_VAL)
    }

    pub fn default_value(&self) -> bool {
        // If parameter has assignment (=), it has a default value
        // We check for the presence of additional children beyond just the name
        self.0.children().next().is_some()
    }
}

// ==================== Statements ====================

/// Assignment statement.
#[derive(Debug, Clone)]
pub struct AssignStmt(SyntaxNode);

impl AstNode for AssignStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::ASSIGN_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Call statement.
#[derive(Debug, Clone)]
pub struct CallStmt(SyntaxNode);

impl AstNode for CallStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CALL_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Return statement.
#[derive(Debug, Clone)]
pub struct ReturnStmt(SyntaxNode);

impl AstNode for ReturnStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::RETURN_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// If statement.
#[derive(Debug, Clone)]
pub struct IfStmt(SyntaxNode);

impl AstNode for IfStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::IF_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// While statement.
#[derive(Debug, Clone)]
pub struct WhileStmt(SyntaxNode);

impl AstNode for WhileStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::WHILE_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// For statement.
#[derive(Debug, Clone)]
pub struct ForStmt(SyntaxNode);

impl AstNode for ForStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FOR_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// For each statement.
#[derive(Debug, Clone)]
pub struct ForEachStmt(SyntaxNode);

impl AstNode for ForEachStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FOR_EACH_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Try statement.
#[derive(Debug, Clone)]
pub struct TryStmt(SyntaxNode);

impl AstNode for TryStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TRY_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Raise statement.
#[derive(Debug, Clone)]
pub struct RaiseStmt(SyntaxNode);

impl AstNode for RaiseStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::RAISE_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Break statement.
#[derive(Debug, Clone)]
pub struct BreakStmt(SyntaxNode);

impl AstNode for BreakStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::BREAK_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Continue statement.
#[derive(Debug, Clone)]
pub struct ContinueStmt(SyntaxNode);

impl AstNode for ContinueStmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CONTINUE_STMT
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

// ==================== Expressions ====================

/// Binary expression.
#[derive(Debug, Clone)]
pub struct BinaryExpr(SyntaxNode);

impl AstNode for BinaryExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::BINARY_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Unary expression.
#[derive(Debug, Clone)]
pub struct UnaryExpr(SyntaxNode);

impl AstNode for UnaryExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::UNARY_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Ternary expression (condition ? true_val : false_val).
#[derive(Debug, Clone)]
pub struct TernaryExpr(SyntaxNode);

impl AstNode for TernaryExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::TERNARY_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Call expression.
#[derive(Debug, Clone)]
pub struct CallExpr(SyntaxNode);

impl AstNode for CallExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::CALL_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Index expression (array[index]).
#[derive(Debug, Clone)]
pub struct IndexExpr(SyntaxNode);

impl AstNode for IndexExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::INDEX_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Field access expression (obj.field).
#[derive(Debug, Clone)]
pub struct FieldExpr(SyntaxNode);

impl AstNode for FieldExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FIELD_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// New expression.
#[derive(Debug, Clone)]
pub struct NewExpr(SyntaxNode);

impl AstNode for NewExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::NEW_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Parenthesized expression.
#[derive(Debug, Clone)]
pub struct ParenExpr(SyntaxNode);

impl AstNode for ParenExpr {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PAREN_EXPR
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Literal expression.
#[derive(Debug, Clone)]
pub struct Literal(SyntaxNode);

impl AstNode for Literal {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::LITERAL
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

// ==================== Annotations ====================

/// Annotation (compiler directive).
#[derive(Debug, Clone)]
pub struct Annotation(SyntaxNode);

impl AstNode for Annotation {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(kind, SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE)
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl Annotation {
    /// Get the annotation kind token (e.g., &НаКлиенте, &AtServer).
    pub fn kind_token(&self) -> Option<SyntaxToken> {
        // For COMPILER_DIRECTIVE, the annotation token is a direct child
        // For ANNOTATION, the first IDENT token after '&' is the annotation kind
        self.0.children_with_tokens().filter_map(|it| it.into_token()).find(|token| {
            matches!(
                token.kind(),
                SyntaxKind::ANN_AT_CLIENT
                    | SyntaxKind::ANN_AT_SERVER
                    | SyntaxKind::ANN_AT_SERVER_NO_CONTEXT
                    | SyntaxKind::ANN_AT_CLIENT_AT_SERVER
                    | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
                    | SyntaxKind::IDENT
            )
        })
    }
}

// ==================== Statements ====================

/// Statement list (body of a procedure/function or block).
#[derive(Debug, Clone)]
pub struct StmtList(SyntaxNode);

impl AstNode for StmtList {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::STMT_LIST
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl StmtList {
    /// Iterate over variable declarations (Перем declarations inside procedures).
    ///
    /// Note: In BSL, both module-level and local variables use the same syntax (Перем),
    /// so they share the same AST node type (VarDef).
    pub fn var_decls(&self) -> impl Iterator<Item = VarDef> + '_ {
        self.0.children().filter_map(VarDef::cast)
    }
}
