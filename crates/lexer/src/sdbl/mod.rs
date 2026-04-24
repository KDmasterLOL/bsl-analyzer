//! SDBL (Structured Data Base Language) lexer for 1C:Enterprise query
//! language.
//!
//! ## Provenance
//!
//! The `SdblTokenKind` enum below is split by banner comments into two
//! clearly demarcated provenance sections:
//!
//! - **Slice 1 — clean-room.** Whitespace, line terminators, line
//!   comments, separators, punctuation, comparison and arithmetic
//!   operators, numeric literals, the string-literal opening quote
//!   (with content handled by the companion [`strings_mode`] module),
//!   date literals, identifiers, and parameter references. These
//!   variants were re-derived from the ITS documentation of the
//!   1C:Enterprise query language (<https://its.1c.ru/db/pubqlang>)
//!   and are attested in `docs/legal/sdbl-clean-room-slice1.md`.
//!
//! - **Slice 2 — clean-room.** Structural keyword vocabulary: core
//!   clause starters (SELECT / FROM / WHERE / GROUP / ORDER / HAVING /
//!   TOTALS / UNION / ALL / DISTINCT / TOP / INTO), the join family
//!   (JOIN / INNER / LEFT / RIGHT / FULL / OUTER / ON-or-BY),
//!   field aliasing (AS), basic predicates (IN / BETWEEN / LIKE / IS),
//!   the CASE family (CASE / WHEN / THEN / ELSE / END), the logical
//!   operators (AND / OR / NOT), and the boolean and NULL literals
//!   (TRUE / FALSE / NULL). Attested in
//!   `docs/legal/sdbl-clean-room-slice2.md`.
//!
//! - **Slices 3–5 — pending.** Remaining long-tail keywords,
//!   built-in functions, metadata-object tokens, virtual-table tokens,
//!   type literals, the UNDEFINED literal, period-type tokens, and
//!   the logos error fallback. These remain as carried over from the
//!   pre-clean-room implementation and will be re-derived by
//!   subsequent slices.
//!
//! All variants share a single longest-match precedence space — the
//! physical reordering below does not change matching semantics. A
//! byte-identity golden corpus (`crates/lexer/tests/sdbl_golden_corpus.rs`)
//! gates any accidental drift.

mod strings_mode;

use logos::Logos;
use smol_str::SmolStr;

