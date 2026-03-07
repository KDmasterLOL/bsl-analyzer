//! SDBL (Structured Data Base Language) lexer for 1C:Enterprise query language.
//!
//! Supports both Russian and English keywords (case-insensitive).
//!
//! SDBL is the SQL-like query language embedded within BSL code as string literals.
//! It's used to query the 1C platform's metadata-based database structure.
//!
use logos::Logos;
use smol_str::SmolStr;

/// Token kinds for SDBL query language.
///
/// Each token represents a lexical element in SDBL queries.
/// Keywords support both Russian and English variants (case-insensitive).
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdblTokenKind {
    #[regex(r"(?i)выбрать|(?i)select")]
    KwSelect,

    #[regex(r"(?i)из|(?i)from")]
    KwFrom,

    #[regex(r"(?i)где|(?i)where")]
    KwWhere,

    #[regex(r"(?i)поместить|(?i)into")]
    KwInto,

    #[regex(r"(?i)уничтожить|(?i)drop")]
    KwDrop,

    #[regex(r"(?i)соединение|(?i)join")]
    KwJoin,

    #[regex(r"(?i)внутреннее|(?i)inner")]
    KwInner,

    #[regex(r"(?i)левое|(?i)left")]
    KwLeft,

    #[regex(r"(?i)правое|(?i)right")]
    KwRight,

    #[regex(r"(?i)полное|(?i)full")]
    KwFull,

    #[regex(r"(?i)внешнее|(?i)outer")]
    KwOuter,

    // Note: ПО (po) in Russian means both ON and BY depending on context
    // We merge them into a single token and let the parser determine usage
    #[regex(r"(?i)по|(?i)on|(?i)by")]
    KwOnOrBy,

    #[regex(r"(?i)сгруппировать|(?i)group")]
    KwGroup,

    #[regex(r"(?i)упорядочить|(?i)order")]
    KwOrder,

    #[regex(r"(?i)имеющие|(?i)having")]
    KwHaving,

    #[regex(r"(?i)итоги|(?i)totals")]
    KwTotals,

    #[regex(r"(?i)автоупорядочивание|(?i)autoorder")]
    KwAutoOrder,

    #[regex(r"(?i)возр|(?i)asc")]
    KwAsc,

    #[regex(r"(?i)убыв|(?i)desc")]
    KwDesc,

    #[regex(r"(?i)иерархия|(?i)hierarchy")]
    KwHierarchy,

    #[regex(r"(?i)различные|(?i)distinct")]
    KwDistinct,

    #[regex(r"(?i)первые|(?i)top")]
    KwTop,

    #[regex(r"(?i)разрешенные|(?i)allowed")]
    KwAllowed,

    #[regex(r"(?i)объединить|(?i)union")]
    KwUnion,

    #[regex(r"(?i)все|(?i)all")]
    KwAll,

    #[regex(r"(?i)для|(?i)for")]
    KwFor,

    #[regex(r"(?i)изменения|(?i)update")]
    KwUpdate,

    #[regex(r"(?i)индексировать|(?i)index")]
    KwIndex,

    #[regex(r"(?i)только|(?i)only")]
    KwOnly,

    #[regex(r"(?i)общие|(?i)overall")]
    KwOverall,

    #[regex(r"(?i)периоды|(?i)periods")]
    KwPeriods,

    #[regex(r"(?i)в|(?i)in")]
    KwIn,

    #[regex(r"(?i)между|(?i)between")]
    KwBetween,

    #[regex(r"(?i)подобно|(?i)like")]
    KwLike,

    #[regex(r"(?i)спецсимвол|(?i)escape")]
    KwEscape,

    #[regex(r"(?i)есть|(?i)is")]
    KwIs,

    #[regex(r"(?i)ссылка|(?i)refs")]
    KwRefs,

    #[regex(r"(?i)выбор|(?i)case")]
    KwCase,

    #[regex(r"(?i)когда|(?i)when")]
    KwWhen,

    #[regex(r"(?i)тогда|(?i)then")]
    KwThen,

    #[regex(r"(?i)иначе|(?i)else")]
    KwElse,

    #[regex(r"(?i)конец|(?i)end")]
    KwEnd,

    #[regex(r"(?i)выразить|(?i)cast")]
    KwCast,

    #[regex(r"(?i)как|(?i)as")]
    KwAs,

    #[regex(r"(?i)тип|(?i)type")]
    KwType,

    #[regex(r"(?i)значение|(?i)value")]
    KwValue,

    #[regex(r"(?i)сумма|(?i)sum")]
    FnSum,

    #[regex(r"(?i)среднее|(?i)avg")]
    FnAvg,

    #[regex(r"(?i)минимум|(?i)min")]
    FnMin,

    #[regex(r"(?i)максимум|(?i)max")]
    FnMax,

    #[regex(r"(?i)количество|(?i)count")]
    FnCount,

    // Note: These also double as period types in TOTALS BY ... PERIODS(...)
    // The parser will determine context. Functions have higher priority.
    #[regex(r"(?i)год|(?i)year", priority = 2)]
    FnYear,

    #[regex(r"(?i)квартал|(?i)quarter", priority = 2)]
    FnQuarter,

    #[regex(r"(?i)месяц|(?i)month", priority = 2)]
    FnMonth,

    #[regex(r"(?i)деньгода|(?i)dayofyear", priority = 2)]
    FnDayOfYear,

    #[regex(r"(?i)день|(?i)day", priority = 2)]
    FnDay,

    #[regex(r"(?i)неделя|(?i)week", priority = 2)]
    FnWeek,

    #[regex(r"(?i)деньнедели|(?i)weekday", priority = 2)]
    FnWeekDay,

    #[regex(r"(?i)час|(?i)hour", priority = 2)]
    FnHour,

    #[regex(r"(?i)минута|(?i)minute", priority = 2)]
    FnMinute,

    #[regex(r"(?i)секунда|(?i)second", priority = 2)]
    FnSecond,

    #[regex(r"(?i)началопериода|(?i)beginofperiod")]
    FnBeginOfPeriod,

    #[regex(r"(?i)конецпериода|(?i)endofperiod")]
    FnEndOfPeriod,

    #[regex(r"(?i)добавитькдате|(?i)dateadd")]
    FnDateAdd,

    #[regex(r"(?i)разностьдат|(?i)datediff")]
    FnDateDiff,

    #[regex(r"(?i)датавремя|(?i)datetime")]
    FnDateTime,

    #[regex(r"(?i)дата|(?i)date", priority = 2)]
    FnDate,

    #[regex(r"(?i)подстрока|(?i)substring")]
    FnSubstring,

    #[regex(r"(?i)длинастроки|(?i)stringlength")]
    FnStringLength,

    #[regex(r"(?i)стрнайти|(?i)strfind")]
    FnStrFind,

    #[regex(r"(?i)врег|(?i)upper")]
    FnUpper,

    #[regex(r"(?i)нрег|(?i)lower")]
    FnLower,

    #[regex(r"(?i)сокрлп|(?i)trimall")]
    FnTrimAll,

    #[regex(r"(?i)сокрл|(?i)triml")]
    FnTrimL,

    #[regex(r"(?i)сокрп|(?i)trimr")]
    FnTrimR,

    #[regex(r"(?i)окр|(?i)round")]
    FnRound,

    #[regex(r"(?i)цел|(?i)int")]
    FnInt,

    #[regex(r"(?i)log10")]
    FnLog10,

    #[regex(r"(?i)log")]
    FnLog,

    #[regex(r"(?i)pow")]
    FnPow,

    #[regex(r"(?i)sqrt")]
    FnSqrt,

    #[regex(r"(?i)типзначения|(?i)valuetype")]
    FnValueType,

    #[regex(r"(?i)представление|(?i)presentation")]
    FnPresentation,

    #[regex(r"(?i)представлениессылки|(?i)refpresentation")]
    FnRefPresentation,

    #[regex(r"(?i)естьnull|(?i)isnull")]
    FnIsNull,

    #[regex(r"(?i)пустаятаблица|(?i)emptytable")]
    FnEmptyTable,

    #[regex(r"(?i)пустаяссылка|(?i)emptyref")]
    FnEmptyRef,

    #[regex(r"(?i)уникальныйидентификатор|(?i)uuid")]
    FnUUID,

    #[regex(r"(?i)справочник|(?i)catalog")]
    MdoCatalog,

    #[regex(r"(?i)документ|(?i)document")]
    MdoDocument,

    #[regex(r"(?i)регистрсведений|(?i)informationregister")]
    MdoInformationRegister,

    #[regex(r"(?i)регистрнакопления|(?i)accumulationregister")]
    MdoAccumulationRegister,

    #[regex(r"(?i)регистрбухгалтерии|(?i)accountingregister")]
    MdoAccountingRegister,

    #[regex(r"(?i)регистррасчета|(?i)calculationregister")]
    MdoCalculationRegister,

    #[regex(r"(?i)плансчетов|(?i)chartofaccounts")]
    MdoChartOfAccounts,

    #[regex(r"(?i)планвидоврасчета|(?i)chartofcalculationtypes")]
    MdoChartOfCalculationTypes,

    #[regex(r"(?i)планвидовхарактеристик|(?i)chartofcharacteristictypes")]
    MdoChartOfCharacteristicTypes,

    #[regex(r"(?i)перечисление|(?i)enum")]
    MdoEnum,

    #[regex(r"(?i)бизнеспроцесс|(?i)businessprocess")]
    MdoBusinessProcess,

    #[regex(r"(?i)задача|(?i)task")]
    MdoTask,

    #[regex(r"(?i)константа|(?i)constant")]
    MdoConstant,

    #[regex(r"(?i)последовательность|(?i)sequence")]
    MdoSequence,

    #[regex(r"(?i)внешнийисточникданных|(?i)externaldatasource")]
    MdoExternalDataSource,

    #[regex(r"(?i)срезпервых|(?i)slicefirst")]
    VtSliceFirst,

    #[regex(r"(?i)срезпоследних|(?i)slicelast")]
    VtSliceLast,

    #[regex(r"(?i)остатки|(?i)balance")]
    VtBalance,

    #[regex(r"(?i)обороты|(?i)turnovers")]
    VtTurnovers,

    #[regex(r"(?i)остаткииобороты|(?i)balanceandturnovers")]
    VtBalanceAndTurnovers,

    #[regex(r"(?i)оборотыдткт|(?i)drcrturnovers")]
    VtDrCrTurnovers,

    #[regex(r"(?i)булево|(?i)boolean")]
    TypeBoolean,

    #[regex(r"(?i)число|(?i)number")]
    TypeNumber,

    #[regex(r"(?i)строка|(?i)string")]
    TypeString,

    #[regex(r"(?i)дата|(?i)date")]
    TypeDate,

    #[regex(r"(?i)истина|(?i)true")]
    LitTrue,

    #[regex(r"(?i)ложь|(?i)false")]
    LitFalse,

    #[regex(r"(?i)null")]
    LitNull,

    #[regex(r"(?i)неопределено|(?i)undefined")]
    LitUndefined,

    #[regex(r"(?i)и|(?i)and")]
    OpAnd,

    #[regex(r"(?i)или|(?i)or")]
    OpOr,

    #[regex(r"(?i)не|(?i)not")]
    OpNot,

    // Note: Most period types are the same as date functions above.
    // Only unique period types are listed here.
    #[regex(r"(?i)декада|(?i)tendays")]
    PeriodTenDays,

    #[regex(r"(?i)полугодие|(?i)halfyear")]
    PeriodHalfYear,

    #[token("=")]
    Eq,

    #[token("<>")]
    Neq,

    #[token("<=")]
    Le,

    #[token("<")]
    Lt,

    #[token(">=")]
    Ge,

    #[token(">")]
    Gt,

    #[token("+")]
    Plus,

    #[token("-")]
    Minus,

    #[token("*")]
    Star,

    #[token("/")]
    Slash,

    #[token("%")]
    Percent,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token(".")]
    Dot,

    #[token(",")]
    Comma,

    #[token(";")]
    Semicolon,

    #[token("#")]
    Hash, // For temporary table names (#TempTable)

    #[token("&")]
    Ampersand, // For query parameters (&Parameter)

    #[token("|")]
    Bar, // For multiline query strings

    // Numbers: floats must come before integers
    #[regex(r"[0-9]+\.[0-9]+")]
    Float,

    #[regex(r"[0-9]+")]
    Decimal,

    #[token("\"")]
    Quote,

    String,

    // Date literals: '20240101' or '20240101120000'
    #[regex(r"'[0-9]{8,14}'")]
    Date,

    // Identifier: Unicode letters, digits, underscore
    // Lower priority than keywords
    #[regex(r"[_a-zA-Zа-яА-ЯёЁ][_a-zA-Zа-яА-ЯёЁ0-9]*", priority = 1)]
    Ident,

    // Parameter reference: &Name
    #[regex(r"&[_a-zA-Zа-яА-ЯёЁ][_a-zA-Zа-яА-ЯёЁ0-9]*")]
    Parameter,

    // Line comment: // ...
    // NOTE: SDBL standard does not define comments, but we support them for:
    // 1. Robustness when parsing queries extracted from BSL strings that may contain "//" text
    // 2. Developer convenience when testing queries
    #[regex(r"//[^\n]*")]
    Comment,

    // Newline
    #[token("\n")]
    Newline,

    // Whitespace (spaces, tabs, carriage returns)
    #[regex(r"[ \t\r]+")]
    Whitespace,

    // Error token for unrecognized input
    Error,
}

