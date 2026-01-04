//! Conversion between parser's NodeKind/TokenKind and syntax's SyntaxKind.
//!
//! This module provides mappings to convert parser events into Rowan syntax kinds.

use lexer::TokenKind;

use crate::event::NodeKind;

/// Convert NodeKind to SyntaxKind.
pub fn node_kind_to_syntax(kind: NodeKind) -> syntax::SyntaxKind {
    use syntax::SyntaxKind as SK;

    match kind {
        // Root
        NodeKind::SourceFile => SK::SOURCE_FILE,

        // Items
        NodeKind::ProcedureDef => SK::PROCEDURE_DEF,
        NodeKind::FunctionDef => SK::FUNCTION_DEF,
        NodeKind::VarDef => SK::VAR_DEF,
        NodeKind::ParamList => SK::PARAM_LIST,
        NodeKind::Param => SK::PARAM,
        NodeKind::Annotation => SK::ANNOTATION,
        NodeKind::AnnotationParams => SK::ANNOTATION_PARAMS,
        NodeKind::AnnotationParam => SK::ANNOTATION_PARAM,
        NodeKind::CompilerDirective => SK::COMPILER_DIRECTIVE,

        // Statements
        NodeKind::StmtList => SK::STMT_LIST,
        NodeKind::AssignStmt => SK::ASSIGN_STMT,
        NodeKind::CallStmt => SK::CALL_STMT,
        NodeKind::ReturnStmt => SK::RETURN_STMT,
        NodeKind::IfStmt => SK::IF_STMT,
        NodeKind::ElseIfClause => SK::ELSIF_CLAUSE,
        NodeKind::ElseClause => SK::ELSE_CLAUSE,
        NodeKind::WhileStmt => SK::WHILE_STMT,
        NodeKind::ForStmt => SK::FOR_STMT,
        NodeKind::ForEachStmt => SK::FOR_EACH_STMT,
        NodeKind::TryStmt => SK::TRY_STMT,
        NodeKind::ExceptClause => SK::EXCEPT_CLAUSE,
        NodeKind::RaiseStmt => SK::RAISE_STMT,
        NodeKind::ExecuteStmt => SK::EXECUTE_STMT,
        NodeKind::BreakStmt => SK::BREAK_STMT,
        NodeKind::ContinueStmt => SK::CONTINUE_STMT,
        NodeKind::GotoStmt => SK::GOTO_STMT,
        NodeKind::LabelStmt => SK::LABEL_STMT,
        NodeKind::AddHandlerStmt => SK::ADD_HANDLER_STMT,
        NodeKind::RemoveHandlerStmt => SK::REMOVE_HANDLER_STMT,
        NodeKind::EmptyStmt => SK::EMPTY_STMT,

        // Expressions
        NodeKind::Expr => SK::EXPR,
        NodeKind::BinaryExpr => SK::BINARY_EXPR,
        NodeKind::UnaryExpr => SK::UNARY_EXPR,
        NodeKind::TernaryExpr => SK::TERNARY_EXPR,
        NodeKind::CallExpr => SK::CALL_EXPR,
        NodeKind::IndexExpr => SK::INDEX_EXPR,
        NodeKind::FieldExpr => SK::FIELD_EXPR,
        NodeKind::NewExpr => SK::NEW_EXPR,
        NodeKind::AwaitExpr => SK::AWAIT_EXPR,
        NodeKind::ParenExpr => SK::PAREN_EXPR,
        NodeKind::Literal => SK::LITERAL,
        NodeKind::Ident => SK::IDENT,
        NodeKind::ArgList => SK::ARG_LIST,

        // Preprocessor
        NodeKind::PreIfDir => SK::PRE_IF_DIR,
        NodeKind::PreElsIfClause => SK::PRE_ELSIF_CLAUSE,
        NodeKind::PreElseClause => SK::PRE_ELSE_CLAUSE,
        NodeKind::PreRegionDir => SK::PRE_REGION_DIR,
        NodeKind::PreDeleteDir => SK::PRE_DELETE_DIR,
        NodeKind::PreInsertDir => SK::PRE_INSERT_DIR,
        NodeKind::PreExpr => SK::PRE_EXPR,
        NodeKind::PreLogicalExpr => SK::PRE_LOGICAL_EXPR,
        NodeKind::PreLogicalOperand => SK::PRE_LOGICAL_OPERAND,
        NodeKind::PreSymbol => SK::PRE_SYMBOL,
        NodeKind::PreBoolOp => SK::PRE_BOOL_OP,

        // SDBL (Query Language)
        NodeKind::SdblQueryPackage => SK::SDBL_QUERY_PACKAGE,
        NodeKind::SdblSelectQuery => SK::SDBL_SELECT_QUERY,
        NodeKind::SdblSubquery => SK::SDBL_SUBQUERY,
        NodeKind::SdblUnionClause => SK::SDBL_UNION_CLAUSE,
        NodeKind::SdblQuery => SK::SDBL_QUERY,
        NodeKind::SdblSelectClause => SK::SDBL_SELECT_CLAUSE,
        NodeKind::SdblFieldList => SK::SDBL_FIELD_LIST,
        NodeKind::SdblSelectedField => SK::SDBL_SELECTED_FIELD,
        NodeKind::SdblAlias => SK::SDBL_ALIAS,
        NodeKind::SdblAsteriskField => SK::SDBL_ASTERISK_FIELD,
        NodeKind::SdblFromClause => SK::SDBL_FROM_CLAUSE,
        NodeKind::SdblDataSource => SK::SDBL_DATA_SOURCE,
        NodeKind::SdblTableRef => SK::SDBL_TABLE_REF,
        NodeKind::SdblJoinClause => SK::SDBL_JOIN_CLAUSE,
        NodeKind::SdblWhereClause => SK::SDBL_WHERE_CLAUSE,
        NodeKind::SdblExpr => SK::SDBL_EXPR,
        NodeKind::SdblLogicalOrExpr => SK::SDBL_LOGICAL_OR_EXPR,
        NodeKind::SdblLogicalAndExpr => SK::SDBL_LOGICAL_AND_EXPR,
        NodeKind::SdblNotExpr => SK::SDBL_NOT_EXPR,
        NodeKind::SdblComparisonExpr => SK::SDBL_COMPARISON_EXPR,
        NodeKind::SdblInExpr => SK::SDBL_IN_EXPR,
        NodeKind::SdblAdditiveExpr => SK::SDBL_ADDITIVE_EXPR,
        NodeKind::SdblMultiplicativeExpr => SK::SDBL_MULTIPLICATIVE_EXPR,
        NodeKind::SdblUnaryExpr => SK::SDBL_UNARY_EXPR,
        NodeKind::SdblParenExpr => SK::SDBL_PAREN_EXPR,
        NodeKind::SdblSubqueryExpr => SK::SDBL_SUBQUERY_EXPR,
        NodeKind::SdblColumnRef => SK::SDBL_COLUMN_REF,
        NodeKind::SdblFunctionCall => SK::SDBL_FUNCTION_CALL,
        NodeKind::SdblLiteral => SK::SDBL_LITERAL,
        NodeKind::SdblMultiString => SK::SDBL_MULTI_STRING,
        NodeKind::SdblParameter => SK::SDBL_PARAMETER,
        NodeKind::SdblError => SK::SDBL_ERROR,

        // Other
        NodeKind::Error => SK::ERROR,
        NodeKind::Comment => SK::COMMENT,
    }
}

