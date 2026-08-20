//! BSL (1C:Enterprise embedded language) lexer.
//!
//! ## Provenance
//!
//! The token inventory is re-derived from Глава 4 «Встроенный язык» of the
//! 1C:Enterprise 8.3.27 Developer's Guide
//! (<https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000116>), one vocabulary
//! at a time, with the sections recorded per variant in
//! `docs/legal/bsl-clean-room-slice-b1.md`. Group banners below name the
//! section that defines each vocabulary; the attestation document is
//! authoritative on scope.

pub mod sdbl;

use logos::Logos;
use smol_str::SmolStr;

#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // ===================================================================
    // Зарезервированные слова — таблица 4.2.4.6, тридцать двуязычных пар.
    // Регистр не значим: 4.2.4.5 и примечание к самой таблице.
    // ===================================================================
    #[regex(r"(?i)процедура|(?i)procedure")]
    KwProcedure,

    #[regex(r"(?i)конецпроцедуры|(?i)endprocedure")]
    KwEndProcedure,

    #[regex(r"(?i)функция|(?i)function")]
    KwFunction,

    #[regex(r"(?i)конецфункции|(?i)endfunction")]
    KwEndFunction,

    // ===================================================================
    // Ключевые слова вне таблицы 4.2.4.6.
    //
    // Источник намеренно не резервирует их: они определены разделами, где
    // описана сама конструкция, и потому не запрещены как имена. Разделы —
    // 4.6.1/4.6.3/4.6.4 (Экспорт, Знач, Асинх), 4.6.9 (Ждать),
    // 4.3.2/4.3.3/4.3.5 (литералы), 4.6.11 (обработчики событий).
    // ===================================================================
    #[regex(r"(?i)экспорт|(?i)export")]
    KwExport,

    #[regex(r"(?i)знач|(?i)val")]
    KwVal,

    #[regex(r"(?i)если|(?i)if")]
    KwIf,

    #[regex(r"(?i)тогда|(?i)then")]
    KwThen,

    #[regex(r"(?i)иначеесли|(?i)elsif")]
    KwElsIf,

    #[regex(r"(?i)иначе|(?i)else")]
    KwElse,

    #[regex(r"(?i)конецесли|(?i)endif")]
    KwEndIf,

    #[regex(r"(?i)для|(?i)for")]
    KwFor,

    #[regex(r"(?i)каждого|(?i)each")]
    KwEach,

    #[regex(r"(?i)из|(?i)in")]
    KwIn,

    #[regex(r"(?i)по|(?i)to")]
    KwTo,

    #[regex(r"(?i)пока|(?i)while")]
    KwWhile,

    #[regex(r"(?i)цикл|(?i)do")]
    KwDo,

    #[regex(r"(?i)конеццикла|(?i)enddo")]
    KwEndDo,

    #[regex(r"(?i)возврат|(?i)return")]
    KwReturn,

    #[regex(r"(?i)продолжить|(?i)continue")]
    KwContinue,

    #[regex(r"(?i)прервать|(?i)break")]
    KwBreak,

    #[regex(r"(?i)перейти|(?i)goto")]
    KwGoto,

    #[regex(r"(?i)попытка|(?i)try")]
    KwTry,

    #[regex(r"(?i)исключение|(?i)except")]
    KwExcept,

    #[regex(r"(?i)конецпопытки|(?i)endtry")]
    KwEndTry,

    #[regex(r"(?i)вызватьисключение|(?i)raise")]
    KwRaise,

    #[regex(r"(?i)перем|(?i)var")]
    KwVar,

    #[regex(r"(?i)новый|(?i)new")]
    KwNew,

    #[regex(r"(?i)выполнить|(?i)execute")]
    KwExecute,

    #[regex(r"(?i)добавитьобработчик|(?i)addhandler")]
    KwAddHandler,

    #[regex(r"(?i)удалитьобработчик|(?i)removehandler")]
    KwRemoveHandler,

    #[regex(r"(?i)асинх|(?i)async")]
    KwAsync,

    #[regex(r"(?i)ждать|(?i)await")]
    KwAwait,

    #[regex(r"(?i)и|(?i)and")]
    KwAnd,

    #[regex(r"(?i)или|(?i)or")]
    KwOr,

    #[regex(r"(?i)не|(?i)not")]
    KwNot,

    #[regex(r"(?i)истина|(?i)true")]
    KwTrue,

    #[regex(r"(?i)ложь|(?i)false")]
    KwFalse,

    #[regex(r"(?i)неопределено|(?i)undefined")]
    KwUndefined,

    #[regex(r"(?i)null")]
    KwNull,

    // ===================================================================
    // Инструкции препроцессора — таблица 4.8.1.2.
    //
    // Список закрыт: открытой формы `#<имя>` источник не знает, поэтому
    // `#` перед неизвестным словом остаётся нераспознанным текстом.
    // ===================================================================
    #[regex(r"#[ \t]*(?i:если|if)")]
    PreIf,

    #[regex(r"#[ \t]*(?i:иначеесли|elsif)")]
    PreElsIf,

    #[regex(r"#[ \t]*(?i:иначе|else)")]
    PreElse,

    #[regex(r"#[ \t]*(?i:конецесли|endif)")]
    PreEndIf,

    #[regex(r"#[ \t]*(?i:область|region)")]
    PreRegion,

    #[regex(r"#[ \t]*(?i:конецобласти|endregion)")]
    PreEndRegion,

    #[regex(r"#(?i)вставка|#(?i)insert")]
    PreInsert,

    #[regex(r"#(?i)конецвставки|#(?i)endinsert")]
    PreEndInsert,

    #[regex(r"#(?i)удаление|#(?i)delete")]
    PreDelete,

    #[regex(r"#(?i)конецудаления|#(?i)enddelete")]
    PreEndDelete,

    // ===================================================================
    // Директивы компиляции — таблица 4.8.1.3.
    //
    // Имя семейства `Ann*` унаследовано: по источнику это директивы
    // компиляции, а не аннотации 4.8.2.
    // ===================================================================
    #[regex(r"&(?i)наклиенте|&(?i)atclient")]
    AnnAtClient,

    #[regex(r"&(?i)насервере|&(?i)atserver")]
    AnnAtServer,

    #[regex(r"&(?i)насерверебезконтекста|&(?i)atservernocontext")]
    AnnAtServerNoContext,

    #[regex(r"&(?i)наклиентенасерверебезконтекста|&(?i)atclientatservernocontext")]
    AnnAtClientAtServerNoContext,

    #[regex(r"&(?i)наклиентенасервере|&(?i)atclientatserver")]
    AnnAtClientAtServer,

    // ===================================================================
    // Аннотации — таблица 4.8.2.
    // ===================================================================
    #[regex(r"&(?i)перед|&(?i)before")]
    AnnBefore,

    #[regex(r"&(?i)после|&(?i)after")]
    AnnAfter,

    /// `&Instead` источником не задан и оставлен разрешением совместимости
    /// с предшественником: 4.8.2 даёт английское имя `Вместо` как `Around`.
    #[regex(r"&(?i)вместо|&(?i)around|&(?i)instead")]
    AnnAround,

    #[regex(r"&(?i)изменениеиконтроль|&(?i)changeandvalidate")]
    AnnChangeAndValidate,

    /// Языковой формой не является: 4.8.2 гласит, что система не поддерживает
    /// пользовательских аннотаций. Разрешение IDE — неизвестное написание
    /// после `&` должно оставаться опознаваемой аннотацией, иначе
    /// восстановление теряет весь заголовок метода.
    #[regex(r"&[_\p{L}][_\p{L}0-9]*")]
    AnnCustom,

    // ===================================================================
    // Операторы и пунктуация — таблица 4.2.5, приоритеты 4.5.4.
    // ===================================================================
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

    /// Приходит только преобразованием лексем SDBL.
    ///
    /// В Главе 4 фигурных скобок нет: они принадлежат расширению
    /// языка запросов для системы компоновки данных.
    LBrace,

    /// Приходит только преобразованием лексем SDBL.
    ///
    /// Закрывающая половина пары; см. [`TokenKind::LBrace`].
    RBrace,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token(".")]
    Dot,

    #[token(",")]
    Comma,

    #[token(";")]
    Semicolon,

    #[token(":")]
    Colon,

    #[token("?")]
    Question,

    #[token("~")]
    Tilde,

    /// Приходит только преобразованием лексем SDBL.
    ///
    /// 4.2.5 даёт `|` только внутри строковой константы, и перенос строки
    /// лексер берёт целиком видами `StringPart` / `StringTail`. Образец на
    /// одиночный `|` был недостижим: он проигрывал `StringPart` по приоритету.
    Bar,

    /// Приходит только преобразованием лексем SDBL.
    ///
    /// 4.8.1.2 знает `#` только зачином инструкции из закрытого списка,
    /// и всю инструкцию лексер берёт целиком видом `Pre*`.
    Hash,

    /// Приходит только преобразованием лексем SDBL.
    ///
    /// 4.8.1.3 и 4.8.2 знают `&` только зачином директивы или аннотации
    /// из закрытых списков, и всё написание лексер берёт целиком видом `Ann*`.
    Ampersand,

    // ===================================================================
    // Литералы — 4.3: число 4.3.8, строка 4.3.6, дата 4.3.4.
    // ===================================================================
    #[regex(r"[0-9]+\.[0-9]*")]
    Float,

    #[regex(r"[0-9]+")]
    Decimal,

    #[regex(r#""([^"\n\r]|"")*""#)]
    String,

    #[regex(r#""([^"\n\r]|"")*"#)]
    StringStart,

    #[regex(r#"\|([^"\n\r]|"")*""#, priority = 3)]
    StringTail,

    #[regex(r#"\|([^"\n\r]|"")*"#, priority = 2)]
    StringPart,

    /// Одинарные кавычки в BSL обрамляют только литерал даты — 4.2.5.
    ///
    /// 4.3.4 говорит, что в литерале игнорируются все значения, отличные от
    /// цифр, и приводит `Дата('2017\03\23 10~45~25')`, поэтому класс
    /// разделителей открыт. Он всё же сужен на буквы, кавычки и перевод
    /// строки: иначе один незакрытый апостроф поглощал бы идентификаторы,
    /// строковый литерал и остаток модуля, а так его радиус — одна строка.
    /// Проверка самого значения даты — дело диагностики, не лексера.
    #[regex(r#"'[^'"\n\r\p{L}]*'"#)]
    Date,

    #[regex(r"[_\p{L}][_\p{L}0-9]*", priority = 1)]
    Ident,

    #[regex(r"//[^\n]*")]
    Comment,

    #[token("\n")]
    Newline,

    #[regex(r"[ \t\r\x{00A0}]+", priority = 0)]
    Whitespace,

    #[token("\u{FEFF}")]
    Bom,

    Error,
}