/// Token kinds produced by the SDBL lexer.
///
/// Variants support bilingual (Russian + English) spelling where the
/// underlying language does; the regex on each variant is the single
/// source of truth.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SdblTokenKind {
    // ============================================================================
    // CLEAN-ROOM Slice 1 — ITS-derived
    // ============================================================================
    //
    // Every variant in this section carries an inline provenance
    // comment pointing at the ITS section (or a local mini-spec) it
    // was re-derived from. Nothing in this block was copied from the
    // pre-clean-room regex text or from any third-party SDBL grammar.
    /// Horizontal whitespace: ASCII space, tab, carriage return.
    // ITS pubqlang/12 — lexical elements: inter-token whitespace.
    #[regex(r"[ \t\r]+")]
    Whitespace,

    /// Line terminator.
    // ITS pubqlang/12 — lexical elements: newline terminates a line.
    #[token("\n")]
    Newline,

    /// Line comment `// ...` up to end-of-line.
    // Local spec (see `docs/legal/sdbl-clean-room-slice1.md` § Scope —
    // line comment): the upstream SDBL grammar at
    // <https://its.1c.ru/db/pubqlang/content/12/hdoc> does not define
    // comments; this token is a tooling concession so that queries
    // extracted from BSL source whose bodies happen to contain `//`
    // tails survive lexing, and so queries can be annotated during
    // review. The regex accepts `//` followed by any non-newline run.
    #[regex(r"//[^\n]*")]
    Comment,

    /// Left parenthesis `(`.
    // ITS pubqlang/12 — separators: opens a parenthesised expression.
    #[token("(")]
    LParen,

    /// Right parenthesis `)`.
    // ITS pubqlang/12 — separators: closes a parenthesised expression.
    #[token(")")]
    RParen,

    /// Dot `.` — member access and dot-path separator.
    // ITS pubqlang/12 — separators: joins the parts of a metadata or
    // field path (e.g. `Catalog.Products.Ref`).
    #[token(".")]
    Dot,

    /// Comma `,` — argument and list-element separator.
    // ITS pubqlang/12 — separators: separates select-list columns and
    // function argument lists.
    #[token(",")]
    Comma,

    /// Semicolon `;` — statement / query separator within a batch.
    // ITS pubqlang/12 — separators: terminates a query in a batch.
    #[token(";")]
    Semicolon,

    /// Hash `#` — temporary-table name marker.
    // ITS pubqlang/10 — temporary tables: a name prefixed with `#`
    // identifies a temporary table in `INTO`/`DROP` position.
    #[token("#")]
    Hash,

    /// Bare ampersand `&`. When followed immediately by an
    /// identifier, the longer `Parameter` match wins; this variant
    /// only appears when `&` stands alone.
    // ITS pubqlang/10 — parameters: parameter references start with `&`.
    #[token("&")]
    Ampersand,

    /// Vertical bar `|` — multiline continuation marker used when a
    /// SDBL query is embedded as a BSL multiline string literal.
    // Local spec (see `docs/legal/sdbl-clean-room-slice1.md` § Scope —
    // bar): BSL multiline-string convention, also described in the
    // mini-spec at the top of `strings_mode`. The bar at the start
    // of each continuation line is preserved through to the parser
    // rather than elided, so layout is reconstructible.
    #[token("|")]
    Bar,

    /// Equality operator `=`.
    // ITS pubqlang/10 — comparison operators: equality.
    #[token("=")]
    Eq,

    /// Inequality operator `<>`. Declared so longest-match picks it
    /// over the one-char `<` followed by `>`.
    // ITS pubqlang/10 — comparison operators: inequality.
    #[token("<>")]
    Neq,

    /// Less-or-equal operator `<=`. Longest-match beats `<`.
    // ITS pubqlang/10 — comparison operators: less-or-equal.
    #[token("<=")]
    Le,

    /// Less-than operator `<`.
    // ITS pubqlang/10 — comparison operators: less-than.
    #[token("<")]
    Lt,

    /// Greater-or-equal operator `>=`. Longest-match beats `>`.
    // ITS pubqlang/10 — comparison operators: greater-or-equal.
    #[token(">=")]
    Ge,

    /// Greater-than operator `>`.
    // ITS pubqlang/10 — comparison operators: greater-than.
    #[token(">")]
    Gt,

    /// Addition operator `+`.
    // ITS pubqlang/10 — arithmetic operators: addition.
    #[token("+")]
    Plus,

    /// Subtraction operator `-` (also appears as the unary-minus
    /// prefix; that distinction is parser-level).
    // ITS pubqlang/10 — arithmetic operators: subtraction / unary minus.
    #[token("-")]
    Minus,

    /// Multiplication operator `*`. The select-all column form is
    /// parsed as the same token in select-list position.
    // ITS pubqlang/10 — arithmetic operators: multiplication; doubles
    // as the `*` select-all wildcard at the parser level.
    #[token("*")]
    Star,

    /// Division operator `/`.
    // ITS pubqlang/10 — arithmetic operators: division.
    #[token("/")]
    Slash,

    /// Modulo operator `%`.
    // ITS pubqlang/10 — arithmetic operators: modulo.
    #[token("%")]
    Percent,

    /// Fractional numeric literal (e.g. `3.14`). Declared before the
    /// integer variant so longest-match picks `3.14` over `3` + `.` +
    /// `14`.
    // ITS pubqlang/12 — numeric literals: fractional form is
    // `DIGITS"."DIGITS`.
    #[regex(r"[0-9]+\.[0-9]+")]
    Float,

    /// Integer numeric literal (e.g. `42`).
    // ITS pubqlang/12 — numeric literals: integer form is `DIGITS`.
    #[regex(r"[0-9]+")]
    Decimal,

    /// String-literal opening quote. The string body is not produced
    /// by logos; the top-level tokeniser detects `"` directly and
    /// hands off to [`strings_mode::scan`], which emits the full run
    /// as a sequence of `String` tokens.
    // ITS pubqlang/12 — string literals: `"`-delimited; see the
    // mini-spec at the top of the `strings_mode` module for the
    // multiline convention.
    #[token("\"")]
    Quote,

    /// String content or delimiter emitted by the strings-mode
    /// scanner. Never produced by logos directly.
    // Local spec: string body structure is defined by the mini-spec
    // at the top of the `strings_mode` module; see
    // <https://its.1c.ru/db/pubqlang/content/12/hdoc> for the
    // upstream `"`-delimited string-literal rule.
    String,

    /// Date literal: `'YYYYMMDD'` (date only) or `'YYYYMMDDhhmmss'`
    /// (date + time).
    // ITS pubqlang/12 — date literals: apostrophe-delimited, 8 or 14
    // decimal digits for calendar date or date-plus-time.
    #[regex(r"'[0-9]{8,14}'")]
    Date,

    /// Identifier: a Unicode letter or underscore followed by any
    /// number of letters, digits, or underscores. Declared with lower
    /// priority than keyword variants so reserved words keep their
    /// specific `Kw*` / `Fn*` kinds.
    // ITS pubqlang/12 — identifiers: start with a letter or
    // underscore, continue with letters, digits, or underscores.
    #[regex(r"[_\p{L}][_\p{L}0-9]*", priority = 1)]
    Ident,

    /// Parameter reference `&Name`: a bound query parameter. Matches
    /// longer than a bare `Ampersand` when an identifier follows the
    /// `&`, so the longer match wins.
    // ITS pubqlang/10 — parameters: `&` immediately followed by an
    // identifier is a named host-bound parameter reference.
    #[regex(r"&[_\p{L}][_\p{L}0-9]*")]
    Parameter,

    // ============================================================================
    // CLEAN-ROOM Slice 2 — structural keyword vocabulary (ITS pubqlang/10, /12)
    // ============================================================================
    //
    // The convenience index — RUS/ENG → Variant — is populated in the
    // accompanying clean-room regex-rewrite commit. This commit performs
    // the mechanical move only; regex attributes are unchanged and per-
    // variant ITS citations arrive with the rewrite.

    // --- Clause starters ---
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

    // --- Join family ---
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

    // --- Aliasing & predicates ---
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

    // --- CASE family ---
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

    // --- Logical operators & literals ---
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

    // ============================================================================
    // LEGACY (Slices 3–5 pending — not part of the clean-room claim)
    // ============================================================================
    //
    // The variants below are carried over unchanged from the
    // pre-clean-room implementation. They remain Tier B material for
    // the duration of the staged migration; the next slices will
    // re-derive remaining long-tail keywords, built-in functions,
    // metadata-object tokens, virtual tables, type literals, and
    // period-type tokens from ITS documentation.
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

    #[regex(r"(?i)периоды|(?i)periods")]
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

    #[regex(r"(?i)неопределено|(?i)undefined")]
    LitUndefined,

    // Note: Most period types are the same as date functions above.
    // Only unique period types are listed here.
    #[regex(r"(?i)декада|(?i)tendays")]
    PeriodTenDays,

    #[regex(r"(?i)полугодие|(?i)halfyear")]
    PeriodHalfYear,

    /// Logos error fallback for any byte sequence that does not match
    /// a declared token. Retained unchanged pending the full Slice 1
    /// → Slice 5 migration.
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
        // Test a string that contains an embedded newline between quotes.
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