/// Convert TokenKind to SyntaxKind.
pub fn token_kind_to_syntax(kind: TokenKind) -> syntax::SyntaxKind {
    use syntax::SyntaxKind as SK;

    match kind {
        // Keywords
        TokenKind::KwProcedure => SK::KW_PROCEDURE,
        TokenKind::KwEndProcedure => SK::KW_END_PROCEDURE,
        TokenKind::KwFunction => SK::KW_FUNCTION,
        TokenKind::KwEndFunction => SK::KW_END_FUNCTION,
        TokenKind::KwExport => SK::KW_EXPORT,
        TokenKind::KwVal => SK::KW_VAL,
        TokenKind::KwIf => SK::KW_IF,
        TokenKind::KwThen => SK::KW_THEN,
        TokenKind::KwElsIf => SK::KW_ELSIF,
        TokenKind::KwElse => SK::KW_ELSE,
        TokenKind::KwEndIf => SK::KW_END_IF,
        TokenKind::KwFor => SK::KW_FOR,
        TokenKind::KwEach => SK::KW_EACH,
        TokenKind::KwIn => SK::KW_IN,
        TokenKind::KwTo => SK::KW_TO,
        TokenKind::KwWhile => SK::KW_WHILE,
        TokenKind::KwDo => SK::KW_DO,
        TokenKind::KwEndDo => SK::KW_END_DO,
        TokenKind::KwReturn => SK::KW_RETURN,
        TokenKind::KwContinue => SK::KW_CONTINUE,
        TokenKind::KwBreak => SK::KW_BREAK,
        TokenKind::KwGoto => SK::KW_GOTO,
        TokenKind::KwTry => SK::KW_TRY,
        TokenKind::KwExcept => SK::KW_EXCEPT,
        TokenKind::KwEndTry => SK::KW_END_TRY,
        TokenKind::KwRaise => SK::KW_RAISE,
        TokenKind::KwVar => SK::KW_VAR,
        TokenKind::KwNew => SK::KW_NEW,
        TokenKind::KwExecute => SK::KW_EXECUTE,
        TokenKind::KwAddHandler => SK::KW_ADD_HANDLER,
        TokenKind::KwRemoveHandler => SK::KW_REMOVE_HANDLER,
        TokenKind::KwAsync => SK::KW_ASYNC,
        TokenKind::KwAwait => SK::KW_AWAIT,
        TokenKind::KwAnd => SK::KW_AND,
        TokenKind::KwOr => SK::KW_OR,
        TokenKind::KwNot => SK::KW_NOT,
        TokenKind::KwTrue => SK::KW_TRUE,
        TokenKind::KwFalse => SK::KW_FALSE,
        TokenKind::KwUndefined => SK::KW_UNDEFINED,
        TokenKind::KwNull => SK::KW_NULL,

        // Preprocessor directives
        TokenKind::PreIf => SK::PRE_IF,
        TokenKind::PreElsIf => SK::PRE_ELSIF,
        TokenKind::PreElse => SK::PRE_ELSE,
        TokenKind::PreEndIf => SK::PRE_END_IF,
        TokenKind::PreRegion => SK::PRE_REGION,
        TokenKind::PreEndRegion => SK::PRE_END_REGION,
        TokenKind::PreUse => SK::PRE_USE,
        TokenKind::PreInsert => SK::PRE_INSERT,
        TokenKind::PreEndInsert => SK::PRE_END_INSERT,
        TokenKind::PreDelete => SK::PRE_DELETE,
        TokenKind::PreEndDelete => SK::PRE_END_DELETE,

        // Annotations
        TokenKind::AnnAtClient => SK::ANN_AT_CLIENT,
        TokenKind::AnnAtServer => SK::ANN_AT_SERVER,
        TokenKind::AnnAtServerNoContext => SK::ANN_AT_SERVER_NO_CONTEXT,
        TokenKind::AnnAtClientAtServerNoContext => SK::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT,
        TokenKind::AnnAtClientAtServer => SK::ANN_AT_CLIENT_AT_SERVER,
        TokenKind::AnnBefore => SK::ANN_BEFORE,
        TokenKind::AnnAfter => SK::ANN_AFTER,
        TokenKind::AnnAround => SK::ANN_AROUND,
        TokenKind::AnnChangeAndValidate => SK::ANN_CHANGE_AND_VALIDATE,
        TokenKind::AnnCustom => SK::ANN_CUSTOM,

        // Operators
        TokenKind::Eq => SK::EQ,
        TokenKind::Neq => SK::NEQ,
        TokenKind::Le => SK::LE,
        TokenKind::Lt => SK::LT,
        TokenKind::Ge => SK::GE,
        TokenKind::Gt => SK::GT,
        TokenKind::Plus => SK::PLUS,
        TokenKind::Minus => SK::MINUS,
        TokenKind::Star => SK::STAR,
        TokenKind::Slash => SK::SLASH,
        TokenKind::Percent => SK::PERCENT,

        // Punctuation
        TokenKind::LParen => SK::L_PAREN,
        TokenKind::RParen => SK::R_PAREN,
        TokenKind::LBracket => SK::L_BRACKET,
        TokenKind::RBracket => SK::R_BRACKET,
        TokenKind::Dot => SK::DOT,
        TokenKind::Comma => SK::COMMA,
        TokenKind::Semicolon => SK::SEMICOLON,
        TokenKind::Colon => SK::COLON,
        TokenKind::Question => SK::QUESTION,
        TokenKind::Tilde => SK::TILDE,
        TokenKind::Bar => SK::BAR,
        TokenKind::Hash => SK::HASH,
        TokenKind::Ampersand => SK::AMPERSAND,
        TokenKind::Exclamation => SK::EXCLAMATION,

        // Literals
        TokenKind::Float => SK::FLOAT,
        TokenKind::Decimal => SK::DECIMAL,
        TokenKind::String => SK::STRING,
        TokenKind::StringStart => SK::STRING_START,
        TokenKind::StringTail => SK::STRING_TAIL,
        TokenKind::StringPart => SK::STRING_PART,
        TokenKind::Date => SK::DATE,

        // Identifiers
        TokenKind::Ident => SK::IDENT,

        // Trivia
        TokenKind::Whitespace => SK::WHITESPACE,
        TokenKind::Newline => SK::NEWLINE,
        TokenKind::Comment => SK::COMMENT,

        // Error fallback
        TokenKind::Error => SK::ERROR,
    }
}
