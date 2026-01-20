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
    AnnotationParams,
    AnnotationParam,
    CompilerDirective,

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
    ExecuteStmt,
    BreakStmt,
    ContinueStmt,
    GotoStmt,
    LabelStmt,
    AddHandlerStmt,
    RemoveHandlerStmt,
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
    AwaitExpr,
    ParenExpr,
    Literal,
    Ident,
    ArgList,

    // Preprocessor
    PreIfDir,
    PreElsIfClause,
    PreElseClause,
    PreRegionDir,
    PreDeleteDir,
    PreInsertDir,
    PreExpr,
    PreLogicalExpr,
    PreLogicalOperand,
    PreSymbol,
    PreBoolOp,

    // SDBL (Query Language)
    // Phase 1: Basic SELECT parsing
    SdblQueryPackage,
    SdblSelectQuery,
    SdblSubquery,
    SdblUnionClause,
    SdblQuery,
    SdblLimitations,
    SdblTopClause,
    SdblSelectClause,
    SdblFieldList,
    SdblSelectedField,
    SdblAlias,
    SdblAsteriskField,
    SdblIntoClause,
    SdblTempTableName,
    SdblFromClause,
    SdblDataSource,
    SdblTableRef,
    SdblJoinClause,
    SdblWhereClause,
    SdblGroupClause,
    SdblOrderClause,
    SdblHavingClause,
    SdblForUpdate,
    SdblIndexBy,
    SdblAutoorder,
    SdblTotalsBy,
    SdblExpr,
    SdblLogicalOrExpr,
    SdblLogicalAndExpr,
    SdblNotExpr,
    SdblComparisonExpr,
    SdblInExpr,
    SdblInHierarchyExpr,
    SdblIsNullExpr,
    SdblBetweenExpr,
    SdblLikeExpr,
    SdblRefsExpr,
    SdblAdditiveExpr,
    SdblMultiplicativeExpr,
    SdblUnaryExpr,
    SdblParenExpr,
    SdblTupleExpr,
    SdblSubqueryExpr,
    SdblColumnRef,
    SdblFunctionCall,
    SdblCaseExpr,
    SdblWhenClause,
    SdblLiteral,
    SdblMultiString,
    SdblParameter,
    SdblType,
    SdblError,

    // Other
    Error,
    Comment,
}
