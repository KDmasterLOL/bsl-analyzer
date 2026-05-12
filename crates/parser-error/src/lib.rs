use lexer::TokenKind;
use smallvec::SmallVec;
use text_size::TextRange;

pub type ParseErrorRange = TextRange;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecoveryKind {
    BumpToken,
    MissingToken,
    RecoverySpan,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParseError {
    Expected {
        expected: SmallVec<[TokenKind; 2]>,
        found: Option<TokenKind>,
        recovery: RecoveryKind,
    },
    Unexpected {
        found: Option<TokenKind>,
        recovery: RecoveryKind,
    },
    Custom {
        message: &'static str,
        recovery: RecoveryKind,
    },
}

impl ParseError {
    pub fn recovery(&self) -> RecoveryKind {
        match self {
            ParseError::Expected { recovery, .. }
            | ParseError::Unexpected { recovery, .. }
            | ParseError::Custom { recovery, .. } => *recovery,
        }
    }

    pub fn format_ru(&self) -> String {
        match self {
            ParseError::Expected { expected, found, recovery: _ } if expected.len() == 1 => {
                format!(
                    "Ожидалось '{}', встречено {}",
                    token_display_ru(expected[0]),
                    found_display_ru(*found)
                )
            }
            ParseError::Expected { expected, found, recovery: _ } => {
                let expected =
                    expected.iter().copied().map(token_display_ru).collect::<Vec<_>>().join(", ");
                format!("Ожидалось одно из: {expected}; встречено: {}", found_display_ru(*found))
            }
            ParseError::Unexpected { found: Some(found), recovery: _ } => {
                format!("Неожиданный токен '{}'", token_display_ru(*found))
            }
            ParseError::Unexpected { found: None, recovery: _ } => {
                "Неожиданный конец файла".to_owned()
            }
            ParseError::Custom { message, recovery: _ } => capitalize_first(message),
        }
    }
}

fn found_display_ru(found: Option<TokenKind>) -> &'static str {
    found.map_or("конец файла", token_display_ru)
}

fn capitalize_first(message: &str) -> String {
    let mut chars = message.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };

    first.to_uppercase().chain(chars).collect()
}

