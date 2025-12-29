//! Lexer for BSL (1C:Enterprise) language.
//!
//! This crate tokenizes BSL source code into a stream of tokens.
//! It supports both Russian and English keywords.

use logos::Logos;
use smol_str::SmolStr;

/// Token kinds for BSL language.
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[logos(skip r"[ \t\r]+")]
pub enum TokenKind {
    // Keywords (Russian)
    #[token("Процедура", ignore(ascii_case))]
    #[token("Procedure", ignore(ascii_case))]
    KwProcedure,

    #[token("КонецПроцедуры", ignore(ascii_case))]
    #[token("EndProcedure", ignore(ascii_case))]
    KwEndProcedure,

    #[token("Функция", ignore(ascii_case))]
    #[token("Function", ignore(ascii_case))]
    KwFunction,

    #[token("КонецФункции", ignore(ascii_case))]
    #[token("EndFunction", ignore(ascii_case))]
    KwEndFunction,

    #[token("Если", ignore(ascii_case))]
    #[token("If", ignore(ascii_case))]
    KwIf,

    #[token("Тогда", ignore(ascii_case))]
    #[token("Then", ignore(ascii_case))]
    KwThen,

    #[token("Иначе", ignore(ascii_case))]
    #[token("Else", ignore(ascii_case))]
    KwElse,

    #[token("ИначеЕсли", ignore(ascii_case))]
    #[token("ElsIf", ignore(ascii_case))]
    KwElsIf,

    #[token("КонецЕсли", ignore(ascii_case))]
    #[token("EndIf", ignore(ascii_case))]
    KwEndIf,

    #[token("Для", ignore(ascii_case))]
    #[token("For", ignore(ascii_case))]
    KwFor,

    #[token("Каждого", ignore(ascii_case))]
    #[token("Each", ignore(ascii_case))]
    KwEach,

    #[token("Из", ignore(ascii_case))]
    #[token("In", ignore(ascii_case))]
    KwIn,

    #[token("По", ignore(ascii_case))]
    #[token("To", ignore(ascii_case))]
    KwTo,

    #[token("Пока", ignore(ascii_case))]
    #[token("While", ignore(ascii_case))]
    KwWhile,

    #[token("Цикл", ignore(ascii_case))]
    #[token("Do", ignore(ascii_case))]
    KwDo,

    #[token("КонецЦикла", ignore(ascii_case))]
    #[token("EndDo", ignore(ascii_case))]
    KwEndDo,

    #[token("Возврат", ignore(ascii_case))]
    #[token("Return", ignore(ascii_case))]
    KwReturn,

    #[token("Перем", ignore(ascii_case))]
    #[token("Var", ignore(ascii_case))]
    KwVar,

    #[token("Попытка", ignore(ascii_case))]
    #[token("Try", ignore(ascii_case))]
    KwTry,

    #[token("Исключение", ignore(ascii_case))]
    #[token("Except", ignore(ascii_case))]
    KwExcept,

    #[token("КонецПопытки", ignore(ascii_case))]
    #[token("EndTry", ignore(ascii_case))]
    KwEndTry,

    #[token("ВызватьИсключение", ignore(ascii_case))]
    #[token("Raise", ignore(ascii_case))]
    KwRaise,

    #[token("Новый", ignore(ascii_case))]
    #[token("New", ignore(ascii_case))]
    KwNew,

    #[token("Экспорт", ignore(ascii_case))]
    #[token("Export", ignore(ascii_case))]
    KwExport,

    #[token("Знач", ignore(ascii_case))]
    #[token("Val", ignore(ascii_case))]
    KwVal,

    #[token("И", ignore(ascii_case))]
    #[token("And", ignore(ascii_case))]
    KwAnd,

    #[token("Или", ignore(ascii_case))]
    #[token("Or", ignore(ascii_case))]
    KwOr,

    #[token("Не", ignore(ascii_case))]
    #[token("Not", ignore(ascii_case))]
    KwNot,

    #[token("Истина", ignore(ascii_case))]
    #[token("True", ignore(ascii_case))]
    KwTrue,

    #[token("Ложь", ignore(ascii_case))]
    #[token("False", ignore(ascii_case))]
    KwFalse,

    #[token("Неопределено", ignore(ascii_case))]
    #[token("Undefined", ignore(ascii_case))]
    KwUndefined,

    #[token("Null", ignore(ascii_case))]
    KwNull,

    #[token("Прервать", ignore(ascii_case))]
    #[token("Break", ignore(ascii_case))]
    KwBreak,

    #[token("Продолжить", ignore(ascii_case))]
    #[token("Continue", ignore(ascii_case))]
    KwContinue,

    #[token("Перейти", ignore(ascii_case))]
    #[token("Goto", ignore(ascii_case))]
    KwGoto,

    #[token("НачатьТранзакцию", ignore(ascii_case))]
    #[token("BeginTransaction", ignore(ascii_case))]
    KwBeginTransaction,

