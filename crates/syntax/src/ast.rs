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

// ==================== SDBL (Query Language) ====================

/// SDBL query package (root node).
///
/// Contains one or more queries separated by semicolons.
#[derive(Debug, Clone)]
pub struct SdblQueryPackage(SyntaxNode);

impl AstNode for SdblQueryPackage {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_QUERY_PACKAGE
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

impl SdblQueryPackage {
    /// Get all SELECT queries in this package.
    pub fn queries(&self) -> impl Iterator<Item = SdblSelectQuery> + '_ {
        self.0.children().filter_map(SdblSelectQuery::cast)
    }
}

/// SDBL SELECT query statement.
#[derive(Debug, Clone)]
pub struct SdblSelectQuery(SyntaxNode);

impl AstNode for SdblSelectQuery {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_SELECT_QUERY
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

impl SdblSelectQuery {
    /// Get the subquery (includes main query and UNIONs).
    pub fn subquery(&self) -> Option<SdblSubquery> {
        self.0.children().find_map(SdblSubquery::cast)
    }
}

/// SDBL subquery (main query + optional UNIONs).
#[derive(Debug, Clone)]
pub struct SdblSubquery(SyntaxNode);

impl AstNode for SdblSubquery {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_SUBQUERY
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

impl SdblSubquery {
    /// Get the main query (first direct SdblQuery child, not UNION queries).
    ///
    /// CRITICAL for AssignAliasFieldsInQuery: Only main query is checked, not UNIONs.
    pub fn main_query(&self) -> Option<SdblQuery> {
        self.0.children().find_map(SdblQuery::cast)
    }

    /// Get UNION clauses.
    pub fn union_clauses(&self) -> impl Iterator<Item = SdblUnionClause> + '_ {
        self.0.children().filter_map(SdblUnionClause::cast)
    }

    /// Get all queries (main query + queries from UNION clauses).
    pub fn queries(&self) -> impl Iterator<Item = SdblQuery> + '_ {
        // First the main query
        let main = self.main_query().into_iter();
        // Then queries from UNION clauses
        let unions = self.union_clauses().filter_map(|union_clause| union_clause.query());
        main.chain(unions)
    }

    /// Get UNION queries (queries from UNION clauses, excluding main query).
    pub fn union_queries(&self) -> impl Iterator<Item = SdblQuery> + '_ {
        self.union_clauses().filter_map(|union_clause| union_clause.query())
    }
}

/// SDBL UNION clause.
#[derive(Debug, Clone)]
pub struct SdblUnionClause(SyntaxNode);

impl AstNode for SdblUnionClause {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_UNION_CLAUSE
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

impl SdblUnionClause {
    /// Get the query inside this UNION clause.
    pub fn query(&self) -> Option<SdblQuery> {
        self.0.children().find_map(SdblQuery::cast)
    }

    /// Check if this UNION has ALL keyword.
    pub fn has_all(&self) -> bool {
        self.0.children_with_tokens().filter_map(|it| it.into_token()).any(|t| {
            if t.kind() == SyntaxKind::IDENT {
                let text = t.text();
                text.eq_ignore_ascii_case("ALL") || text.eq_ignore_ascii_case("ВСЕ")
            } else {
                false
            }
        })
    }
}

/// Individual SDBL SELECT query.
#[derive(Debug, Clone)]
pub struct SdblQuery(SyntaxNode);

impl AstNode for SdblQuery {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_QUERY
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

impl SdblQuery {
    /// Get the field list.
    pub fn field_list(&self) -> Option<SdblFieldList> {
        self.0.children().find_map(SdblFieldList::cast)
    }

    /// Get the FROM clause.
    pub fn from_clause(&self) -> Option<SdblFromClause> {
        self.0.children().find_map(SdblFromClause::cast)
    }

    /// Get the WHERE clause.
    pub fn where_clause(&self) -> Option<SdblWhereClause> {
        self.0.children().find_map(SdblWhereClause::cast)
    }
}

/// SDBL field list.
#[derive(Debug, Clone)]
pub struct SdblFieldList(SyntaxNode);

impl AstNode for SdblFieldList {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_FIELD_LIST
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

impl SdblFieldList {
    /// Get all selected fields.
    pub fn fields(&self) -> impl Iterator<Item = SdblSelectedField> + '_ {
        self.0.children().filter_map(SdblSelectedField::cast)
    }
}

/// SDBL selected field (expression + optional alias).
///
/// **CRITICAL FOR AssignAliasFieldsInQuery DIAGNOSTIC**
#[derive(Debug, Clone)]
pub struct SdblSelectedField(SyntaxNode);

impl AstNode for SdblSelectedField {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_SELECTED_FIELD
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

impl SdblSelectedField {
    /// Check if this is an asterisk field (* or Table.*).
    ///
    /// Asterisk fields don't need aliases according to diagnostic rules.
    pub fn is_asterisk(&self) -> bool {
        self.0.children().any(|n| n.kind() == SyntaxKind::SDBL_ASTERISK_FIELD)
    }

