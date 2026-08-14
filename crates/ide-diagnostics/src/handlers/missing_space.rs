use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;
use std::collections::HashSet;
use syntax::{SyntaxKind, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_LIST_FOR_CHECK_LEFT: &str = "";
const DEFAULT_LIST_FOR_CHECK_RIGHT: &str = ", ;";
const DEFAULT_LIST_FOR_CHECK_LEFT_AND_RIGHT: &str = "+ - * / = % < > <> <= >=";
const DEFAULT_CHECK_SPACE_TO_RIGHT_OF_UNARY: bool = false;
const DEFAULT_ALLOW_MULTIPLE_COMMAS: bool = false;

const UNARY_CONTEXT_TOKENS: &[SyntaxKind] = &[
    SyntaxKind::PLUS,
    SyntaxKind::MINUS,
    SyntaxKind::STAR,
    SyntaxKind::SLASH,
    SyntaxKind::EQ,
    SyntaxKind::PERCENT,
    SyntaxKind::LT,
    SyntaxKind::GT,
    SyntaxKind::L_PAREN,
    SyntaxKind::L_BRACKET,
    SyntaxKind::COMMA,
    SyntaxKind::KW_RETURN,
    SyntaxKind::NEQ,
    SyntaxKind::LE,
    SyntaxKind::GE,
];

#[derive(Debug, Clone)]
struct Config {
    left_symbols: HashSet<String>,
    right_symbols: HashSet<String>,
    left_right_symbols: HashSet<String>,
    check_space_to_right_of_unary: bool,
    allow_multiple_commas: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::MissingSpace;

        let left_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckLeft")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_LEFT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let right_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckRight")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_RIGHT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let left_right_symbols: HashSet<String> = ctx
            .config
            .get_string(code, "listForCheckLeftAndRight")
            .unwrap_or(DEFAULT_LIST_FOR_CHECK_LEFT_AND_RIGHT)
            .split_whitespace()
            .map(String::from)
            .collect();

        let check_space_to_right_of_unary = ctx
            .config
            .get_bool(code, "checkSpaceToRightOfUnary")
            .unwrap_or(DEFAULT_CHECK_SPACE_TO_RIGHT_OF_UNARY);

        let allow_multiple_commas = ctx
            .config
            .get_bool(code, "allowMultipleCommas")
            .unwrap_or(DEFAULT_ALLOW_MULTIPLE_COMMAS);

        tracing::debug!(
            left_count = left_symbols.len(),
            right_count = right_symbols.len(),
            left_right_count = left_right_symbols.len(),
            check_unary = check_space_to_right_of_unary,
            allow_commas = allow_multiple_commas,
            "MissingSpace config loaded"
        );

        Self {
            left_symbols,
            right_symbols,
            left_right_symbols,
            check_space_to_right_of_unary,
            allow_multiple_commas,
        }
    }
}

fn is_trivia(token: &SyntaxToken) -> bool {
    token.kind().is_trivia()
}

fn is_keyword_with_left_right_space(token: &SyntaxToken) -> bool {
    matches!(
        token.kind(),
        SyntaxKind::KW_OR | SyntaxKind::KW_AND | SyntaxKind::KW_IN | SyntaxKind::KW_TO
    )
}

fn is_keyword_with_left_space(token: &SyntaxToken) -> bool {
    matches!(token.kind(), SyntaxKind::KW_EXPORT | SyntaxKind::KW_THEN | SyntaxKind::KW_DO)
}

fn is_keyword_with_right_space(token: &SyntaxToken) -> bool {
    matches!(
        token.kind(),
        SyntaxKind::KW_IF
            | SyntaxKind::KW_ELSIF
            | SyntaxKind::KW_WHILE
            | SyntaxKind::KW_FOR
            | SyntaxKind::KW_NOT
            | SyntaxKind::KW_EACH
    )
}

fn is_unary_operator(tokens: &[SyntaxToken], current_index: usize) -> bool {
    let mut prev_index = current_index;
    loop {
        if prev_index == 0 {
            return true;
        }
        prev_index -= 1;

        if !is_trivia(&tokens[prev_index]) {
            return UNARY_CONTEXT_TOKENS.contains(&tokens[prev_index].kind());
        }
    }
}

fn should_check_left(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.left_symbols.contains(text) || is_keyword_with_left_space(token)
}

fn should_check_right(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.right_symbols.contains(text) || is_keyword_with_right_space(token)
}