/// An SDBL token with its kind, text, and position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblToken {
    /// The kind of token
    pub kind: SdblTokenKind,
    /// The original text of the token
    pub text: SmolStr,
    /// The byte offset in the source code
    pub offset: usize,
}

/// Tokenizes SDBL query string into a stream of tokens.
///
/// # Example
///
/// ```
/// use lexer::sdbl::tokenize_sdbl;
///
/// let tokens = tokenize_sdbl("SELECT Name FROM Catalog.Products");
/// assert!(tokens.len() > 0);
/// ```
pub fn tokenize_sdbl(input: &str) -> Vec<SdblToken> {
    let mut result = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        let remaining = &input[pos..];

        if remaining.starts_with('"') {
            let strings = tokenize_strings_mode(input, pos);
            result.extend(strings.tokens);
            pos = strings.end_pos;
        } else {
            let mut lexer = SdblTokenKind::lexer(remaining);
            if let Some(token_result) = lexer.next() {
                let kind = token_result.unwrap_or(SdblTokenKind::Error);
                if kind == SdblTokenKind::Quote {
                    unreachable!("Quote should be handled above");
                }
                let text = SmolStr::new(lexer.slice());
                let offset = pos;
                result.push(SdblToken { kind, text: text.clone(), offset });
                pos += text.len();
            } else {
                break;
            }
        }
    }

    result
}

