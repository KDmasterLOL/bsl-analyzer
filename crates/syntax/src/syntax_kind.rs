//! Defines SyntaxKind - an enum of all possible syntactic constructs in BSL.
//!
//! This includes both tokens (from lexer) and composite nodes (from parser).

use std::fmt;

/// All syntax kinds in BSL language.
///
/// This enum includes:
/// - Special markers (TOMBSTONE, EOF)
/// - All token kinds from lexer
/// - All composite node kinds from parser
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
// Allow non-camel-case for token names (EOF, KW_PROCEDURE, etc.) which follow
// constant naming convention and are widely used in parser/lexer literature.
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    /// Placeholder for deleted nodes (never appears in final tree)
    #[doc(hidden)]
    TOMBSTONE,
    /// End of file marker
    EOF,

    WHITESPACE,
    NEWLINE,
    COMMENT,
    BOM, // UTF-8 BOM (Byte Order Mark)

    // Procedure/Function keywords
    KW_PROCEDURE,
    KW_END_PROCEDURE,
    KW_FUNCTION,
    KW_END_FUNCTION,
    KW_EXPORT,
    KW_VAL,

    // Control flow keywords
    KW_IF,
    KW_THEN,
    KW_ELSIF,
    KW_ELSE,
    KW_END_IF,

    // Loop keywords
    KW_FOR,
    KW_EACH,
    KW_IN,
    KW_TO,
    KW_WHILE,
    KW_DO,
    KW_END_DO,
    KW_RETURN,
    KW_CONTINUE,
    KW_BREAK,
    KW_GOTO,

    // Exception handling
    KW_TRY,
    KW_EXCEPT,
    KW_END_TRY,
    KW_RAISE,

    // Variable and value keywords
    KW_VAR,
    KW_NEW,
    KW_EXECUTE,

    // Event handlers
    KW_ADD_HANDLER,
    KW_REMOVE_HANDLER,

    // Async/Await
    KW_ASYNC,
    KW_AWAIT,

    // Logical operators
    KW_AND,
    KW_OR,
    KW_NOT,

    // Boolean literals
    KW_TRUE,
    KW_FALSE,

    // Special values
    KW_UNDEFINED,
    KW_NULL,

    PRE_IF,
    PRE_ELSIF,
    PRE_ELSE,
    PRE_END_IF,
    PRE_REGION,
    PRE_END_REGION,
    PRE_USE,
    PRE_INSERT,
    PRE_END_INSERT,
    PRE_DELETE,
    PRE_END_DELETE,

    ANN_AT_CLIENT,
    ANN_AT_SERVER,
    ANN_AT_SERVER_NO_CONTEXT,
    ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT,
    ANN_AT_CLIENT_AT_SERVER,
    ANN_BEFORE,
    ANN_AFTER,
    ANN_AROUND,
    ANN_CHANGE_AND_VALIDATE,
    ANN_CUSTOM,

    EQ,      // =
    NEQ,     // <>
    LE,      // <=
    LT,      // <
    GE,      // >=
    GT,      // >
    PLUS,    // +
    MINUS,   // -
    STAR,    // *
    SLASH,   // /
    PERCENT, // %

    L_PAREN,     // (
    R_PAREN,     // )
    L_BRACKET,   // [
    R_BRACKET,   // ]
    DOT,         // .
    COMMA,       // ,
    SEMICOLON,   // ;
    COLON,       // :
    QUESTION,    // ?
    TILDE,       // ~
    BAR,         // |
    HASH,        // #
    AMPERSAND,   // &
    EXCLAMATION, // !

    FLOAT,        // 123.45
    DECIMAL,      // 123
    STRING,       // "text"
    STRING_START, // "start... (multiline start)
    STRING_TAIL,  // |...end" (multiline end)
    STRING_PART,  // |...part... (multiline middle)
    DATE,         // '20240101' or '20240101120000'

    IDENT, // identifier

    // Root
    SOURCE_FILE,

    // Items
    PROCEDURE_DEF,
    FUNCTION_DEF,
    VAR_DEF,
    PARAM_LIST,
    PARAM,
    ANNOTATION,
    ANNOTATION_PARAMS,
    ANNOTATION_PARAM,
    COMPILER_DIRECTIVE,

    // Statements
    STMT_LIST,
    ASSIGN_STMT,
    CALL_STMT,
    RETURN_STMT,
    IF_STMT,
    ELSIF_CLAUSE,
    ELSE_CLAUSE,
    WHILE_STMT,
    FOR_STMT,
    FOR_EACH_STMT,
    TRY_STMT,
    EXCEPT_CLAUSE,
    RAISE_STMT,
    EXECUTE_STMT,
    BREAK_STMT,
    CONTINUE_STMT,
    GOTO_STMT,
    LABEL_STMT,
    ADD_HANDLER_STMT,
    REMOVE_HANDLER_STMT,
    EMPTY_STMT,

    // Expressions
    EXPR,
    BINARY_EXPR,
    UNARY_EXPR,
    TERNARY_EXPR,
    CALL_EXPR,
    INDEX_EXPR,
    FIELD_EXPR,
    NEW_EXPR,
    AWAIT_EXPR,
    PAREN_EXPR,
    LITERAL,
    ARG_LIST,

    // Preprocessor
    PRE_IF_DIR,
    PRE_ELSIF_CLAUSE,
    PRE_ELSE_CLAUSE,
    PRE_REGION_DIR,
    PRE_DELETE_DIR,
    PRE_INSERT_DIR,
    PRE_EXPR,
    PRE_LOGICAL_EXPR,
    PRE_LOGICAL_OPERAND,
    PRE_SYMBOL,
    PRE_BOOL_OP,

    // Phase 1 (MVP): Basic SELECT parsing for AssignAliasFieldsInQuery diagnostic
    // Phase 2-4: Complete SDBL grammar (JOINs, GROUP BY, ORDER BY, etc.)

    // Query Structure
    SDBL_QUERY_PACKAGE, // Root: queries separated by semicolons
    SDBL_SELECT_QUERY,  // SELECT query statement
    SDBL_SUBQUERY,      // Main query + UNIONs
    SDBL_UNION_CLAUSE,  // UNION [ALL] query
    SDBL_QUERY,         // Individual SELECT query
    SDBL_LIMITATIONS,   // Query limitations (TOP, DISTINCT, ALLOWED)
    SDBL_TOP_CLAUSE,    // TOP count clause

    // SELECT Components
    SDBL_SELECT_CLAUSE,   // SELECT clause with fields
    SDBL_FIELD_LIST,      // List of fields in SELECT
    SDBL_SELECTED_FIELD,  // Single field (expression + optional alias)
    SDBL_ALIAS,           // [AS] identifier (CRITICAL for diagnostic)
    SDBL_ASTERISK_FIELD,  // * or Table.*
    SDBL_INTO_CLAUSE,     // INTO/ПОМЕСТИТЬ clause for temporary tables
    SDBL_TEMP_TABLE_NAME, // Temporary table name in INTO clause

    // FROM/WHERE Components
    SDBL_FROM_CLAUSE,  // FROM clause with data sources
    SDBL_DATA_SOURCE,  // Table or subquery in FROM
    SDBL_TABLE_REF,    // Table reference (e.g., Catalog.Products)
    SDBL_WHERE_CLAUSE, // WHERE clause with conditions

    // Expressions
    SDBL_EXPR,                // SDBL expression (general)
    SDBL_LOGICAL_OR_EXPR,     // OR expression
    SDBL_LOGICAL_AND_EXPR,    // AND expression
    SDBL_NOT_EXPR,            // NOT expression
    SDBL_COMPARISON_EXPR,     // Comparison operators (=, <>, <, >, etc.)
    SDBL_IN_EXPR,             // IN predicate (IN (values) or IN (subquery))
    SDBL_IN_HIERARCHY_EXPR,   // IN HIERARCHY predicate (expr IN HIERARCHY(root))
    SDBL_IS_NULL_EXPR,        // IS NULL / IS NOT NULL predicate
    SDBL_BETWEEN_EXPR,        // BETWEEN predicate (expr BETWEEN low AND high)
    SDBL_LIKE_EXPR,           // LIKE predicate (expr LIKE pattern [ESCAPE char])
    SDBL_REFS_EXPR, // REFS predicate (expr REFS MDO) - checks if reference is of specific type
    SDBL_ADDITIVE_EXPR, // Additive operators (+, -)
    SDBL_MULTIPLICATIVE_EXPR, // Multiplicative operators (*, /, MOD)
    SDBL_UNARY_EXPR, // Unary operators (+, -, NOT)
    SDBL_PAREN_EXPR, // Parenthesized expression (single expr)
    SDBL_TUPLE_EXPR, // Tuple expression (expr, expr, ...) for row-wise comparison
    SDBL_SUBQUERY_EXPR, // Subquery in expression context

    // Primary Expressions
    SDBL_COLUMN_REF,    // Column reference
    SDBL_FUNCTION_CALL, // Function call
    SDBL_CASE_EXPR,     // CASE expression (CASE [expr] WHEN ... THEN ... [ELSE ...] END)
    SDBL_WHEN_CLAUSE,   // WHEN clause in CASE expression
    SDBL_LITERAL,       // Literal value (number, string, boolean, null)
    SDBL_MULTI_STRING,  // Multiple string tokens or string with newlines (multiString: STR+)
    SDBL_PARAMETER,     // Parameter reference (&Parameter)
    SDBL_TYPE,          // Type specification in CAST (СТРОКА, ЧИСЛО, etc.)

    // Future (Phase 2+)
    SDBL_JOIN_CLAUSE,   // JOIN clause
    SDBL_GROUP_CLAUSE,  // GROUP BY clause
    SDBL_ORDER_CLAUSE,  // ORDER BY clause
    SDBL_HAVING_CLAUSE, // HAVING clause
    SDBL_FOR_UPDATE,    // FOR UPDATE clause
    SDBL_INDEX_BY,      // INDEX BY clause
    SDBL_AUTOORDER,     // AUTOORDER clause
    SDBL_TOTALS_BY,     // TOTALS BY clause

    // DROP TABLE query
    SDBL_DROP_QUERY, // DROP/УНИЧТОЖИТЬ temp table query

    // Missing argument placeholder
    SDBL_MISSING_ARG, // Intentionally empty VT parameter slot

    // Error recovery
    SDBL_ERROR, // Error node for SDBL

    // Error recovery
    ERROR,

    // Must be last for range checks
    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    /// Returns true if this is a trivia token (whitespace, comment, or BOM).
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE | SyntaxKind::BOM
        )
    }

    /// Returns true if this is a keyword token.
    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            SyntaxKind::KW_PROCEDURE
                | SyntaxKind::KW_END_PROCEDURE
                | SyntaxKind::KW_FUNCTION
                | SyntaxKind::KW_END_FUNCTION
                | SyntaxKind::KW_EXPORT
                | SyntaxKind::KW_VAL
                | SyntaxKind::KW_IF
                | SyntaxKind::KW_THEN
                | SyntaxKind::KW_ELSIF
                | SyntaxKind::KW_ELSE
                | SyntaxKind::KW_END_IF
                | SyntaxKind::KW_FOR
                | SyntaxKind::KW_EACH
                | SyntaxKind::KW_IN
                | SyntaxKind::KW_TO
                | SyntaxKind::KW_WHILE
                | SyntaxKind::KW_DO
                | SyntaxKind::KW_END_DO
                | SyntaxKind::KW_RETURN
                | SyntaxKind::KW_CONTINUE
                | SyntaxKind::KW_BREAK
                | SyntaxKind::KW_GOTO
                | SyntaxKind::KW_TRY
                | SyntaxKind::KW_EXCEPT
                | SyntaxKind::KW_END_TRY
                | SyntaxKind::KW_RAISE
                | SyntaxKind::KW_VAR
                | SyntaxKind::KW_NEW
                | SyntaxKind::KW_EXECUTE
                | SyntaxKind::KW_ADD_HANDLER
                | SyntaxKind::KW_REMOVE_HANDLER
                | SyntaxKind::KW_ASYNC
                | SyntaxKind::KW_AWAIT
                | SyntaxKind::KW_AND
                | SyntaxKind::KW_OR
                | SyntaxKind::KW_NOT
                | SyntaxKind::KW_TRUE
                | SyntaxKind::KW_FALSE
                | SyntaxKind::KW_UNDEFINED
                | SyntaxKind::KW_NULL
        )
    }

    /// Returns true if this is a literal token.
    pub fn is_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::FLOAT
                | SyntaxKind::DECIMAL
                | SyntaxKind::STRING
                | SyntaxKind::STRING_START
                | SyntaxKind::STRING_TAIL
                | SyntaxKind::STRING_PART
                | SyntaxKind::DATE
                | SyntaxKind::KW_TRUE
                | SyntaxKind::KW_FALSE
                | SyntaxKind::KW_UNDEFINED
                | SyntaxKind::KW_NULL
        )
    }

    /// Returns true if this is a string literal token.
    pub fn is_string_literal(self) -> bool {
        matches!(
            self,
            SyntaxKind::STRING
                | SyntaxKind::STRING_START
                | SyntaxKind::STRING_TAIL
                | SyntaxKind::STRING_PART
        )
    }

    /// Returns true if this is a number literal token (including dates).
    pub fn is_number_literal(self) -> bool {
        matches!(self, SyntaxKind::DECIMAL | SyntaxKind::FLOAT | SyntaxKind::DATE)
    }

    /// Returns true if this is a boolean literal token.
    pub fn is_boolean_literal(self) -> bool {
        matches!(self, SyntaxKind::KW_TRUE | SyntaxKind::KW_FALSE)
    }

    /// Returns true if this is a preprocessor directive token.
    pub fn is_preprocessor(self) -> bool {
        matches!(
            self,
            SyntaxKind::PRE_IF
                | SyntaxKind::PRE_ELSIF
                | SyntaxKind::PRE_ELSE
                | SyntaxKind::PRE_END_IF
                | SyntaxKind::PRE_REGION
                | SyntaxKind::PRE_END_REGION
                | SyntaxKind::PRE_USE
        )
    }

    /// Returns true if this is an annotation token.
    pub fn is_annotation(self) -> bool {
        matches!(
            self,
            SyntaxKind::ANN_AT_CLIENT
                | SyntaxKind::ANN_AT_SERVER
                | SyntaxKind::ANN_AT_SERVER_NO_CONTEXT
                | SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT
                | SyntaxKind::ANN_AT_CLIENT_AT_SERVER
                | SyntaxKind::ANN_BEFORE
                | SyntaxKind::ANN_AFTER
                | SyntaxKind::ANN_AROUND
                | SyntaxKind::ANN_CHANGE_AND_VALIDATE
                | SyntaxKind::ANN_CUSTOM
        )
    }

    /// Returns true if this is an operator token.
    pub fn is_operator(self) -> bool {
        matches!(
            self,
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
                | SyntaxKind::GE
        )
    }
}

