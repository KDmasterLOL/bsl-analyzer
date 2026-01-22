//! CanonicalSpellingKeywords diagnostic
//!
//! Checks that BSL keywords use canonical spelling (capitalized).
//!
//! ## Why?
//! Consistent keyword spelling improves code readability and maintainability.
//! BSL is case-insensitive, but canonical style uses capitalized keywords.
//!
//! ## Bad practice
//! ```bsl
//! функция Тест()
//!     если Истина тогда
//!         возврат 1;
//!     конецесли;
//!     ВОЗВРАТ 0;
//! конецфункции
//! ```
//!
//! ## Good practice
//! ```bsl
//! Функция Тест()
//!     Если Истина Тогда
//!         Возврат 1;
//!     КонецЕсли;
//!     Возврат 0;
//! КонецФункции
//! ```
//!
//! ## Configuration
//! - No parameters
//! - Can be disabled via config

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxToken};

/// Check if text contains Cyrillic characters
fn is_cyrillic(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{0400}'..='\u{04FF}'))
}

/// Check if a token is inside a preprocessor directive
fn is_in_preprocessor(token: &SyntaxToken) -> bool {
    let mut current = token.parent();
    while let Some(node) = current {
        if matches!(
            node.kind(),
            SyntaxKind::PRE_IF_DIR
                | SyntaxKind::PRE_ELSIF_CLAUSE
                | SyntaxKind::PRE_ELSE_CLAUSE
                | SyntaxKind::PRE_SYMBOL
        ) {
            return true;
        }
        current = node.parent();
    }
    false
}

/// Check preprocessor symbols (Сервер, Клиент, etc.)
fn check_preproc_symbol(actual: &str) -> Option<String> {
    let lower = actual.to_lowercase();
    match lower.as_str() {
        "сервер" | "server" => check_keyword(actual, &["Сервер", "Server"]),
        "клиент" | "client" => check_keyword(actual, &["Клиент", "Client"]),
        "вебклиент" | "webclient" => check_keyword(actual, &["ВебКлиент", "WebClient"]),
        "тонкийклиент" | "thinclient" => {
            check_keyword(actual, &["ТонкийКлиент", "ThinClient"])
        }
        "толстыйклиентобычноеприложение" | "thickclientordinaryapplication" => {
            check_keyword(
                actual,
                &["ТолстыйКлиентОбычноеПриложение", "ThickClientOrdinaryApplication"],
            )
        }
        "толстыйклиентуправляемоеприложение" | "thickclientmanagedapplication" => {
            check_keyword(
                actual,
                &["ТолстыйКлиентУправляемоеПриложение", "ThickClientManagedApplication"],
            )
        }
        "мобильноеприложениеклиент" | "mobileappclient" => {
            check_keyword(actual, &["МобильноеПриложениеКлиент", "MobileAppClient"])
        }
        "мобильноеприложениесервер" | "mobileappserver" => {
            check_keyword(actual, &["МобильноеПриложениеСервер", "MobileAppServer"])
        }
        "мобильныйклиент" | "mobileclient" => {
            check_keyword(actual, &["МобильныйКлиент", "MobileClient"])
        }
        "внешнеесоединение" | "externalconnection" => {
            check_keyword(actual, &["ВнешнееСоединение", "ExternalConnection"])
        }
        "наклиенте" | "atclient" => check_keyword(actual, &["НаКлиенте", "AtClient"]),
        "насервере" | "atserver" => check_keyword(actual, &["НаСервере", "AtServer"]),
        _ => None,
    }
}

/// Check if a keyword matches one of the canonical forms
///
/// Returns None if already canonical, otherwise returns suggested canonical form
fn check_keyword(actual: &str, canonical_forms: &[&str]) -> Option<String> {
    if canonical_forms.contains(&actual) {
        return None; // Already canonical
    }

    // Suggest first matching language canonical form
    // Prefer same script (Cyrillic vs Latin)
    let is_cyrillic_input = is_cyrillic(actual);
    canonical_forms
        .iter()
        .find(|&&form| is_cyrillic(form) == is_cyrillic_input)
        .or_else(|| canonical_forms.first())
        .map(|&s| s.to_string())
}

