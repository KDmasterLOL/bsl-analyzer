//! Convert SDBL tokens to parser-compatible Token format.
//!
//! The lexer produces `SdblToken` with `SdblTokenKind`, but the parser
//! expects `Token` with `TokenKind`. This module provides the conversion.

use lexer::sdbl::{SdblToken, SdblTokenKind};
use lexer::{Token, TokenKind};

/// Convert SDBL token to parser Token.
///
/// Maps SDBL-specific token kinds to appropriate BSL TokenKind variants.
/// Where SDBL has unique tokens (e.g., SDBL keywords), we map them to
/// generic Ident tokens and rely on contextual parsing.
pub fn convert_sdbl_token(sdbl_token: &SdblToken) -> Token {
    Token {
        kind: convert_sdbl_token_kind(sdbl_token.kind),
        text: sdbl_token.text.clone(),
        offset: sdbl_token.offset,
    }
}

/// Convert all SDBL tokens to parser tokens.
pub fn convert_sdbl_tokens(sdbl_tokens: &[SdblToken]) -> Vec<Token> {
    sdbl_tokens.iter().map(convert_sdbl_token).collect()
}

/// Map SdblTokenKind to TokenKind.
///
/// Many SDBL tokens map directly to BSL equivalents.
/// SDBL-specific keywords are mapped to Ident since we don't have
/// dedicated BSL TokenKind variants for them.
fn convert_sdbl_token_kind(kind: SdblTokenKind) -> TokenKind {
    use SdblTokenKind as S;
    use TokenKind as T;

    match kind {
        // SDBL clause keywords - mapped to Ident for at_sdbl_keyword() matching
        // at_sdbl_keyword() checks for Ident + case-insensitive text match
        S::KwSelect => T::Ident,
        S::KwFrom => T::Ident,
        S::KwWhere => T::Ident,
        S::KwAs => T::Ident,
        S::KwUnion => T::Ident,
        S::KwAll => T::Ident,
        S::KwDistinct => T::Ident,
        S::KwTop => T::Ident,

        // Logical operators - use BSL keyword tokens for type-safe parsing
        // These are used in expressions and should be checked with p.at(TokenKind::KwAnd)
        S::OpAnd => T::KwAnd,
        S::OpOr => T::KwOr,
        S::OpNot => T::KwNot,

        // Predicates - use BSL keyword tokens
        S::KwIn => T::KwIn,
        S::KwIs => T::Ident,
        S::LitNull => T::Ident, // Was T::KwNull - FIXED (treated as keyword in SDBL)
        S::KwBetween => T::Ident,
        S::KwLike => T::Ident,
        S::KwEscape => T::Ident,
        S::KwCase => T::Ident,
        S::KwWhen => T::Ident,
        S::KwThen => T::Ident, // Was T::KwThen - FIXED
        S::KwElse => T::Ident, // Was T::KwElse - FIXED
        S::KwEnd => T::Ident,
        S::KwInto => T::Ident,
        S::KwGroup => T::Ident,
        S::KwOnOrBy => T::Ident, // BY/ON/ПО merged token
        S::KwHaving => T::Ident,
        S::KwOrder => T::Ident,
        S::KwAsc => T::Ident,
        S::KwDesc => T::Ident,
        S::KwFor => T::Ident, // Was T::KwFor - FIXED
        S::KwUpdate => T::Ident,
        S::KwIndex => T::Ident,
        S::KwJoin => T::Ident,
        S::KwInner => T::Ident,
        S::KwLeft => T::Ident,
        S::KwRight => T::Ident,
        S::KwFull => T::Ident,
        S::KwOuter => T::Ident,
        S::KwCast | S::KwValue | S::KwType => T::Ident,
        S::KwAllowed => T::Ident,
        S::KwAutoOrder => T::Ident,
        S::KwOnly => T::Ident,
        S::KwTotals => T::Ident,
        S::KwOverall => T::Ident,
        S::KwPeriods => T::Ident,
        S::KwRefs => T::Ident,
        S::KwHierarchy => T::Ident,
        S::KwDrop => T::Ident,

        // Aggregate functions
        S::FnSum | S::FnCount | S::FnAvg | S::FnMin | S::FnMax => T::Ident,

        // Date/time functions (only map ones that exist)
        S::FnYear
        | S::FnQuarter
        | S::FnMonth
        | S::FnDayOfYear
        | S::FnDay
        | S::FnWeek
        | S::FnWeekDay
        | S::FnHour
        | S::FnMinute
        | S::FnSecond
        | S::FnBeginOfPeriod
        | S::FnEndOfPeriod
        | S::FnDateAdd
        | S::FnDateDiff
        | S::FnDateTime => T::Ident,

        // String functions
        S::FnSubstring => T::Ident,

        // Type conversion/casting
        S::FnPresentation | S::FnValueType | S::FnDate => T::Ident,

        // Other functions
        S::FnIsNull | S::FnRefPresentation | S::FnEmptyRef | S::FnEmptyTable | S::FnUUID => {
            T::Ident
        }

        // Type literals
        S::TypeBoolean | S::TypeNumber | S::TypeString | S::TypeDate => T::Ident,

        // Metadata objects
        S::MdoCatalog
        | S::MdoDocument
        | S::MdoEnum
        | S::MdoChartOfCharacteristicTypes
        | S::MdoChartOfAccounts
        | S::MdoChartOfCalculationTypes
        | S::MdoInformationRegister
        | S::MdoAccumulationRegister
        | S::MdoAccountingRegister
        | S::MdoCalculationRegister
        | S::MdoBusinessProcess
        | S::MdoTask
        | S::MdoExternalDataSource
        | S::MdoConstant
        | S::MdoSequence => T::Ident,

        // Math/string functions
        S::FnStringLength
        | S::FnStrFind
        | S::FnUpper
        | S::FnLower
        | S::FnTrimAll
        | S::FnTrimL
        | S::FnTrimR
        | S::FnRound
        | S::FnInt
        | S::FnLog10
        | S::FnLog
        | S::FnPow
        | S::FnSqrt => T::Ident,

        // Virtual table suffixes
        S::VtSliceFirst
        | S::VtSliceLast
        | S::VtBalance
        | S::VtTurnovers
        | S::VtBalanceAndTurnovers
        | S::VtDrCrTurnovers => T::Ident,

        // Period types
        S::PeriodTenDays | S::PeriodHalfYear => T::Ident,

        // Operators
        S::Eq => T::Eq,
        S::Neq => T::Neq,
        S::Lt => T::Lt,
        S::Le => T::Le,
        S::Gt => T::Gt,
        S::Ge => T::Ge,
        S::Plus => T::Plus,
        S::Minus => T::Minus,
        S::Star => T::Star,
        S::Slash => T::Slash,
        S::Percent => T::Percent,

        // Punctuation
        S::LParen => T::LParen,
        S::RParen => T::RParen,
        S::Dot => T::Dot,
        S::Comma => T::Comma,
        S::Semicolon => T::Semicolon,
        S::Ampersand => T::Ampersand,
        S::Hash => T::Hash, // For temporary tables (#TempTable)
        S::Bar => T::Bar,   // For multiline query strings

        // Literals
        S::LitTrue => T::KwTrue,
        S::LitFalse => T::KwFalse,
        S::LitUndefined => T::KwUndefined,
        S::Decimal => T::Decimal,
        S::Float => T::Float,
        S::String => T::String,
        S::Date => T::Date,

        // Identifiers
        S::Ident => T::Ident,

        // Parameters (&Parameter)
        S::Parameter => T::Ampersand, // Will be converted to parameter syntax later

        // Trivia (all treated as trivia and skipped by parser)
        S::Newline => T::Newline,
        S::Comment => T::Comment,
        S::Whitespace => T::Whitespace,

        S::Quote => T::Error,
        S::Error => T::Error,
    }
}