fn should_check_left_right(token: &SyntaxToken, config: &Config) -> bool {
    let text = token.text();
    config.left_right_symbols.contains(text) || is_keyword_with_left_right_space(token)
}

fn check_left_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    _config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if index == 0 {
        return None;
    }

    let mut prev_index = index - 1;
    loop {
        if !is_trivia(&tokens[prev_index]) {
            break;
        }
        if prev_index == 0 {
            return None;
        }
        prev_index -= 1;
    }

    let prev_token = &tokens[prev_index];

    if prev_token.kind() == SyntaxKind::L_PAREN {
        return None;
    }

    if index > 0 && is_trivia(&tokens[index - 1]) {
        return None;
    }

    let range = token.text_range();
    let insert = range.start();
    Some(Diagnostic {
        code,
        message: format!("Отсутствует пробел слева от '{}'", token.text()),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix::safe(
            format!("Добавить пробел слева от '{}'", token.text()),
            vec![TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() }],
        )],
    })
}

fn check_right_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if !config.check_space_to_right_of_unary
        && matches!(token.kind(), SyntaxKind::PLUS | SyntaxKind::MINUS)
        && is_unary_operator(tokens, index)
    {
        return None;
    }

    if index + 1 >= tokens.len() {
        return None;
    }

    let mut next_index = index + 1;
    loop {
        if next_index >= tokens.len() {
            return None;
        }
        if !is_trivia(&tokens[next_index]) {
            break;
        }
        next_index += 1;
    }

    let next_token = &tokens[next_index];

    if config.allow_multiple_commas
        && token.kind() == SyntaxKind::COMMA
        && next_token.kind() == SyntaxKind::COMMA
    {
        return None;
    }

    if index + 1 < tokens.len() && is_trivia(&tokens[index + 1]) {
        return None;
    }

    let range = token.text_range();
    let insert = range.end();
    Some(Diagnostic {
        code,
        message: format!("Отсутствует пробел справа от '{}'", token.text()),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix::safe(
            format!("Добавить пробел справа от '{}'", token.text()),
            vec![TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() }],
        )],
    })
}

