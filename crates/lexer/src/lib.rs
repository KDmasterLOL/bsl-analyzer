//! Lexer for BSL (1C:Enterprise) language.
//!
//! Supports both Russian and English keywords (case-insensitive).
//!
//! ## Token Structure
//!
//! The lexer recognizes the following token categories:
//! - Keywords (bilingual: Russian/English)
//! - Preprocessor directives (#If, #Region, etc.)
//! - Annotations (&AtClient, &AtServer, etc.)
//! - Operators (arithmetic, comparison, logical)
//! - Punctuation
//! - Literals (numbers, strings, dates, booleans)
//! - Comments
//!
//! ## SDBL Support
//!
//! The `sdbl` module provides a separate lexer for SDBL (query language) tokens.
//! SDBL queries are embedded in BSL string literals.
//!
pub mod sdbl;

use logos::Logos;
use smol_str::SmolStr;

/// Token kinds for BSL language.
///
/// Each token represents a lexical element in BSL source code.
/// Keywords support both Russian and English variants (case-insensitive).
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    // Note: Using regex with (?i) for case-insensitive matching of both ASCII and Cyrillic

    // Procedure/Function keywords
    #[regex(r"(?i)процедура|(?i)procedure")]
    KwProcedure,

    #[regex(r"(?i)конецпроцедуры|(?i)endprocedure")]
    KwEndProcedure,

    #[regex(r"(?i)функция|(?i)function")]
    KwFunction,

    #[regex(r"(?i)конецфункции|(?i)endfunction")]
    KwEndFunction,

    #[regex(r"(?i)экспорт|(?i)export")]
    KwExport,

    #[regex(r"(?i)знач|(?i)val")]
    KwVal,

    // Control flow keywords
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

    // Loop keywords
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

    // Exception handling
    #[regex(r"(?i)попытка|(?i)try")]
    KwTry,

    #[regex(r"(?i)исключение|(?i)except")]
    KwExcept,

    #[regex(r"(?i)конецпопытки|(?i)endtry")]
    KwEndTry,

    #[regex(r"(?i)вызватьисключение|(?i)raise")]
    KwRaise,

    // Variable and value keywords
    #[regex(r"(?i)перем|(?i)var")]
    KwVar,

    #[regex(r"(?i)новый|(?i)new")]
    KwNew,

    #[regex(r"(?i)выполнить|(?i)execute")]
    KwExecute,

    // Event handlers
    #[regex(r"(?i)добавитьобработчик|(?i)addhandler")]
    KwAddHandler,

    #[regex(r"(?i)удалитьобработчик|(?i)removehandler")]
    KwRemoveHandler,

    // Async/Await
    #[regex(r"(?i)асинх|(?i)async")]
    KwAsync,

    #[regex(r"(?i)ждать|(?i)await")]
    KwAwait,

    // Logical operators
    #[regex(r"(?i)и|(?i)and")]
    KwAnd,

    #[regex(r"(?i)или|(?i)or")]
    KwOr,

    #[regex(r"(?i)не|(?i)not")]
    KwNot,

    // Boolean literals
    #[regex(r"(?i)истина|(?i)true")]
    KwTrue,

    #[regex(r"(?i)ложь|(?i)false")]
    KwFalse,

    // Special values
    #[regex(r"(?i)неопределено|(?i)undefined")]
    KwUndefined,

    #[regex(r"(?i)null")]
    KwNull,

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

    #[regex(r"#(?i)использовать|#(?i)use")]
    PreUse,

    #[regex(r"#(?i)вставка|#(?i)insert")]
    PreInsert,

    #[regex(r"#(?i)конецвставки|#(?i)endinsert")]
    PreEndInsert,

    #[regex(r"#(?i)удаление|#(?i)delete")]
    PreDelete,

    #[regex(r"#(?i)конецудаления|#(?i)enddelete")]
    PreEndDelete,

    // NOTE: Preprocessor platform/OS symbols (Клиент, НаКлиенте, Сервер, Linux, etc.)
    // are NOT separate tokens. They are recognized as Ident and checked by the parser
    // in preprocessor expression context. This prevents false matches like "НаКлиенте"
    // in "Процедура НаКлиенте()" being tokenized as PreAtClient instead of Ident.
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

    #[regex(r"&(?i)перед|&(?i)before")]
    AnnBefore,

    #[regex(r"&(?i)после|&(?i)after")]
    AnnAfter,

    #[regex(r"&(?i)вместо|&(?i)instead")]
    AnnAround,

    #[regex(r"&(?i)изменениеиконтроль|&(?i)changeandvalidate")]
    AnnChangeAndValidate,

    // Custom annotation (any identifier after &)
    #[regex(r"&[_\p{L}][_\p{L}0-9]*")]
    AnnCustom,

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
    Tilde, // For labels: ~LabelName:

    #[token("|", priority = 1)]
    Bar, // For multiline strings (lower priority than StringPart/StringTail)

    #[token("#")]
    Hash, // Generic preprocessor marker

    #[token("&")]
    Ampersand, // For annotations

    #[token("!")]
    Exclamation, // Used in preprocessor (e.g., #!)

    // Numbers: floats must come before integers to match correctly
    #[regex(r"[0-9]+\.[0-9]*")]
    Float,

    #[regex(r"[0-9]+")]
    Decimal,

    // Strings: "..." with "" escaping
    #[regex(r#""([^"\n\r]|"")*""#)]
    String,

    // String start (for multiline strings): "...
    // Without closing quote before newline
    #[regex(r#""([^"\n\r]|"")*"#)]
    StringStart,

    // String tail: |..."
    #[regex(r#"\|([^"\n\r]|"")*""#, priority = 3)]
    StringTail,

    // String continuation: |...
    #[regex(r#"\|([^"\n\r]|"")*"#, priority = 2)]
    StringPart,

    // Date literals:
    // 'YYYYMMDD', 'YYYYMMDDHHMMSS', 'YYYY-MM-DD', 'YYYY.MM.DD',
    // 'YYYY.MM.DD HH:MM.SS', or 'YYYY,MM,DD'
    #[regex(r"'[0-9]{8,14}'")]
    #[regex(r"'[0-9]{4}-[0-9]{2}-[0-9]{2}'")]
    #[regex(r"'[0-9]{4}\.[0-9]{2}\.[0-9]{2}( [0-9]{2}:[0-9]{2}\.[0-9]{2})?'")]
    #[regex(r"'[0-9]{4},[0-9]{2},[0-9]{2}'")]
    Date,

    // Identifier: Unicode letters, digits, underscore
    // Must start with letter or underscore
    // Uses \p{L} to support all Unicode letters (Latin, Cyrillic, Greek, etc.)
    // Lower priority than keywords to ensure keywords are matched first
    #[regex(r"[_\p{L}][_\p{L}0-9]*", priority = 1)]
    Ident,

    // Line comment: // ...
    #[regex(r"//[^\n]*")]
    Comment,

    // Newline
    #[token("\n")]
    Newline,

    // Whitespace (spaces, tabs, carriage returns, NBSP)
    // Must have LOWEST priority to ensure it doesn't match identifiers or other tokens
    // NOTE: We must tokenize whitespace explicitly for Rowan's full-fidelity trees
    #[regex(r"[ \t\r\x{00A0}]+", priority = 0)]
    Whitespace,

    // UTF-8 BOM (Byte Order Mark) - treated as trivia
    // Common in BSL files exported from 1C platform
    #[token("\u{FEFF}")]
    Bom,

    // Error token for unrecognized input
    Error,
}