impl From<u16> for SyntaxKind {
    fn from(value: u16) -> Self {
        assert!(value < SyntaxKind::__LAST as u16, "SyntaxKind value out of range: {}", value);
        // SAFETY: We checked that value is in range
        unsafe { std::mem::transmute(value) }
    }
}

impl From<SyntaxKind> for u16 {
    fn from(kind: SyntaxKind) -> Self {
        kind as u16
    }
}

impl fmt::Display for SyntaxKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let kind = SyntaxKind::KW_PROCEDURE;
        let raw: u16 = kind.into();
        let restored: SyntaxKind = raw.into();
        assert_eq!(kind, restored);
    }

    #[test]
    fn test_is_trivia() {
        assert!(SyntaxKind::WHITESPACE.is_trivia());
        assert!(SyntaxKind::COMMENT.is_trivia());
        assert!(SyntaxKind::NEWLINE.is_trivia());
        assert!(!SyntaxKind::IDENT.is_trivia());
    }

    #[test]
    fn test_is_keyword() {
        assert!(SyntaxKind::KW_PROCEDURE.is_keyword());
        assert!(SyntaxKind::KW_IF.is_keyword());
        assert!(!SyntaxKind::IDENT.is_keyword());
    }

    #[test]
    fn test_is_literal() {
        assert!(SyntaxKind::DECIMAL.is_literal());
        assert!(SyntaxKind::STRING.is_literal());
        assert!(SyntaxKind::KW_TRUE.is_literal());
        assert!(!SyntaxKind::IDENT.is_literal());
    }
}