fn check_left_right_space(
    tokens: &[SyntaxToken],
    index: usize,
    token: &SyntaxToken,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let missing_left = check_left_space(tokens, index, token, config, code, ctx).is_some();
    let missing_right = check_right_space(tokens, index, token, config, code, ctx).is_some();

    if !missing_left && !missing_right {
        return None;
    }

    let message = if missing_left && missing_right {
        format!("Отсутствует пробел слева и справа от '{}'", token.text())
    } else if missing_left {
        format!("Отсутствует пробел слева от '{}'", token.text())
    } else {
        format!("Отсутствует пробел справа от '{}'", token.text())
    };

    let range = token.text_range();
    let mut edits = Vec::new();
    if missing_left {
        let insert = range.start();
        edits.push(TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() });
    }
    if missing_right {
        let insert = range.end();
        edits.push(TextEdit { range: TextRange::new(insert, insert), new_text: " ".to_string() });
    }

    let label = if missing_left && missing_right {
        format!("Добавить пробелы вокруг '{}'", token.text())
    } else if missing_left {
        format!("Добавить пробел слева от '{}'", token.text())
    } else {
        format!("Добавить пробел справа от '{}'", token.text())
    };

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![Fix::safe(label, edits)],
    })
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("MissingSpace::check").entered();
    let code = DiagnosticCode::MissingSpace;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let tokens: Vec<_> = root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

    let mut diagnostics = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if is_trivia(token) {
            continue;
        }

        if should_check_left(token, &config) {
            if let Some(diag) = check_left_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }

        if should_check_right(token, &config) {
            if let Some(diag) = check_right_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }

        if should_check_left_right(token, &config) {
            if let Some(diag) = check_left_right_space(&tokens, index, token, &config, code, ctx) {
                diagnostics.push(diag);
            }
        }
    }

    tracing::debug!(count = diagnostics.len(), "MissingSpace diagnostics found");

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic_with_config;
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_comprehensive() {
        let code = "\n// Комментарий,комментарий  \t\t// 0\n\nПроцедура Метод1(Парам1, Парам2)\t// 0\n\tПерем А1,Б1;А1=Б1+ 1;\t\t\t// 4\n\n\t// Рез1=Парам1+Парам2;     \t\t// 0\n\tРез1=Парам1+Парам2;     \t\t// 2\n\tРез1=Парам1- Парам2;    \t\t// 2\n\tРез1=Парам1 + Парам2;   \t\t// 1\n\tРез1 =Парам1* Парам2;   \t\t// 2\n\tРез1 =Парам1 /Парам2;   \t\t// 2\n\tРез1 = Парам1 + Парам2; \t\t// 0\n\tРез1= Парам1% Парам2;   \t\t// 2\n\nКонецПроцедуры\n\nПроцедура Метод2(А,Б, В, Г) \t\t// 1\n\tРез = А> Б;\t\t\t\t\t// 1\n\tРез = А <Б;\t\t\t\t\t// 1\n\tРез = А > Б;\t\t\t\t\t// 0\n\n\tРез = А>= Б;\t\t\t\t\t// 1\n\tРез = А <=Б;\t\t\t\t\t// 1\n\tРез = А <> Б;\t\t\t\t\t// 0\n\tРез = А<>Б;\t\t\t\t\t// 1\n\n    Рез = -А;                       // 1\n    Рез = - Б;                      // 0\nКонецПроцедуры\nПроцедура Тест()\nМетод1(60,24);Метод2(24, 60,,60);   // 3\n\nМетод2(60, 60, 60);    \t\t\t\t// 0\nМетод1(24,        \t\t\t\t\t// 0\n\t\t60,\n\t\t60);\n// Ошибка маскируется из-за комментария в строке\nРез=А+Б;\n\n// Тут не должно быть ошибок\nРаботаСКурсамиВалют.СформироватьСуммуПрописью(-ОстатокНаКонец, Документ.ВалютаДокумента);\n\nПредставление =\" \" +СокрЛ(Объект); // после равно и после плюса\n\nСообщить(Представление);\nКонецПроцедуры\n\nПроцедура Тест2()Экспорт            // 1\n\nДЛЯ(ит = 1)по(7)Цикл                // 3\nКонецЦикла;\n\nДля каждого(поле)из(коллекция)Цикл  // 3\nКонецЦикла;\n\nЕсли(ИСТИНА)ИЛИ(ЛОЖЬ)И(ИСТИНА)Тогда // 4\nИначеЕсли(ИСТИНА)ИЛИ(НЕ(ЛОЖЬ))Тогда // 4\nКонецЕсли;\n\nКонецПроцедуры";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 44, "Should find 44 diagnostics");
    }

    #[test]
    fn test_unary_operators_default() {
        let code = r"
Процедура Тест()
    Рез = -А;
    Рез = А - Б;
    Рез = -(-А);
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 0, "Unary operators should not trigger errors by default");
    }

    #[test]
    fn test_unary_operators_with_check_enabled() {
        let code = r"
Процедура Тест()
    Рез = -А;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "checkSpaceToRightOfUnary": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(
            diagnostics.len(),
            1,
            "Should detect missing space after unary minus when enabled"
        );
    }

    #[test]
    fn test_left_paren_exception() {
        let code = r"
Процедура Тест()
    Метод(А, Б);
    Если(условие)Тогда
    КонецЕсли;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(!diagnostics.is_empty(), "Should detect some spacing errors");
    }

    #[test]
    fn test_allow_multiple_commas_false() {
        let code = r"
Процедура Тест()
    Метод(60,,24);
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(!diagnostics.is_empty(), "Should detect missing space between commas by default");
    }

    #[test]
    fn test_allow_multiple_commas_true() {
        let code = r"
Процедура Тест()
    Метод(60,,24);
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "allowMultipleCommas": true
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(diagnostics.len() < 10, "Should have fewer errors with allowMultipleCommas");
    }

    #[test]
    fn test_keyword_spacing() {
        let code = r"
Процедура Тест()Экспорт
    Если(ИСТИНА)Тогда
    КонецЕсли;

    Для(каждого А)Цикл
    КонецЦикла;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(!diagnostics.is_empty(), "Should detect keyword spacing errors");
    }

    #[test]
    fn test_operator_spacing() {
        let code = r"
Процедура Тест()
    А=Б+1;
    Рез=Парам1+Парам2;
    Рез=Парам1- Парам2;
КонецПроцедуры
        ";
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(!diagnostics.is_empty(), "Should detect operator spacing errors");
    }

    #[test]
    fn test_custom_symbols_left_right() {
        let code = r"
Процедура Тест()
    А=Б;
    Х+У;
КонецПроцедуры
        ";
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingSpace,
            serde_json::json!({
                "listForCheckLeftAndRight": "= +"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert!(!diagnostics.is_empty(), "Should detect custom symbol spacing");
    }
}
