//! Parser events.
//!
//! The parser produces events that are later processed to build the syntax tree.

/// Parsing events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Start a new node.
    Start { kind: NodeKind, forward_parent: Option<usize> },
    /// Finish the current node.
    Finish,
    /// Add a token to the current node.
    Token { kind: lexer::TokenKind },
    /// Placeholder for nodes that will be replaced.
    Placeholder,
}

/// Node kinds for the syntax tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    // Root
    SourceFile,

    // Items
    ProcedureDef,
    FunctionDef,
    VarDef,
    ParamList,
    Param,
    Annotation,

    // Statements
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

    // Expressions
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
    Ident,
    ArgList,

    // Preprocessor
    PreIfDir,
    PreRegionDir,

    // Other
    Error,
    Comment,
}