    #[token("ЗафиксироватьТранзакцию", ignore(ascii_case))]
    #[token("CommitTransaction", ignore(ascii_case))]
    KwCommitTransaction,

    #[token("ОтменитьТранзакцию", ignore(ascii_case))]
    #[token("RollbackTransaction", ignore(ascii_case))]
    KwRollbackTransaction,

    // Preprocessor
    #[token("#Если", ignore(ascii_case))]
    #[token("#If", ignore(ascii_case))]
    PreIf,

    #[token("#Тогда", ignore(ascii_case))]
    #[token("#Then", ignore(ascii_case))]
    PreThen,

    #[token("#ИначеЕсли", ignore(ascii_case))]
    #[token("#ElsIf", ignore(ascii_case))]
    PreElsIf,

    #[token("#Иначе", ignore(ascii_case))]
    #[token("#Else", ignore(ascii_case))]
    PreElse,

    #[token("#КонецЕсли", ignore(ascii_case))]
    #[token("#EndIf", ignore(ascii_case))]
    PreEndIf,

    #[token("#Область", ignore(ascii_case))]
    #[token("#Region", ignore(ascii_case))]
    PreRegion,

    #[token("#КонецОбласти", ignore(ascii_case))]
    #[token("#EndRegion", ignore(ascii_case))]
    PreEndRegion,

    // Annotations
    #[token("&НаКлиенте", ignore(ascii_case))]
    #[token("&AtClient", ignore(ascii_case))]
    AnnAtClient,

    #[token("&НаСервере", ignore(ascii_case))]
    #[token("&AtServer", ignore(ascii_case))]
    AnnAtServer,

    #[token("&НаСервереБезКонтекста", ignore(ascii_case))]
    #[token("&AtServerNoContext", ignore(ascii_case))]
    AnnAtServerNoContext,

    #[token("&НаКлиентеНаСервереБезКонтекста", ignore(ascii_case))]
    #[token("&AtClientAtServerNoContext", ignore(ascii_case))]
    AnnAtClientAtServerNoContext,

    #[token("&НаКлиентеНаСервере", ignore(ascii_case))]
    #[token("&AtClientAtServer", ignore(ascii_case))]
    AnnAtClientAtServer,

    // Punctuation
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

    #[token("=")]
    Eq,

    #[token("<>")]
    Neq,

    #[token("<")]
    Lt,

    #[token("<=")]
    Le,

    #[token(">")]
    Gt,

    #[token(">=")]
    Ge,

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

    #[token("?")]
    Question,

    #[token("~")]
    Tilde,

    // Literals
    #[regex(r"[0-9]+(\.[0-9]+)?")]
    Number,

    #[regex(r#""([^"]|"")*""#)]
    String,

    #[regex(r"'[^']*'")]
    Date,

    // Identifiers
    #[regex(r"[_a-zA-Zа-яА-ЯёЁ][_a-zA-Zа-яА-ЯёЁ0-9]*")]
    Ident,

    // Comments
    #[regex(r"//[^\n]*")]
    Comment,

    // Newline
    #[token("\n")]
    Newline,

    // Label (for Goto)
    #[regex(r"~[_a-zA-Zа-яА-ЯёЁ][_a-zA-Zа-яА-ЯёЁ0-9]*:")]
    Label,

    // Error token for unrecognized input
    Error,
}

/// A token with its kind and text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub text: SmolStr,
    pub offset: usize,
}

/// Tokenizes BSL source code.
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
    fn test_keywords() {
        let tokens = tokenize("Процедура Тест() КонецПроцедуры");
        assert_eq!(tokens[0].kind, TokenKind::KwProcedure);
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[2].kind, TokenKind::LParen);
        assert_eq!(tokens[3].kind, TokenKind::RParen);
        assert_eq!(tokens[4].kind, TokenKind::KwEndProcedure);
    }

    #[test]
    fn test_english_keywords() {
        let tokens = tokenize("Procedure Test() EndProcedure");
        assert_eq!(tokens[0].kind, TokenKind::KwProcedure);
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[4].kind, TokenKind::KwEndProcedure);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize(r#""Hello, World!""#);
        assert_eq!(tokens[0].kind, TokenKind::String);
    }

    #[test]
    fn test_number() {
        let tokens = tokenize("123.456");
        assert_eq!(tokens[0].kind, TokenKind::Number);
    }

    #[test]
    fn test_comment() {
        let tokens = tokenize("// This is a comment\nПеременная");
        assert_eq!(tokens[0].kind, TokenKind::Comment);
        assert_eq!(tokens[1].kind, TokenKind::Newline);
        assert_eq!(tokens[2].kind, TokenKind::Ident);
    }

    #[test]
    fn test_preprocessor() {
        let tokens = tokenize("#Область Тест\n#КонецОбласти");
        assert_eq!(tokens[0].kind, TokenKind::PreRegion);
        assert_eq!(tokens[1].kind, TokenKind::Ident);
        assert_eq!(tokens[3].kind, TokenKind::PreEndRegion);
    }
}