/// Check a single token for canonical spelling
fn check_token_canonical(token: &SyntaxToken) -> Option<String> {
    let actual = token.text();

    match token.kind() {
        // Procedure/Function keywords
        SyntaxKind::KW_PROCEDURE => check_keyword(actual, &["Процедура", "Procedure"]),
        SyntaxKind::KW_END_PROCEDURE => check_keyword(actual, &["КонецПроцедуры", "EndProcedure"]),
        SyntaxKind::KW_FUNCTION => check_keyword(actual, &["Функция", "Function"]),
        SyntaxKind::KW_END_FUNCTION => check_keyword(actual, &["КонецФункции", "EndFunction"]),
        SyntaxKind::KW_EXPORT => check_keyword(actual, &["Экспорт", "Export"]),
        SyntaxKind::KW_VAL => check_keyword(actual, &["Знач", "Val"]),

        // Control flow keywords
        SyntaxKind::KW_IF => check_keyword(actual, &["Если", "If"]),
        SyntaxKind::KW_THEN => check_keyword(actual, &["Тогда", "Then"]),
        SyntaxKind::KW_ELSIF => check_keyword(actual, &["ИначеЕсли", "ElsIf"]),
        SyntaxKind::KW_ELSE => check_keyword(actual, &["Иначе", "Else"]),
        SyntaxKind::KW_END_IF => check_keyword(actual, &["КонецЕсли", "EndIf"]),

        // Loop keywords
        SyntaxKind::KW_FOR => check_keyword(actual, &["Для", "For"]),
        SyntaxKind::KW_EACH => {
            // Special case: "каждого" and "each" (lowercase) are also canonical
            check_keyword(actual, &["Каждого", "каждого", "Each", "each"])
        }
        SyntaxKind::KW_IN => check_keyword(actual, &["Из", "In"]),
        SyntaxKind::KW_TO => check_keyword(actual, &["По", "To"]),
        SyntaxKind::KW_WHILE => check_keyword(actual, &["Пока", "While"]),
        SyntaxKind::KW_DO => check_keyword(actual, &["Цикл", "Do"]),
        SyntaxKind::KW_END_DO => check_keyword(actual, &["КонецЦикла", "EndDo"]),
        SyntaxKind::KW_RETURN => check_keyword(actual, &["Возврат", "Return"]),
        SyntaxKind::KW_CONTINUE => check_keyword(actual, &["Продолжить", "Continue"]),
        SyntaxKind::KW_BREAK => check_keyword(actual, &["Прервать", "Break"]),
        SyntaxKind::KW_GOTO => check_keyword(actual, &["Перейти", "Goto"]),

        // Exception handling
        SyntaxKind::KW_TRY => check_keyword(actual, &["Попытка", "Try"]),
        SyntaxKind::KW_EXCEPT => check_keyword(actual, &["Исключение", "Except"]),
        SyntaxKind::KW_END_TRY => check_keyword(actual, &["КонецПопытки", "EndTry"]),
        SyntaxKind::KW_RAISE => check_keyword(actual, &["ВызватьИсключение", "Raise"]),

        // Variable and value keywords
        SyntaxKind::KW_VAR => check_keyword(actual, &["Перем", "Var"]),
        SyntaxKind::KW_NEW => check_keyword(actual, &["Новый", "New"]),
        SyntaxKind::KW_EXECUTE => check_keyword(actual, &["Выполнить", "Execute"]),

        // Event handlers
        SyntaxKind::KW_ADD_HANDLER => check_keyword(actual, &["ДобавитьОбработчик", "AddHandler"]),
        SyntaxKind::KW_REMOVE_HANDLER => {
            check_keyword(actual, &["УдалитьОбработчик", "RemoveHandler"])
        }

        // Async/Await
        SyntaxKind::KW_ASYNC => check_keyword(actual, &["Асинх", "Async"]),
        SyntaxKind::KW_AWAIT => check_keyword(actual, &["Ждать", "Await"]),

        // Logical operators (special rules: multiple canonical forms)
        SyntaxKind::KW_AND => check_keyword(actual, &["И", "And", "AND"]),
        SyntaxKind::KW_OR => check_keyword(actual, &["Или", "ИЛИ", "Or", "OR"]),
        SyntaxKind::KW_NOT => check_keyword(actual, &["Не", "НЕ", "Not", "NOT"]),

        // Boolean literals
        SyntaxKind::KW_TRUE => check_keyword(actual, &["Истина", "True"]),
        SyntaxKind::KW_FALSE => check_keyword(actual, &["Ложь", "False"]),

        // Special values
        SyntaxKind::KW_UNDEFINED => check_keyword(actual, &["Неопределено", "Undefined"]),
        SyntaxKind::KW_NULL => check_keyword(actual, &["NULL", "Null"]),

        // Preprocessor directives
        SyntaxKind::PRE_IF => check_keyword(actual, &["#Если", "#If"]),
        SyntaxKind::PRE_ELSIF => check_keyword(actual, &["#ИначеЕсли", "#ElsIf"]),
        SyntaxKind::PRE_ELSE => check_keyword(actual, &["#Иначе", "#Else"]),
        SyntaxKind::PRE_END_IF => check_keyword(actual, &["#КонецЕсли", "#EndIf"]),
        SyntaxKind::PRE_REGION => check_keyword(actual, &["#Область", "#Region"]),
        SyntaxKind::PRE_END_REGION => check_keyword(actual, &["#КонецОбласти", "#EndRegion"]),
        SyntaxKind::PRE_USE => check_keyword(actual, &["#Использовать", "#Use"]),
        SyntaxKind::PRE_INSERT => check_keyword(actual, &["#Вставить", "#Insert"]),

        // Annotations
        SyntaxKind::ANN_AT_CLIENT => check_keyword(actual, &["&НаКлиенте", "&AtClient"]),
        SyntaxKind::ANN_AT_SERVER => check_keyword(actual, &["&НаСервере", "&AtServer"]),
        SyntaxKind::ANN_AT_SERVER_NO_CONTEXT => {
            check_keyword(actual, &["&НаСервереБезКонтекста", "&AtServerNoContext"])
        }
        SyntaxKind::ANN_AT_CLIENT_AT_SERVER => {
            check_keyword(actual, &["&НаКлиентеНаСервере", "&AtClientAtServer"])
        }
        SyntaxKind::ANN_AT_CLIENT_AT_SERVER_NO_CONTEXT => check_keyword(
            actual,
            &["&НаКлиентеНаСервереБезКонтекста", "&AtClientAtServerNoContext"],
        ),

        // Preprocessor symbols (IDENT tokens inside preprocessor)
        SyntaxKind::IDENT if is_in_preprocessor(token) => check_preproc_symbol(actual),

        _ => None,
    }
}