impl TokenKind {
    /// Returns true if this is a BSL keyword token.
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

/// A token with its kind, text, and position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The kind of token
    pub kind: TokenKind,
    /// The original text of the token
    pub text: SmolStr,
    /// The byte offset in the source code
    pub offset: usize,
}

/// Tokenizes BSL source code into a stream of tokens.
///
/// # Example
///
/// ```
/// use lexer::tokenize;
///
/// let tokens = tokenize("Процедура Тест() КонецПроцедуры");
/// // 7 tokens: КwProcedure, Whitespace, Ident, LParen, RParen, Whitespace, KwEndProcedure
/// assert_eq!(tokens.len(), 7);
/// ```
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
        // "Line1 -> StringStart
        // \n -> Newline
        // |Line2" -> StringTail
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
        assert_eq!(non_whitespace[1].kind, TokenKind::Ident); // "Клиент" is now Ident, checked by parser
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

        // With parameters
        let tokens = tokenize("&Вместо(\"Метод\")");
        assert_eq!(tokens[0].kind, TokenKind::AnnAround);
        assert_eq!(tokens[1].kind, TokenKind::LParen);
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

        // Verify we get expected tokens
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

        // Should have: KwFunction, Whitespace, Ident, LParen, KwVal, Whitespace, Ident, RParen
        assert_eq!(tokens[0].kind, TokenKind::KwFunction);
        assert_eq!(tokens[1].kind, TokenKind::Whitespace);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
        assert_eq!(tokens[2].text.as_str(), "ОпределитьСтавкуНДС");
    }

    #[test]
    fn test_string_with_paren_sdbl() {
        // Test that " (" is lexed as ONE String token, not three
        let input = r#"" (" + y"#;
        let tokens = tokenize(input);

        eprintln!("Tokens for {:?}:", input);
        for (i, tok) in tokens.iter().enumerate() {
            eprintln!("  [{}] {:?}: {:?}", i, tok.kind, tok.text);
        }

        // Should have: String(" ("), Whitespace, Plus, Whitespace, Ident(y)
        assert_eq!(tokens[0].kind, TokenKind::String, "First token should be String");
        assert_eq!(
            tokens[0].text.as_str(),
            r#"" (""#,
            "First string should be entire ' (' with quotes"
        );
    }

    #[test]
    fn test_bom() {
        // UTF-8 BOM at start of file
        let input = "\u{FEFF}Процедура Тест() КонецПроцедуры";
        let tokens = tokenize(input);

        eprintln!("Tokens with BOM:");
        for (i, tok) in tokens.iter().enumerate() {
            eprintln!("  [{}] {:?}: {:?}", i, tok.kind, tok.text);
        }

        // First token should be BOM
        assert_eq!(tokens[0].kind, TokenKind::Bom, "First token should be BOM");
        assert_eq!(tokens[0].text.as_str(), "\u{FEFF}");
        // Second token should be keyword
        assert_eq!(tokens[1].kind, TokenKind::KwProcedure);
    }
}
