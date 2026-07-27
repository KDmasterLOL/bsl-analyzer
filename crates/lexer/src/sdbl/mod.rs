//! SDBL (1C:Enterprise query language) lexer.
//!
//! ## Provenance
//!
//! The token vocabulary is being re-derived from official 1C sources one
//! vocabulary at a time. Each re-derived group carries a `CLEAN-ROOM`
//! banner naming the attestation document that records its sources;
//! everything still awaiting re-derivation carries a `LEGACY` banner
//! naming the slice that owns it.
//!
//! - Slice 3b — metadata-object table vocabulary and the `Error`
//!   fallback: `docs/legal/sdbl-clean-room-slice3b.md`.
//!
//! The lexer core, the structural keyword vocabulary, the clause-keyword
//! leftovers and the primitive-type vocabulary are attested by
//! `docs/legal/sdbl-clean-room-slice{1,2,2-addendum,3a}.md`. Their
//! in-file banners were removed in a repository-wide comment prune and
//! are not restored here, so absence of a banner above a declaration
//! does not imply absence of an attestation — the attestation documents
//! are authoritative on scope.

mod strings_mode;

use logos::Logos;
use smol_str::SmolStr;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdblTokenKind {
    #[regex(r"[ \t\r]+")]
    Whitespace,

    #[token("\n")]
    Newline,

    #[regex(r"//[^\n]*")]
    Comment,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    // LEGACY (unowned) — the brace pair postdates every closed lexer
    // attestation and no pending slice claims it. See
    // `docs/legal/sdbl-clean-room-slice3b.md` § Unowned brace tokens.
    #[token("{")]
    LBrace,

    #[token("}")]
    RBrace,

    #[token(".")]
    Dot,

    #[token(",")]
    Comma,

    #[token(";")]
    Semicolon,

    #[token("#")]
    Hash,

    #[token("&")]
    Ampersand,

    #[token("|")]
    Bar,

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

    #[regex(r"[0-9]+\.[0-9]+")]
    Float,

    #[regex(r"[0-9]+")]
    Decimal,

    #[token("\"")]
    Quote,

    String,

    #[regex(r"'[0-9]{8,14}'")]
    Date,

    #[regex(r"[_\p{L}][_\p{L}0-9]*", priority = 1)]
    Ident,

    #[regex(r"&[_\p{L}][_\p{L}0-9]*")]
    Parameter,

    #[regex(r"(?i)выбрать|(?i)select")]
    KwSelect,

    #[regex(r"(?i)из|(?i)from")]
    KwFrom,

    #[regex(r"(?i)поместить|(?i)into")]
    KwInto,

    #[regex(r"(?i)где|(?i)where")]
    KwWhere,

    #[regex(r"(?i)сгруппировать|(?i)group")]
    KwGroup,

    #[regex(r"(?i)упорядочить|(?i)order")]
    KwOrder,

    #[regex(r"(?i)имеющие|(?i)having")]
    KwHaving,

    #[regex(r"(?i)итоги|(?i)totals")]
    KwTotals,

    #[regex(r"(?i)объединить|(?i)union")]
    KwUnion,

    #[regex(r"(?i)все|(?i)all")]
    KwAll,

    #[regex(r"(?i)различные|(?i)distinct")]
    KwDistinct,

    #[regex(r"(?i)первые|(?i)top")]
    KwTop,

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

    #[regex(r"(?i)по|(?i)on|(?i)by")]
    KwOnOrBy,

    #[regex(r"(?i)как|(?i)as")]
    KwAs,

    #[regex(r"(?i)в|(?i)in")]
    KwIn,

    #[regex(r"(?i)между|(?i)between")]
    KwBetween,

    #[regex(r"(?i)подобно|(?i)like")]
    KwLike,

    #[regex(r"(?i)есть|(?i)is")]
    KwIs,

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

    #[regex(r"(?i)и|(?i)and")]
    OpAnd,

    #[regex(r"(?i)или|(?i)or")]
    OpOr,

    #[regex(r"(?i)не|(?i)not")]
    OpNot,

    #[regex(r"(?i)истина|(?i)true")]
    LitTrue,

    #[regex(r"(?i)ложь|(?i)false")]
    LitFalse,

    #[regex(r"(?i)null")]
    LitNull,

    #[regex(r"(?i)уничтожить|(?i)drop")]
    KwDrop,

    #[regex(r"(?i)автоупорядочивание|(?i)autoorder")]
    KwAutoOrder,

    #[regex(r"(?i)возр|(?i)asc")]
    KwAsc,

    #[regex(r"(?i)убыв|(?i)desc")]
    KwDesc,

    #[regex(r"(?i)иерархия|(?i)hierarchy")]
    KwHierarchy,

    #[regex(r"(?i)разрешенные|(?i)allowed")]
    KwAllowed,

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

    #[regex(r"(?i)периодами|(?i)periods")]
    KwPeriods,

    #[regex(r"(?i)спецсимвол|(?i)escape")]
    KwEscape,

    #[regex(r"(?i)ссылка|(?i)refs")]
    KwRefs,

    #[regex(r"(?i)выразить|(?i)cast")]
    KwCast,

    #[regex(r"(?i)тип|(?i)type")]
    KwType,

    #[regex(r"(?i)значение|(?i)value")]
    KwValue,

    #[regex(r"(?i)булево|(?i)boolean")]
    TypeBoolean,

    #[regex(r"(?i)число|(?i)number")]
    TypeNumber,

    #[regex(r"(?i)строка|(?i)string")]
    TypeString,

    #[regex(r"(?i)дата|(?i)date")]
    TypeDate,

    #[regex(r"(?i)неопределено|(?i)undefined")]
    LitUndefined,

    #[regex(r"(?i)декада|(?i)tendays")]
    PeriodTenDays,

    #[regex(r"(?i)полугодие|(?i)halfyear")]
    PeriodHalfYear,

    // ===================================================================
    // LEGACY (Slice 4 pending — query-function vocabulary)
    //
    // Aggregate, date, string, numeric and type/presentation functions.
    // Not yet re-derived from official 1C sources.
    // ===================================================================
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

    // ===================================================================
    // CLEAN-ROOM Slice 3b — metadata-object table vocabulary
    // (in progress)
    //
    // Table roots that name a metadata object as a query data source.
    // Attestation: `docs/legal/sdbl-clean-room-slice3b.md`.
    // ===================================================================
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

    // ===================================================================
    // LEGACY (Slice 5 pending — virtual tables and external data sources)
    //
    // Virtual-table suffixes and the external-data-source table root.
    // Not yet re-derived from official 1C sources.
    // ===================================================================
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

    // ===================================================================
    // CLEAN-ROOM Slice 3b — lexer error fallback (in progress)
    //
    // Attestation: `docs/legal/sdbl-clean-room-slice3b.md`.
    // ===================================================================
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblToken {
    pub kind: SdblTokenKind,
    pub text: SmolStr,
    pub offset: usize,
}

pub fn tokenize_sdbl(input: &str) -> Vec<SdblToken> {
    let mut result = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        let remaining = &input[pos..];

        if remaining.starts_with('"') {
            let strings = strings_mode::scan(input, pos);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_statement() {
        let tokens = tokenize_sdbl("SELECT Name FROM Catalog.Products");
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
        assert_eq!(tokens[0].kind, SdblTokenKind::KwLeft);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwJoin);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::MdoCatalog);
    }

    #[test]
    fn test_aggregate_functions() {
        let tokens = tokenize_sdbl("SUM(Amount) AS Total");
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
        assert_eq!(tokens[6].kind, SdblTokenKind::Parameter);
        assert_eq!(tokens[6].text.as_str(), "&StartDate");
    }

    #[test]
    fn test_temporary_table() {
        let tokens = tokenize_sdbl("INTO #TempTable");
        assert_eq!(tokens[0].kind, SdblTokenKind::KwInto);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Hash);
        assert_eq!(tokens[3].kind, SdblTokenKind::Ident);
    }

    #[test]
    fn test_virtual_table() {
        let tokens = tokenize_sdbl("FROM AccumulationRegister.Stock.Balance");
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
        assert_eq!(tokens[0].kind, SdblTokenKind::FnYear);
        assert_eq!(tokens[4].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[5].kind, SdblTokenKind::FnMonth);
        assert_eq!(tokens[9].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[10].kind, SdblTokenKind::FnDay);
    }

    #[test]
    fn test_string_functions() {
        let tokens = tokenize_sdbl("SUBSTRING(Name, 1, 10) UPPER(Name)");
        assert_eq!(tokens[0].kind, SdblTokenKind::FnSubstring);
        assert_eq!(tokens[10].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[11].kind, SdblTokenKind::FnUpper);
    }

    #[test]
    fn test_group_by() {
        let tokens = tokenize_sdbl("GROUP BY Category HAVING COUNT(*) > 5");
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
        assert_eq!(tokens[8].kind, SdblTokenKind::KwUnion);
        assert_eq!(tokens[9].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[10].kind, SdblTokenKind::KwAll);
    }

    #[test]
    fn test_distinct_top() {
        let tokens = tokenize_sdbl("SELECT DISTINCT TOP 100 Name");
        assert_eq!(tokens[0].kind, SdblTokenKind::KwSelect);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::KwDistinct);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwTop);
    }

    #[test]
    fn test_in_predicate() {
        let tokens = tokenize_sdbl("WHERE Category IN (&CategoryList)");
        assert_eq!(tokens[0].kind, SdblTokenKind::KwWhere);
        assert_eq!(tokens[1].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[2].kind, SdblTokenKind::Ident);
        assert_eq!(tokens[3].kind, SdblTokenKind::Whitespace);
        assert_eq!(tokens[4].kind, SdblTokenKind::KwIn);
    }

    #[test]
    fn test_between_predicate() {
        let tokens = tokenize_sdbl("WHERE Price BETWEEN 100 AND 500");
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
        let input = "SELECT\n   \"\" AS Field";
        let tokens = tokenize_sdbl(input);

        println!("\nMultiline string test tokens:");
        for (i, token) in tokens.iter().enumerate() {
            println!("  {}: {:?} = {:?}", i, token.kind, token.text);
        }

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
        let input = "\"text\nmore\"";
        let tokens = tokenize_sdbl(input);

        println!("\nString with embedded newline tokens:");
        for (i, token) in tokens.iter().enumerate() {
            println!("  {}: {:?} = {:?}", i, token.kind, token.text);
        }

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