impl TokenKind {
    /// Все виды по порядку объявления.
    ///
    /// Нужен перебору: свойство, проверенное на выборке видов, зелено и у
    /// таблицы, разошедшейся на не вошедшем в выборку. Полнота списка сама
    /// под тестом — `the_list_of_kinds_covers_every_variant`, — потому что
    /// список, отставший от перечисления, ослабляет перебор молча.
    pub const ALL: &'static [TokenKind] = &[
        TokenKind::KwProcedure,
        TokenKind::KwEndProcedure,
        TokenKind::KwFunction,
        TokenKind::KwEndFunction,
        TokenKind::KwExport,
        TokenKind::KwVal,
        TokenKind::KwIf,
        TokenKind::KwThen,
        TokenKind::KwElsIf,
        TokenKind::KwElse,
        TokenKind::KwEndIf,
        TokenKind::KwFor,
        TokenKind::KwEach,
        TokenKind::KwIn,
        TokenKind::KwTo,
        TokenKind::KwWhile,
        TokenKind::KwDo,
        TokenKind::KwEndDo,
        TokenKind::KwReturn,
        TokenKind::KwContinue,
        TokenKind::KwBreak,
        TokenKind::KwGoto,
        TokenKind::KwTry,
        TokenKind::KwExcept,
        TokenKind::KwEndTry,
        TokenKind::KwRaise,
        TokenKind::KwVar,
        TokenKind::KwNew,
        TokenKind::KwExecute,
        TokenKind::KwAddHandler,
        TokenKind::KwRemoveHandler,
        TokenKind::KwAsync,
        TokenKind::KwAwait,
        TokenKind::KwAnd,
        TokenKind::KwOr,
        TokenKind::KwNot,
        TokenKind::KwTrue,
        TokenKind::KwFalse,
        TokenKind::KwUndefined,
        TokenKind::KwNull,
        TokenKind::PreIf,
        TokenKind::PreElsIf,
        TokenKind::PreElse,
        TokenKind::PreEndIf,
        TokenKind::PreRegion,
        TokenKind::PreEndRegion,
        TokenKind::PreInsert,
        TokenKind::PreEndInsert,
        TokenKind::PreDelete,
        TokenKind::PreEndDelete,
        TokenKind::AnnAtClient,
        TokenKind::AnnAtServer,
        TokenKind::AnnAtServerNoContext,
        TokenKind::AnnAtClientAtServerNoContext,
        TokenKind::AnnAtClientAtServer,
        TokenKind::AnnBefore,
        TokenKind::AnnAfter,
        TokenKind::AnnAround,
        TokenKind::AnnChangeAndValidate,
        TokenKind::AnnCustom,
        TokenKind::Eq,
        TokenKind::Neq,
        TokenKind::Le,
        TokenKind::Lt,
        TokenKind::Ge,
        TokenKind::Gt,
        TokenKind::Plus,
        TokenKind::Minus,
        TokenKind::Star,
        TokenKind::Slash,
        TokenKind::Percent,
        TokenKind::LParen,
        TokenKind::RParen,
        TokenKind::LBrace,
        TokenKind::RBrace,
        TokenKind::LBracket,
        TokenKind::RBracket,
        TokenKind::Dot,
        TokenKind::Comma,
        TokenKind::Semicolon,
        TokenKind::Colon,
        TokenKind::Question,
        TokenKind::Tilde,
        TokenKind::Bar,
        TokenKind::Hash,
        TokenKind::Ampersand,
        TokenKind::Float,
        TokenKind::Decimal,
        TokenKind::String,
        TokenKind::StringStart,
        TokenKind::StringTail,
        TokenKind::StringPart,
        TokenKind::Date,
        TokenKind::Ident,
        TokenKind::Comment,
        TokenKind::Newline,
        TokenKind::Whitespace,
        TokenKind::Bom,
        TokenKind::Error,
    ];

    /// Лексема, которую грамматика не разбирает: пробелы, переводы строки,
    /// комментарии, BOM.
    ///
    /// Определение живёт здесь, потому что вид лексемы принадлежит лексеру.
    /// Слою дерева отвечает `SyntaxKind::is_trivia`, и согласие двух
    /// предикатов держится перебором всех видов.
    pub const fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace | TokenKind::Comment | TokenKind::Newline | TokenKind::Bom
        )
    }

    pub fn is_keyword(self) -> bool {
        matches!(
            self,
            TokenKind::KwProcedure
                | TokenKind::KwEndProcedure
                | TokenKind::KwFunction
                | TokenKind::KwEndFunction
                | TokenKind::KwExport
                | TokenKind::KwVal
                | TokenKind::KwIf
                | TokenKind::KwThen
                | TokenKind::KwElsIf
                | TokenKind::KwElse
                | TokenKind::KwEndIf
                | TokenKind::KwFor
                | TokenKind::KwEach
                | TokenKind::KwIn
                | TokenKind::KwTo
                | TokenKind::KwWhile
                | TokenKind::KwDo
                | TokenKind::KwEndDo
                | TokenKind::KwReturn
                | TokenKind::KwContinue
                | TokenKind::KwBreak
                | TokenKind::KwGoto
                | TokenKind::KwTry
                | TokenKind::KwExcept
                | TokenKind::KwEndTry
                | TokenKind::KwRaise
                | TokenKind::KwVar
                | TokenKind::KwNew
                | TokenKind::KwExecute
                | TokenKind::KwAddHandler
                | TokenKind::KwRemoveHandler
                | TokenKind::KwAsync
                | TokenKind::KwAwait
                | TokenKind::KwAnd
                | TokenKind::KwOr
                | TokenKind::KwNot
                | TokenKind::KwTrue
                | TokenKind::KwFalse
                | TokenKind::KwUndefined
                | TokenKind::KwNull
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: SmolStr,
    pub offset: usize,
}

pub fn tokenize(input: &str) -> Vec<Token> {
    let mut lexer = TokenKind::lexer(input);
    let mut tokens = Vec::new();

    while let Some(result) = lexer.next() {
        let kind = result.unwrap_or(TokenKind::Error);
        let text = SmolStr::new(lexer.slice());
        let offset = lexer.span().start;
        tokens.push(Token { kind, text, offset });
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords_russian() {
        let tokens = tokenize("Процедура Тест() КонецПроцедуры");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwProcedure);
        assert_eq!(non_whitespace[1].kind, TokenKind::Ident);
        assert_eq!(non_whitespace[2].kind, TokenKind::LParen);
        assert_eq!(non_whitespace[3].kind, TokenKind::RParen);
        assert_eq!(non_whitespace[4].kind, TokenKind::KwEndProcedure);
    }

    #[test]
    fn test_keywords_english() {
        let tokens = tokenize("Procedure Test() EndProcedure");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwProcedure);
        assert_eq!(non_whitespace[1].kind, TokenKind::Ident);
        assert_eq!(non_whitespace[2].kind, TokenKind::LParen);
        assert_eq!(non_whitespace[3].kind, TokenKind::RParen);
        assert_eq!(non_whitespace[4].kind, TokenKind::KwEndProcedure);
    }

    #[test]
    fn test_keywords_case_insensitive() {
        let tokens = tokenize("пРоЦеДуРа тЕсТ() кОнЕцПрОцЕдУрЫ");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwProcedure);
        assert_eq!(non_whitespace[4].kind, TokenKind::KwEndProcedure);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize(r#""Hello, World!""#);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].text.as_str(), r#""Hello, World!""#);
    }

    #[test]
    fn test_string_with_escaped_quotes() {
        let tokens = tokenize(r#""Hello ""World""!""#);
        assert_eq!(tokens[0].kind, TokenKind::String);
    }

    #[test]
    fn test_multiline_string_parts() {
        let tokens = tokenize("\"Line1\n|Line2\"");
        assert_eq!(tokens[0].kind, TokenKind::StringStart);
        assert_eq!(tokens[1].kind, TokenKind::Newline);
        assert_eq!(tokens[2].kind, TokenKind::StringTail);
    }

    #[test]
    fn test_decimal_number() {
        let tokens = tokenize("123");
        assert_eq!(tokens[0].kind, TokenKind::Decimal);
        assert_eq!(tokens[0].text.as_str(), "123");
    }

    #[test]
    fn test_float_number() {
        let tokens = tokenize("123.456");
        assert_eq!(tokens[0].kind, TokenKind::Float);
        assert_eq!(tokens[0].text.as_str(), "123.456");
    }

    #[test]
    fn test_float_number_with_trailing_dot() {
        let tokens = tokenize("0.");
        assert_eq!(tokens[0].kind, TokenKind::Float);
        assert_eq!(tokens[0].text.as_str(), "0.");
    }

    #[test]
    fn test_date_literal() {
        let tokens = tokenize("'20240101'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
    }

    #[test]
    fn test_iso_date_literal() {
        let tokens = tokenize("'0001-01-01'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
    }

    #[test]
    fn test_dotted_date_literals() {
        let tokens = tokenize("'0001.01.01' '1000.01.01 00:00.00'");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();

        assert_eq!(non_whitespace[0].kind, TokenKind::Date);
        assert_eq!(non_whitespace[1].kind, TokenKind::Date);
    }

    #[test]
    fn test_comma_separated_date_literal() {
        let tokens = tokenize("'0001,01,01'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
    }

    #[test]
    fn test_datetime_literal() {
        let tokens = tokenize("'20240101120000'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
    }

    #[test]
    fn test_iso_datetime_literal() {
        let tokens = tokenize("'0001-01-01 09:00:00'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn test_digits_with_spaced_time_literal() {
        let tokens = tokenize("'00010101 22:00'");
        assert_eq!(tokens[0].kind, TokenKind::Date);
        assert_eq!(tokens.len(), 1);
    }

    #[test]
    fn test_empty_date_literal() {
        let tokens = tokenize("''");
        assert_eq!(tokens[0].kind, TokenKind::Date);
    }

    #[test]
    fn test_date_literal_adjacent_to_operator() {
        let tokens = tokenize("'00010101'+1");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![TokenKind::Date, TokenKind::Plus, TokenKind::Decimal]);
    }

    #[test]
    fn test_apostrophe_inside_multiline_string_is_not_date() {
        let tokens = tokenize("\"a\n|'text'\"");
        assert!(tokens.iter().all(|t| t.kind != TokenKind::Date));
    }

    #[test]
    fn test_comment() {
        let tokens = tokenize("// This is a comment\nПеременная");
        assert_eq!(tokens[0].kind, TokenKind::Comment);
        assert_eq!(tokens[1].kind, TokenKind::Newline);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
    }

    #[test]
    fn test_nbsp_is_whitespace() {
        let tokens = tokenize("А\u{00A0}=\u{00A0}1;\u{00A0}");
        assert_eq!(tokens[1].kind, TokenKind::Whitespace);
        assert_eq!(tokens[3].kind, TokenKind::Whitespace);
        assert_eq!(tokens[6].kind, TokenKind::Whitespace);
    }

    #[test]
    fn test_preprocessor_region() {
        let tokens = tokenize("#Область Тест\n#КонецОбласти");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::PreRegion);
        assert_eq!(non_whitespace[1].kind, TokenKind::Ident);
        assert_eq!(non_whitespace[2].kind, TokenKind::Newline);
        assert_eq!(non_whitespace[3].kind, TokenKind::PreEndRegion);
    }

    #[test]
    fn test_preprocessor_if() {
        let tokens = tokenize("#Если Клиент Тогда\n#КонецЕсли");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::PreIf);
        assert_eq!(non_whitespace[1].kind, TokenKind::Ident);
        assert_eq!(non_whitespace[2].kind, TokenKind::KwThen);
    }

    #[test]
    fn test_preprocessor_directives_with_space_after_hash() {
        let tokens = tokenize(
            "# Если Клиент Тогда\n# ИначеЕсли Сервер Тогда\n# Иначе\n# КонецЕсли\n# Область Тест\n# КонецОбласти",
        );
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();

        assert_eq!(non_whitespace[0].kind, TokenKind::PreIf);
        assert_eq!(non_whitespace[4].kind, TokenKind::PreElsIf);
        assert_eq!(non_whitespace[8].kind, TokenKind::PreElse);
        assert_eq!(non_whitespace[10].kind, TokenKind::PreEndIf);
        assert_eq!(non_whitespace[12].kind, TokenKind::PreRegion);
        assert_eq!(non_whitespace[15].kind, TokenKind::PreEndRegion);
    }

    #[test]
    fn test_preprocessor_directive_with_tab_after_hash() {
        let tokens = tokenize("#\tЕсли Клиент Тогда");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();

        assert_eq!(non_whitespace[0].kind, TokenKind::PreIf);
    }

    #[test]
    fn test_annotations() {
        let tokens = tokenize("&НаКлиенте");
        assert_eq!(tokens[0].kind, TokenKind::AnnAtClient);
    }

    #[test]
    fn test_custom_annotation() {
        let tokens = tokenize("&МояАннотация");
        assert_eq!(tokens[0].kind, TokenKind::AnnCustom);
    }

    #[test]
    fn test_extension_annotations() {
        let tokens = tokenize("&Перед");
        assert_eq!(tokens[0].kind, TokenKind::AnnBefore);

        let tokens = tokenize("&После");
        assert_eq!(tokens[0].kind, TokenKind::AnnAfter);

        let tokens = tokenize("&Вместо");
        assert_eq!(tokens[0].kind, TokenKind::AnnAround);

        let tokens = tokenize("&ИзменениеИКонтроль");
        assert_eq!(tokens[0].kind, TokenKind::AnnChangeAndValidate);

        let tokens = tokenize("&Вместо(\"Метод\")");
        assert_eq!(tokens[0].kind, TokenKind::AnnAround);
        assert_eq!(tokens[1].kind, TokenKind::LParen);
    }

    /// Английские написания директив компиляции и аннотаций — 4.8.1.3 и 4.8.2.
    ///
    /// Перебором, а не выборкой: у `&Around` длина совпадения буквально равна
    /// длине `AnnCustom`, и разводит их только приоритет logos. Проверка на
    /// одном написании зелена и у таблицы, разошедшейся на соседнем.
    #[test]
    fn english_spellings_of_directives_and_annotations() {
        for (input, expected) in [
            ("&AtClient", TokenKind::AnnAtClient),
            ("&AtServer", TokenKind::AnnAtServer),
            ("&AtServerNoContext", TokenKind::AnnAtServerNoContext),
            ("&AtClientAtServerNoContext", TokenKind::AnnAtClientAtServerNoContext),
            ("&AtClientAtServer", TokenKind::AnnAtClientAtServer),
            ("&Before", TokenKind::AnnBefore),
            ("&After", TokenKind::AnnAfter),
            ("&Around", TokenKind::AnnAround),
            ("&ChangeAndValidate", TokenKind::AnnChangeAndValidate),
        ] {
            let tokens = tokenize(input);
            assert_eq!(tokens.len(), 1, "{input}: ожидалась одна лексема, получено {tokens:?}");
            assert_eq!(tokens[0].kind, expected, "{input}");
        }
    }

    /// `&Instead` источником не задан и держится разрешением совместимости.
    #[test]
    fn instead_is_kept_as_a_compatibility_spelling() {
        assert_eq!(tokenize("&Instead")[0].kind, TokenKind::AnnAround);
    }

    #[test]
    fn test_logical_operators() {
        let tokens = tokenize("И ИЛИ НЕ");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwAnd);
        assert_eq!(non_whitespace[1].kind, TokenKind::KwOr);
        assert_eq!(non_whitespace[2].kind, TokenKind::KwNot);
    }

    #[test]
    fn test_comparison_operators() {
        let tokens = tokenize("< <= > >= = <>");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::Lt);
        assert_eq!(non_whitespace[1].kind, TokenKind::Le);
        assert_eq!(non_whitespace[2].kind, TokenKind::Gt);
        assert_eq!(non_whitespace[3].kind, TokenKind::Ge);
        assert_eq!(non_whitespace[4].kind, TokenKind::Eq);
        assert_eq!(non_whitespace[5].kind, TokenKind::Neq);
    }

    #[test]
    fn test_arithmetic_operators() {
        let tokens = tokenize("+ - * / %");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::Plus);
        assert_eq!(non_whitespace[1].kind, TokenKind::Minus);
        assert_eq!(non_whitespace[2].kind, TokenKind::Star);
        assert_eq!(non_whitespace[3].kind, TokenKind::Slash);
        assert_eq!(non_whitespace[4].kind, TokenKind::Percent);
    }

    #[test]
    fn test_async_await() {
        let tokens = tokenize("Асинх Функция Тест() Ждать Результат; КонецФункции");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwAsync);
        assert_eq!(non_whitespace[1].kind, TokenKind::KwFunction);
        assert_eq!(non_whitespace[5].kind, TokenKind::KwAwait);
    }

    #[test]
    fn test_event_handlers() {
        let tokens = tokenize("ДобавитьОбработчик УдалитьОбработчик");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwAddHandler);
        assert_eq!(non_whitespace[1].kind, TokenKind::KwRemoveHandler);
    }

    #[test]
    fn test_tilde_for_label() {
        let tokens = tokenize("~Метка:");
        assert_eq!(tokens[0].kind, TokenKind::Tilde);
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[2].kind, TokenKind::Colon);
    }

    #[test]
    fn test_goto_label() {
        let tokens = tokenize("Перейти ~Метка;\n~Метка:\nВозврат;");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwGoto);
        assert_eq!(non_whitespace[1].kind, TokenKind::Tilde);
        assert_eq!(non_whitespace[2].kind, TokenKind::Ident);
        assert_eq!(non_whitespace[3].kind, TokenKind::Semicolon);
    }

    #[test]
    fn test_boolean_literals() {
        let tokens = tokenize("Истина Ложь True False");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwTrue);
        assert_eq!(non_whitespace[1].kind, TokenKind::KwFalse);
        assert_eq!(non_whitespace[2].kind, TokenKind::KwTrue);
        assert_eq!(non_whitespace[3].kind, TokenKind::KwFalse);
    }

    #[test]
    fn test_undefined_null() {
        let tokens = tokenize("Неопределено Null");
        let non_whitespace: Vec<_> =
            tokens.iter().filter(|t| t.kind != TokenKind::Whitespace).collect();
        assert_eq!(non_whitespace[0].kind, TokenKind::KwUndefined);
        assert_eq!(non_whitespace[1].kind, TokenKind::KwNull);
    }

    #[test]
    fn test_execute_keyword() {
        let tokens = tokenize("Выполнить");
        assert_eq!(tokens[0].kind, TokenKind::KwExecute);
    }

    #[test]
    fn test_complete_procedure() {
        let code = r#"
Процедура Тест(Знач Параметр)
    Если Параметр > 0 Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецПроцедуры
"#;
        let tokens = tokenize(code);

        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwProcedure));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwVal));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwIf));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwReturn));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwTrue));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwFalse));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwEndProcedure));
    }

    #[test]
    fn test_whitespace_tokenization() {
        let code = "Функция ОпределитьСтавкуНДС(Знач Ставка)";
        let tokens = tokenize(code);

        eprintln!("=== Direct Lexer Output ===");
        for (i, tok) in tokens.iter().enumerate() {
            eprintln!(
                "{}: {:?} @ {}..{} = {:?}",
                i,
                tok.kind,
                tok.offset,
                tok.offset + tok.text.len(),
                tok.text
            );
        }

        assert_eq!(tokens[0].kind, TokenKind::KwFunction);
        assert_eq!(tokens[1].kind, TokenKind::Whitespace);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
        assert_eq!(tokens[2].text.as_str(), "ОпределитьСтавкуНДС");
    }

    #[test]
    fn test_string_with_paren_sdbl() {
        let input = r#"" (" + y"#;
        let tokens = tokenize(input);

        eprintln!("Tokens for {:?}:", input);
        for (i, tok) in tokens.iter().enumerate() {
            eprintln!("  [{}] {:?}: {:?}", i, tok.kind, tok.text);
        }

        assert_eq!(tokens[0].kind, TokenKind::String, "First token should be String");
        assert_eq!(
            tokens[0].text.as_str(),
            r#"" (""#,
            "First string should be entire ' (' with quotes"
        );
    }

    #[test]
    fn test_bom() {
        let input = "\u{FEFF}Процедура Тест() КонецПроцедуры";
        let tokens = tokenize(input);

        eprintln!("Tokens with BOM:");
        for (i, tok) in tokens.iter().enumerate() {
            eprintln!("  [{}] {:?}: {:?}", i, tok.kind, tok.text);
        }

        assert_eq!(tokens[0].kind, TokenKind::Bom, "First token should be BOM");
        assert_eq!(tokens[0].text.as_str(), "\u{FEFF}");
        assert_eq!(tokens[1].kind, TokenKind::KwProcedure);
    }
}

#[cfg(test)]
mod kind_list_tests {
    use super::TokenKind;

    /// Список видов покрывает перечисление целиком и в порядке объявления.
    ///
    /// Проверяется через дискриминанты, а не через длину: вид, вставленный в
    /// середину, длину бы не изменил, если заодно удалить другой, — а порядок
    /// сдвинул бы. `Error` объявлен последним, и на этом держится счёт.
    #[test]
    fn the_list_of_kinds_covers_every_variant() {
        for (index, kind) in TokenKind::ALL.iter().enumerate() {
            assert_eq!(*kind as usize, index, "список видов разошёлся с перечислением на {kind:?}");
        }
        assert_eq!(
            TokenKind::ALL.len(),
            TokenKind::Error as usize + 1,
            "в перечислении появились виды, не попавшие в список"
        );
    }
}
