use lexer::sdbl::{SdblToken, SdblTokenKind};
use lexer::{Token, TokenKind};

pub fn convert_sdbl_token(sdbl_token: &SdblToken) -> Token {
    Token {
        kind: convert_sdbl_token_kind(sdbl_token.kind),
        text: sdbl_token.text.clone(),
        offset: sdbl_token.offset,
    }
}

pub fn convert_sdbl_tokens(sdbl_tokens: &[SdblToken]) -> Vec<Token> {
    sdbl_tokens.iter().map(convert_sdbl_token).collect()
}

fn convert_sdbl_token_kind(kind: SdblTokenKind) -> TokenKind {
    use SdblTokenKind as S;
    use TokenKind as T;

    match kind {
        S::KwSelect => T::Ident,
        S::KwFrom => T::Ident,
        S::KwWhere => T::Ident,
        S::KwAs => T::Ident,
        S::KwUnion => T::Ident,
        S::KwAll => T::Ident,
        S::KwDistinct => T::Ident,
        S::KwTop => T::Ident,

        S::OpAnd => T::KwAnd,
        S::OpOr => T::KwOr,
        S::OpNot => T::KwNot,

        S::KwIn => T::KwIn,
        S::KwIs => T::Ident,
        S::LitNull => T::Ident,
        S::KwBetween => T::Ident,
        S::KwLike => T::Ident,
        S::KwEscape => T::Ident,
        S::KwCase => T::Ident,
        S::KwWhen => T::Ident,
        S::KwThen => T::Ident,
        S::KwElse => T::Ident,
        S::KwEnd => T::Ident,
        S::KwInto => T::Ident,
        S::KwGroup => T::Ident,
        S::KwOnOrBy => T::Ident,
        S::KwHaving => T::Ident,
        S::KwOrder => T::Ident,
        S::KwAsc => T::Ident,
        S::KwDesc => T::Ident,
        S::KwFor => T::Ident,
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

        S::FnSum | S::FnCount | S::FnAvg | S::FnMin | S::FnMax => T::Ident,

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

        S::FnSubstring => T::Ident,

        S::FnPresentation | S::FnValueType => T::Ident,

        S::FnIsNull | S::FnRefPresentation | S::FnEmptyRef | S::FnEmptyTable | S::FnUUID => {
            T::Ident
        }

        S::FnRecordAutoNumber | S::FnGroupedBy | S::FnStoredDataSize => T::Ident,

        S::TypeBoolean | S::TypeNumber | S::TypeString | S::TypeDate => T::Ident,

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
        | S::MdoConstants
        | S::MdoSequence
        | S::MdoDocumentJournal
        | S::MdoExchangePlan
        | S::MdoFilterCriterion => T::Ident,

        S::FnStringLength
        | S::FnStrFind
        | S::FnStrReplace
        | S::FnUpper
        | S::FnLower
        | S::FnTrimAll
        | S::FnTrimL
        | S::FnTrimR
        | S::FnLeft
        | S::FnRight
        | S::FnRound
        | S::FnInt
        | S::FnExp
        | S::FnLog10
        | S::FnLog
        | S::FnPow
        | S::FnSqrt
        | S::FnACos
        | S::FnASin
        | S::FnATan
        | S::FnCos
        | S::FnSin
        | S::FnTan => T::Ident,

        S::VtSliceFirst
        | S::VtSliceLast
        | S::VtBalance
        | S::VtTurnovers
        | S::VtBalanceAndTurnovers
        | S::VtDrCrTurnovers => T::Ident,

        S::PeriodTenDays | S::PeriodHalfYear => T::Ident,

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

        S::LParen => T::LParen,
        S::RParen => T::RParen,
        S::LBrace => T::LBrace,
        S::RBrace => T::RBrace,
        S::Dot => T::Dot,
        S::Comma => T::Comma,
        S::Semicolon => T::Semicolon,
        S::Ampersand => T::Ampersand,
        S::Hash => T::Hash,
        S::Bar => T::Bar,

        S::LitTrue => T::KwTrue,
        S::LitFalse => T::KwFalse,
        S::LitUndefined => T::KwUndefined,
        S::Decimal => T::Decimal,
        S::Float => T::Float,
        S::String => T::String,
        S::Date => T::Date,

        S::Ident => T::Ident,

        S::Parameter => T::Ampersand,

        S::Newline => T::Newline,
        S::Comment => T::Comment,
        S::Whitespace => T::Whitespace,

        S::Quote => T::Error,
        S::Error => T::Error,
    }
}