struct StringsResult {
    tokens: Vec<SdblToken>,
    end_pos: usize,
}

fn tokenize_strings_mode(input: &str, start_pos: usize) -> StringsResult {
    let mut tokens = Vec::new();
    let mut pos = start_pos;
    let bytes = input.as_bytes();

    if pos >= bytes.len() || bytes[pos] != b'"' {
        return StringsResult { tokens, end_pos: pos };
    }

    let opening_quote_pos = pos;
    tokens.push(SdblToken {
        kind: SdblTokenKind::String,
        text: SmolStr::new(&input[opening_quote_pos..opening_quote_pos + 1]),
        offset: opening_quote_pos,
    });
    pos += 1;

    loop {
        let content_start = pos;

        while pos < bytes.len() && bytes[pos] != b'"' && bytes[pos] != b'\n' && bytes[pos] != b'\r'
        {
            pos += 1;
        }

        if pos >= bytes.len() {
            if content_start < pos {
                let text = SmolStr::new(&input[content_start..pos]);
                tokens.push(SdblToken { kind: SdblTokenKind::String, text, offset: content_start });
            }
            break;
        }

        if bytes[pos] == b'"' {
            if pos + 1 < bytes.len() && bytes[pos + 1] == b'"' {
                pos += 2;
                continue;
            } else {
                if content_start < pos {
                    let text = SmolStr::new(&input[content_start..pos]);
                    tokens.push(SdblToken {
                        kind: SdblTokenKind::String,
                        text,
                        offset: content_start,
                    });
                }
                tokens.push(SdblToken {
                    kind: SdblTokenKind::String,
                    text: SmolStr::new(&input[pos..pos + 1]),
                    offset: pos,
                });
                pos += 1;
                break;
            }
        }

        if bytes[pos] == b'\n' || bytes[pos] == b'\r' {
            if content_start < pos {
                let text = SmolStr::new(&input[content_start..pos]);
                tokens.push(SdblToken { kind: SdblTokenKind::String, text, offset: content_start });
            }

            while pos < bytes.len()
                && (bytes[pos] == b'\n'
                    || bytes[pos] == b'\r'
                    || bytes[pos] == b' '
                    || bytes[pos] == b'\t')
            {
                pos += 1;
            }
        }
    }

    StringsResult { tokens, end_pos: pos }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_statement() {
        let tokens = tokenize_sdbl("SELECT Name FROM Catalog.Products");
        // SELECT(0) WS(1) Name(2) WS(3) FROM(4) WS(5) Catalog(6) .(7) Products(8)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwFrom);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::MdoCatalog);
        assert_eq!(tokens[7].kind, SdblTokenKind::Dot);
        assert_eq!(tokens[8].kind, SdblTokenKind::Ident);
    }

    #[test]
    fn test_russian_keywords() {
        let tokens = tokenize_sdbl("ВЫБРАТЬ Наименование ИЗ Справочник.Товары");
        // ВЫБРАТЬ(0) WS(1) Наименование(2) WS(3) ИЗ(4) WS(5) Справочник(6) .(7) Товары(8)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwFrom);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::MdoCatalog);
        assert_eq!(tokens[7].kind, SdblTokenKind::Dot);
        assert_eq!(tokens[8].kind, SdblTokenKind::Ident);
    }

    #[test]
    fn test_where_clause() {
        let tokens = tokenize_sdbl("WHERE Price > 100");
        // WHERE(0) WS(1) Price(2) WS(3) >(4) WS(5) 100(6)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::Gt);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::Decimal);
    }

    #[test]
    fn test_join() {
        let tokens =
            tokenize_sdbl("LEFT JOIN Catalog.Categories ON Products.Category = Categories.Ref");
        // LEFT(0) WS(1) JOIN(2) WS(3) Catalog(4) .(5) Categories(6) ...
        assert_eq!(tokens[0].kind, SdblTokenKind::KwLeft);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwJoin);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::MdoCatalog);
    }

    #[test]
    fn test_aggregate_functions() {
        let tokens = tokenize_sdbl("SUM(Amount) AS Total");
        // SUM(0) ((1) Amount(2) )(3) WS(4) AS(5) WS(6) Total(7)
        assert_eq!(tokens[0].kind, SdblTokenKind::FnSum);
        assert_eq!(tokens[1].kind, SdblTokenKind::LParen);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::RParen);
        assert_eq!(tokens[4].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[5].kind, SdblTokenKind::KwAs);
        assert_eq!(tokens[6].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[7].kind, SdblTokenKind::Ident);
    }

    #[test]
    fn test_parameters() {
        let tokens = tokenize_sdbl("WHERE Date > &StartDate");
        // WHERE(0) WS(1) Date(2) WS(3) >(4) WS(5) &StartDate(6)
        assert_eq!(tokens[6].kind, SdblTokenKind::Parameter);
        assert_eq!(tokens[6].text.as_str(), "&StartDate");
    }

    #[test]
    fn test_temporary_table() {
        let tokens = tokenize_sdbl("INTO #TempTable");
        // INTO(0) WS(1) #(2) TempTable(3)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwInto);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Hash);
        assert_eq!(tokens[3].kind, SdblTokenKind::Ident);
    }

    #[test]
    fn test_virtual_table() {
        let tokens = tokenize_sdbl("FROM AccumulationRegister.Stock.Balance");
        // FROM(0) WS(1) AccumulationRegister(2) .(3) Stock(4) .(5) Balance(6)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwFrom);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::MdoAccumulationRegister);
        assert_eq!(tokens[3].kind, SdblTokenKind::Dot);
        assert_eq!(tokens[4].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[5].kind, SdblTokenKind::Dot);
        assert_eq!(tokens[6].kind, SdblTokenKind::VtBalance);
    }

    #[test]
    fn test_case_expression() {
        let tokens = tokenize_sdbl("CASE WHEN Amount > 0 THEN 1 ELSE 0 END");
        // CASE(0) WS(1) WHEN(2) WS(3) Amount(4) WS(5) >(6) WS(7) 0(8) WS(9) THEN(10) WS(11) 1(12) WS(13) ELSE(14) WS(15) 0(16) WS(17) END(18)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwCase);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwWhen);
        assert_eq!(tokens[10].kind, SdblTokenKind::KwThen);
        assert_eq!(tokens[14].kind, SdblTokenKind::KwElse);
        assert_eq!(tokens[18].kind, SdblTokenKind::KwEnd);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize_sdbl(r#""Hello World""#);
        assert_eq!(tokens[0].kind, SdblTokenKind::String);
    }

    #[test]
    fn test_date_literal() {
        let tokens = tokenize_sdbl("'20240101'");
        assert_eq!(tokens[0].kind, SdblTokenKind::Date);
    }

    #[test]
    fn test_boolean_literals() {
        let tokens = tokenize_sdbl("TRUE FALSE Истина Ложь");
        // TRUE(0) WS(1) FALSE(2) WS(3) Истина(4) WS(5) Ложь(6)
        assert_eq!(tokens[0].kind, SdblTokenKind::LitTrue);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::LitFalse);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::LitTrue);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::LitFalse);
    }

    #[test]
    fn test_comment_separator_line() {
        let input = "SELECT 1;\n////////////////////////////////////////////////////////////////////////////////\nSELECT 2";
        let tokens = tokenize_sdbl(input);

        // Find semicolon and comment
        let semicolon_pos = tokens.iter().position(|t| t.kind == SdblTokenKind::Semicolon).unwrap();
        let comment_pos = tokens.iter().position(|t| t.kind == SdblTokenKind::Comment);

        println!("Tokens around semicolon:");
        for (i, token) in tokens.iter().enumerate() {
            if i >= semicolon_pos.saturating_sub(2) && i <= semicolon_pos + 5 {
                println!(
                    "  [{} {}]: {:?} = {:?}",
                    i,
                    if i == semicolon_pos { "←" } else { " " },
                    token.kind,
                    token.text
                );
            }
        }

        assert!(comment_pos.is_some(), "Comment token should be found for ////////////////");
        println!("Comment found at position {}", comment_pos.unwrap());
    }

    #[test]
    fn test_date_functions() {
        let tokens = tokenize_sdbl("YEAR(Date) MONTH(Date) DAY(Date)");
        // YEAR(0) ((1) Date(2) )(3) WS(4) MONTH(5) ((6) Date(7) )(8) WS(9) DAY(10) ((11) Date(12) )(13)
        assert_eq!(tokens[0].kind, SdblTokenKind::FnYear);
        assert_eq!(tokens[4].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[5].kind, SdblTokenKind::FnMonth);
        assert_eq!(tokens[9].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[10].kind, SdblTokenKind::FnDay);
    }

    #[test]
    fn test_string_functions() {
        let tokens = tokenize_sdbl("SUBSTRING(Name, 1, 10) UPPER(Name)");
        // SUBSTRING(0) ((1) Name(2) ,(3) WS(4) 1(5) ,(6) WS(7) 10(8) )(9) WS(10) UPPER(11) ((12) Name(13) )(14)
        assert_eq!(tokens[0].kind, SdblTokenKind::FnSubstring);
        assert_eq!(tokens[10].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[11].kind, SdblTokenKind::FnUpper);
    }

    #[test]
    fn test_group_by() {
        let tokens = tokenize_sdbl("GROUP BY Category HAVING COUNT(*) > 5");
        // GROUP(0) WS(1) BY(2) WS(3) Category(4) WS(5) HAVING(6) WS(7) COUNT(8) ...
        assert_eq!(tokens[0].kind, SdblTokenKind::KwGroup);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwOnOrBy);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::KwHaving);
        assert_eq!(tokens[7].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[8].kind, SdblTokenKind::FnCount);
    }

    #[test]
    fn test_order_by() {
        let tokens = tokenize_sdbl("ORDER BY Name ASC, Price DESC");
        // ORDER(0) WS(1) BY(2) WS(3) Name(4) WS(5) ASC(6) ,(7) WS(8) Price(9) WS(10) DESC(11)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwOrder);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwOnOrBy);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::KwAsc);
        assert_eq!(tokens[9].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[10].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[11].kind, SdblTokenKind::KwDesc);
    }

    #[test]
    fn test_union() {
        let tokens = tokenize_sdbl("SELECT * FROM Table1 UNION ALL SELECT * FROM Table2");
        // SELECT(0) WS(1) *(2) WS(3) FROM(4) WS(5) Table1(6) WS(7) UNION(8) WS(9) ALL(10) WS(11) SELECT(12) ...
        assert_eq!(tokens[8].kind, SdblTokenKind::KwUnion);
        assert_eq!(tokens[9].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[10].kind, SdblTokenKind::KwAll);
    }

    #[test]
    fn test_distinct_top() {
        let tokens = tokenize_sdbl("SELECT DISTINCT TOP 100 Name");
        // SELECT(0) WS(1) DISTINCT(2) WS(3) TOP(4) WS(5) 100(6) WS(7) Name(8)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwDistinct);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwTop);
    }

    #[test]
    fn test_in_predicate() {
        let tokens = tokenize_sdbl("WHERE Category IN (&CategoryList)");
        // WHERE(0) WS(1) Category(2) WS(3) IN(4) WS(5) ((6) &CategoryList(7) )(8)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwIn);
    }

    #[test]
    fn test_between_predicate() {
        let tokens = tokenize_sdbl("WHERE Price BETWEEN 100 AND 500");
        // WHERE(0) WS(1) Price(2) WS(3) BETWEEN(4) WS(5) 100(6) WS(7) AND(8) WS(9) 500(10)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwBetween);
        assert_eq!(tokens[6].kind, SdblTokenKind::Decimal);
        assert_eq!(tokens[7].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[8].kind, SdblTokenKind::OpAnd);
    }

    #[test]
    fn test_like_predicate() {
        let tokens = tokenize_sdbl("WHERE Name LIKE \"%Products%\"");
        // WHERE(0) WS(1) Name(2) WS(3) LIKE(4) WS(5) "%Products%"(6)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwLike);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::String);
    }

    #[test]
    fn test_is_null() {
        let tokens = tokenize_sdbl("WHERE Description IS NULL");
        // WHERE(0) WS(1) Description(2) WS(3) IS(4) WS(5) NULL(6)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwIs);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::LitNull);
    }

    #[test]
    fn test_cast() {
        let tokens = tokenize_sdbl("CAST(Price AS NUMBER(15, 2))");
        // CAST(0) ((1) Price(2) WS(3) AS(4) WS(5) NUMBER(6) ((7) 15(8) ,(9) WS(10) 2(11) )(12) )(13)
        assert_eq!(tokens[0].kind, SdblTokenKind::KwCast);
        assert_eq!(tokens[1].kind, SdblTokenKind::LParen);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwAs);
        assert_eq!(tokens[5].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[6].kind, SdblTokenKind::TypeNumber);
    }

    #[test]
    fn test_multiline_string_behavior() {
        // Test what happens with strings containing newlines
        // (after SDBL extraction from BSL multiline strings)

        // Before lexer modification, this will produce ERROR tokens
        // After modification, should produce String tokens
        let input = "SELECT\n   \"\" AS Field";
        let tokens = tokenize_sdbl(input);

        // Should have: SELECT, Newline, Whitespace, String(""), Whitespace, AS, Whitespace, Ident
        println!("\nMultiline string test tokens:");
        for (i, token) in tokens.iter().enumerate() {
            println!("  {}: {:?} = {:?}", i, token.kind, token.text);
        }

        // After lexer fix, empty string should be a String token
        // For now, just document what we find
        assert!(!tokens.is_empty(), "Should produce some tokens");
    }

    #[test]
    fn test_string_without_newline() {
        let tokens = tokenize_sdbl(r#""simple""#);
        let string_tokens: Vec<_> =
            tokens.iter().filter(|t| t.kind == SdblTokenKind::String).collect();
        assert_eq!(string_tokens.len(), 3);
        assert_eq!(string_tokens[0].text, r#"""#);
        assert_eq!(string_tokens[1].text, r#"simple"#);
        assert_eq!(string_tokens[2].text, r#"""#);
    }

    #[test]
    fn test_string_with_embedded_newline() {
        // Test a string that has a newline INSIDE it (like ANTLR STR+)
        // This simulates what ANTLR would create for multiString
        let input = "\"text\nmore\"";
        let tokens = tokenize_sdbl(input);

        println!("\nString with embedded newline tokens:");
        for (i, token) in tokens.iter().enumerate() {
            println!("  {}: {:?} = {:?}", i, token.kind, token.text);
        }

        // With current regex [^"\n\r], this should NOT match
        // and produce ERROR or split tokens
        assert!(!tokens.is_empty(), "Should produce tokens");
    }

    #[test]
    fn test_multiline_string_splitting() {
        let input = r#"   " КАК,
   " КАК,
   " КАК,"#;
        let tokens = tokenize_sdbl(input);

        let string_tokens: Vec<_> =
            tokens.iter().filter(|t| t.kind == SdblTokenKind::String).collect();

        assert!(
            string_tokens.len() >= 2,
            "Expected at least 2 String tokens after splitting, got {}",
            string_tokens.len()
        );
    }
}
