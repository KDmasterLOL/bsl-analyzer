//! Syntax trees for BSL language.
//!
//! This crate provides syntax tree infrastructure based on Rowan.

pub mod ast;

use rowan::Language;

/// BSL language definition for Rowan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BslLanguage {}

impl Language for BslLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        SyntaxKind::from(raw.0)
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into())
    }
}

/// Syntax node type for BSL.
pub type SyntaxNode = rowan::SyntaxNode<BslLanguage>;
/// Syntax token type for BSL.
pub type SyntaxToken = rowan::SyntaxToken<BslLanguage>;
/// Syntax element type for BSL.
pub type SyntaxElement = rowan::SyntaxElement<BslLanguage>;

/// All syntax kinds in BSL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Semicolon,
    Colon,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Question,
    Tilde,
    Number,
    String,
    Date,
    Ident,
    Comment,
    Newline,
    Whitespace,

    // Keywords
    KwProcedure,
    KwEndProcedure,
    KwFunction,
    KwEndFunction,
    KwIf,
    KwThen,
    KwElse,
    KwElsIf,
    KwEndIf,
    KwFor,
    KwEach,
    KwIn,
    KwTo,
    KwWhile,
    KwDo,
    KwEndDo,
    KwReturn,
    KwVar,
    KwTry,
    KwExcept,
    KwEndTry,
    KwRaise,
    KwNew,
    KwExport,
    KwVal,
    KwAnd,
    KwOr,
    KwNot,
    KwTrue,
    KwFalse,
    KwUndefined,
    KwNull,
    KwBreak,
    KwContinue,
    KwGoto,

    // Nodes
    SourceFile,
    ProcedureDef,
    FunctionDef,
    VarDef,
    ParamList,
    Param,
    Annotation,
    StmtList,
    AssignStmt,
    CallStmt,
    ReturnStmt,
    IfStmt,
    ElseIfClause,
    ElseClause,
    WhileStmt,
    ForStmt,
    ForEachStmt,
    TryStmt,
    ExceptClause,
    RaiseStmt,
    BreakStmt,
    ContinueStmt,
    GotoStmt,
    LabelStmt,
    EmptyStmt,
    Expr,
    BinaryExpr,
    UnaryExpr,
    TernaryExpr,
    CallExpr,
    IndexExpr,
    FieldExpr,
    NewExpr,
    ParenExpr,
    Literal,
    ArgList,

    // Preprocessor
    PreIfDir,
    PreRegionDir,

    // Special
    Error,

    // Must be last
    __LAST,
}

impl From<u16> for SyntaxKind {
    fn from(value: u16) -> Self {
        assert!(value < SyntaxKind::__LAST as u16);
        // SAFETY: We checked that value is in range
        unsafe { std::mem::transmute(value) }
    }
}

impl From<SyntaxKind> for u16 {
    fn from(kind: SyntaxKind) -> Self {
        kind as u16
    }
}

/// Result of parsing.
#[derive(Debug)]
pub struct Parse<T> {
    green: rowan::GreenNode,
    errors: Vec<String>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Parse<T> {
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}