    /// Get the alias (if present).
    pub fn alias(&self) -> Option<SdblAlias> {
        self.0.children().find_map(SdblAlias::cast)
    }

    /// Get the expression (column reference, function call, etc.).
    pub fn expression(&self) -> Option<SyntaxNode> {
        self.0.children().find(|n| {
            matches!(
                n.kind(),
                SyntaxKind::SDBL_COLUMN_REF
                    | SyntaxKind::SDBL_FUNCTION_CALL
                    | SyntaxKind::SDBL_LITERAL
                    | SyntaxKind::SDBL_PAREN_EXPR
                    | SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | SyntaxKind::SDBL_LOGICAL_AND_EXPR
                    | SyntaxKind::SDBL_NOT_EXPR
                    | SyntaxKind::SDBL_COMPARISON_EXPR
                    | SyntaxKind::SDBL_ADDITIVE_EXPR
                    | SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                    | SyntaxKind::SDBL_UNARY_EXPR
            )
        })
    }
}

/// SDBL alias ([AS] identifier).
///
/// **CRITICAL FOR AssignAliasFieldsInQuery DIAGNOSTIC**
#[derive(Debug, Clone)]
pub struct SdblAlias(SyntaxNode);

impl AstNode for SdblAlias {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_ALIAS
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

impl SdblAlias {
    /// Check if alias has AS/КАК keyword.
    ///
    /// **CRITICAL for AssignAliasFieldsInQuery diagnostic:**
    /// Returns true if AS/КАК keyword is present (explicit alias).
    /// Returns false for implicit aliases (just identifier without AS).
    ///
    /// # Example
    /// ```sdbl
    /// Name AS ProductName  // has_as_keyword() = true
    /// Name ProductName     // has_as_keyword() = false (implicit alias - error!)
    /// ```
    pub fn has_as_keyword(&self) -> bool {
        self.0.children_with_tokens().filter_map(|it| it.into_token()).any(|t| {
            // Check if token is IDENT with text "AS" or "КАК" (case-insensitive)
            // This is needed because SDBL keywords are mapped to Ident in token converter
            if t.kind() == SyntaxKind::IDENT {
                let text = t.text();
                text.eq_ignore_ascii_case("AS") || text.eq_ignore_ascii_case("КАК")
            } else {
                false
            }
        })
    }

    /// Get the identifier token (alias name).
    pub fn identifier(&self) -> Option<SyntaxToken> {
        // Get the last IDENT token (after AS keyword if present)
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .filter(|t| t.kind() == SyntaxKind::IDENT)
            .filter(|t| {
                // Filter out AS/КАК keywords
                let text = t.text();
                !text.eq_ignore_ascii_case("AS") && !text.eq_ignore_ascii_case("КАК")
            })
            .last()
    }

    /// Get the alias name.
    pub fn name(&self) -> Option<String> {
        self.identifier().map(|tok| tok.text().to_string())
    }
}

/// SDBL asterisk field (* or Table.*).
#[derive(Debug, Clone)]
pub struct SdblAsteriskField(SyntaxNode);

impl AstNode for SdblAsteriskField {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_ASTERISK_FIELD
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

/// SDBL FROM clause.
#[derive(Debug, Clone)]
pub struct SdblFromClause(SyntaxNode);

impl AstNode for SdblFromClause {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_FROM_CLAUSE
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

impl SdblFromClause {
    /// Get all data sources.
    pub fn data_sources(&self) -> impl Iterator<Item = SdblDataSource> + '_ {
        self.0.children().filter_map(SdblDataSource::cast)
    }
}

/// SDBL data source (table or subquery in FROM clause).
#[derive(Debug, Clone)]
pub struct SdblDataSource(SyntaxNode);

impl AstNode for SdblDataSource {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_DATA_SOURCE
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

impl SdblDataSource {
    /// Get the table reference (if this is a table, not a subquery).
    pub fn table_ref(&self) -> Option<SdblTableRef> {
        self.0.children().find_map(SdblTableRef::cast)
    }

    /// Get the subquery (if this is a subquery, not a table).
    pub fn subquery(&self) -> Option<SdblSubquery> {
        self.0.children().find_map(SdblSubquery::cast)
    }

    /// Get the alias (for table or subquery).
    pub fn alias(&self) -> Option<SdblAlias> {
        self.0.children().find_map(SdblAlias::cast)
    }
}

/// SDBL table reference.
#[derive(Debug, Clone)]
pub struct SdblTableRef(SyntaxNode);

impl AstNode for SdblTableRef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_TABLE_REF
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

/// SDBL WHERE clause.
#[derive(Debug, Clone)]
pub struct SdblWhereClause(SyntaxNode);

impl AstNode for SdblWhereClause {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SDBL_WHERE_CLAUSE
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