fn token_display_ru(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::KwProcedure => "Процедура",
        TokenKind::KwEndProcedure => "КонецПроцедуры",
        TokenKind::KwFunction => "Функция",
        TokenKind::KwEndFunction => "КонецФункции",
        TokenKind::KwExport => "Экспорт",
        TokenKind::KwVal => "Знач",
        TokenKind::KwIf => "Если",
        TokenKind::KwThen => "Тогда",
        TokenKind::KwElsIf => "ИначеЕсли",
        TokenKind::KwElse => "Иначе",
        TokenKind::KwEndIf => "КонецЕсли",
        TokenKind::KwFor => "Для",
        TokenKind::KwEach => "Каждого",
        TokenKind::KwIn => "Из",
        TokenKind::KwTo => "По",
        TokenKind::KwWhile => "Пока",
        TokenKind::KwDo => "Цикл",
        TokenKind::KwEndDo => "КонецЦикла",
        TokenKind::KwReturn => "Возврат",
        TokenKind::KwContinue => "Продолжить",
        TokenKind::KwBreak => "Прервать",
        TokenKind::KwGoto => "Перейти",
        TokenKind::KwTry => "Попытка",
        TokenKind::KwExcept => "Исключение",
        TokenKind::KwEndTry => "КонецПопытки",
        TokenKind::KwRaise => "ВызватьИсключение",
        TokenKind::KwVar => "Перем",
        TokenKind::KwNew => "Новый",
        TokenKind::KwExecute => "Выполнить",
        TokenKind::KwAddHandler => "ДобавитьОбработчик",
        TokenKind::KwRemoveHandler => "УдалитьОбработчик",
        TokenKind::KwAsync => "Асинх",
        TokenKind::KwAwait => "Ждать",
        TokenKind::KwAnd => "И",
        TokenKind::KwOr => "Или",
        TokenKind::KwNot => "Не",
        TokenKind::KwTrue => "Истина",
        TokenKind::KwFalse => "Ложь",
        TokenKind::KwUndefined => "Неопределено",
        TokenKind::KwNull => "Null",
        TokenKind::PreIf => "#Если",
        TokenKind::PreElsIf => "#ИначеЕсли",
        TokenKind::PreElse => "#Иначе",
        TokenKind::PreEndIf => "#КонецЕсли",
        TokenKind::PreRegion => "#Область",
        TokenKind::PreEndRegion => "#КонецОбласти",
        TokenKind::PreUse => "#Использовать",
        TokenKind::PreInsert => "#Вставка",
        TokenKind::PreEndInsert => "#КонецВставки",
        TokenKind::PreDelete => "#Удаление",
        TokenKind::PreEndDelete => "#КонецУдаления",
        TokenKind::Eq => "=",
        TokenKind::Neq => "<>",
        TokenKind::Le => "<=",
        TokenKind::Lt => "<",
        TokenKind::Ge => ">=",
        TokenKind::Gt => ">",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Slash => "/",
        TokenKind::Percent => "%",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::Dot => ".",
        TokenKind::Comma => ",",
        TokenKind::Semicolon => ";",
        TokenKind::Colon => ":",
        TokenKind::Question => "?",
        TokenKind::Tilde => "~",
        TokenKind::Bar => "|",
        TokenKind::Hash => "#",
        TokenKind::Ampersand => "&",
        TokenKind::Exclamation => "!",
        TokenKind::Ident => "идентификатор",
        TokenKind::String => "строка",
        TokenKind::StringStart => "начало строки",
        TokenKind::StringTail => "окончание строки",
        TokenKind::StringPart => "часть строки",
        TokenKind::Decimal => "число",
        TokenKind::Float => "число",
        TokenKind::Date => "дата",
        TokenKind::Newline => "перевод строки",
        TokenKind::Whitespace => "пробел",
        TokenKind::Comment => "комментарий",
        TokenKind::Bom => "BOM",
        TokenKind::Error => "ошибка",
        _ => "токен",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_single_token_found_contains_ru_token_names() {
        let err = ParseError::Expected {
            expected: SmallVec::from_slice(&[TokenKind::KwThen]),
            found: Some(TokenKind::KwElse),
            recovery: RecoveryKind::MissingToken,
        };

        let message = err.format_ru();

        assert!(message.contains("Тогда"), "{message}");
        assert!(message.contains("Иначе"), "{message}");
    }

    #[test]
    fn expected_multi_token_found_eof_contains_choices_and_eof() {
        let err = ParseError::Expected {
            expected: SmallVec::from_slice(&[TokenKind::KwThen, TokenKind::KwDo]),
            found: None,
            recovery: RecoveryKind::RecoverySpan,
        };

        let message = err.format_ru();

        assert!(message.contains("одно из"), "{message}");
        assert!(message.contains("конец файла"), "{message}");
    }

    #[test]
    fn unexpected_some_contains_ru_token_name() {
        let err = ParseError::Unexpected {
            found: Some(TokenKind::KwEndIf),
            recovery: RecoveryKind::BumpToken,
        };

        let message = err.format_ru();

        assert!(message.contains("Неожиданный"), "{message}");
        assert!(message.contains("КонецЕсли"), "{message}");
    }

    #[test]
    fn unexpected_none_contains_eof() {
        let err = ParseError::Unexpected { found: None, recovery: RecoveryKind::RecoverySpan };

        let message = err.format_ru();

        assert!(message.contains("конец файла"), "{message}");
    }

    #[test]
    fn custom_message_starts_with_uppercase_russian_letter() {
        let err = ParseError::Custom {
            message: "ошибка разбора", recovery: RecoveryKind::Custom
        };

        let message = err.format_ru();

        assert!(message.starts_with('О'), "{message}");
    }
}