/// Main entry point for CanonicalSpellingKeywords diagnostic
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CanonicalSpellingKeywords;
    // Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    // Traverse all tokens (including those in composite nodes)
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            // Skip trivia (whitespace, comments, newlines)
            if matches!(
                token.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT
            ) {
                continue;
            }

            // Check canonical spelling
            if let Some(canonical) = check_token_canonical(&token) {
                let range = token.text_range();
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::CanonicalSpellingKeywords,
                    message: format!(
                        "Ключевое слово '{}' написано не канонически, используйте '{}'",
                        token.text(),
                        canonical
                    ),
                    severity: ctx.severity(code),
                    range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic_with_config};
    use crate::DiagnosticsConfig;

    #[test]
    fn test_canonical_keywords() {
        let code = r#"Процедура Тест()
    Если Истина Тогда
        Возврат 1;
    КонецЕсли;
    Возврат 0;
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // All keywords are canonical, should NOT detect
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_lowercase_keywords() {
        let code = r#"функция Тест()
    возврат 0;
конецфункции"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect 3 non-canonical keywords
        assert!(diagnostics.len() >= 3);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CanonicalSpellingKeywords);
    }

    #[test]
    fn test_uppercase_keywords() {
        let code = r#"ФУНКЦИЯ Тест()
    ВОЗВРАТ 0;
КОНЕЦФУНКЦИИ"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect 3 non-canonical keywords
        assert!(diagnostics.len() >= 3);
    }

    #[test]
    fn test_mixed_case_keywords() {
        let code = r#"ЕслИ Истина ТогдА
    Результат = 1;
ИнаЧе
    Результат = 0;
КонецЕсЛи;"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect: ЕслИ, ТогдА, ИнаЧе, КонецЕсЛи
        assert!(diagnostics.len() >= 4);
    }

    #[test]
    fn test_logical_operators() {
        let code = r#"Процедура Тест()
    // Canonical forms
    А = Истина И Ложь;
    Б = Истина And Ложь;
    В = Истина AND Ложь;
    Г = Истина Or Ложь;
    Д = Истина OR Ложь;
    Е = Истина Not Ложь;
    Ж = Истина NOT Ложь;

    // Non-canonical forms
    З = Истина и Ложь;
    И = Истина and Ложь;
    К = Истина or Ложь;
    Л = Истина not Ложь;
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Should detect only non-canonical lowercase forms
        // Note: lexer may not recognize mixed-case as keywords
        assert!(!diagnostics.is_empty(), "Should detect lowercase logical operators");
    }

    #[test]
    fn test_each_keyword_lowercase() {
        let code = r#"Процедура Тест()
    // Lowercase "каждого" is canonical after "Для"
    Для Каждого Элемент Из Массив Цикл
        Сообщить(Элемент);
    КонецЦикла;

    Для каждого Элемент Из Массив Цикл
        Сообщить(Элемент);
    КонецЦикла;
КонецПроцедуры"#;

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Both "Каждого" and "каждого" are canonical, should NOT detect
        // Only other non-canonical would be detected
        for diag in &diagnostics {
            assert!(!diag.message.contains("Каждого"));
            assert!(!diag.message.contains("каждого"));
        }
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/CanonicalSpellingKeywordsDiagnostic.bsl");

        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // CRITICAL: Must match Java implementation (127 diagnostics)
        assert_eq!(
            diagnostics.len(),
            127,
            "Must match Java implementation exactly (127 diagnostics)"
        );

        // Verify exact positions (spot-check key diagnostics)
        // Using Java test line/column numbers directly (0-based)

        // ПерЕМ
        assert_diagnostic_range(code, &diagnostics[0], 8, 4, 9);
        // НЕОПРЕделено
        assert_diagnostic_range(code, &diagnostics[1], 15, 8, 20);
        // НоВый
        assert_diagnostic_range(code, &diagnostics[2], 16, 8, 13);
        // ЕслИ
        assert_diagnostic_range(code, &diagnostics[3], 35, 4, 8);
        // ТогдА
        assert_diagnostic_range(code, &diagnostics[4], 35, 20, 25);
        // ИначеЕСли
        assert_diagnostic_range(code, &diagnostics[5], 37, 4, 13);
        // ИнаЧе
        assert_diagnostic_range(code, &diagnostics[6], 41, 4, 9);
        // КонецЕсЛи
        assert_diagnostic_range(code, &diagnostics[7], 43, 4, 13);
        // ДЛЯ
        assert_diagnostic_range(code, &diagnostics[8], 73, 4, 7);
        // КАЖДОГО
        assert_diagnostic_range(code, &diagnostics[9], 73, 8, 15);
        // ИЗ
        assert_diagnostic_range(code, &diagnostics[10], 73, 29, 31);
        // ЦикЛ
        assert_diagnostic_range(code, &diagnostics[11], 73, 34, 38);
        // ПРервать
        assert_diagnostic_range(code, &diagnostics[12], 74, 7, 15);
        // ПРодолжить
        assert_diagnostic_range(code, &diagnostics[13], 75, 7, 17);
        // КонецЦиклА
        assert_diagnostic_range(code, &diagnostics[14], 76, 4, 14);
    }
}
