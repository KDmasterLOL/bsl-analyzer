//! Reports hardcoded absolute file-system paths in string literals.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use regex::Regex;
use syntax::{SyntaxKind, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_SEARCH_WORDS_STD_PATHS_UNIX: &str =
    r"bin|boot|dev|etc|home|lib|lost\+found|misc|mnt|media|opt|proc|root|run|sbin|tmp|usr|var";

fn is_hardcode_path(content: &str) -> bool {
    let bytes = content.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    let first_char = bytes[0];
    let has_separator = content.contains('\\') || content.contains('/');

    match first_char {
        b'/' => has_separator && content.len() > 1,
        b'~' => has_separator,
        b'%' => {
            if let Some(end_percent) = content[1..].find('%') {
                let after_var = &content[end_percent + 2..];
                after_var.starts_with('\\') || after_var.starts_with('/')
            } else {
                false
            }
        }
        b'\\' => bytes.len() > 1 && (bytes[1] == b'\\' || bytes[1] == b'/'),
        c if c.is_ascii_alphabetic() => {
            bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
        }
        _ => false,
    }
}

#[derive(Debug, Clone)]
struct Config {
    search_words_std_paths_unix: Regex,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let std_paths_str = ctx
            .config
            .get_string(DiagnosticCode::UsingHardcodePath, "searchWordsStdPathsUnix")
            .unwrap_or(DEFAULT_SEARCH_WORDS_STD_PATHS_UNIX);

        let search_words_std_paths_unix = Regex::new(&format!("(?i)^/({})(/|$)", std_paths_str))
            .unwrap_or_else(|_| {
                Regex::new(&format!("(?i)^/({})(/|$)", DEFAULT_SEARCH_WORDS_STD_PATHS_UNIX))
                    .unwrap()
            });

        Self { search_words_std_paths_unix }
    }
}

fn extract_string_content(text: &str) -> Option<String> {
    if text.len() < 3 {
        return None;
    }
    Some(text[1..text.len() - 1].to_string())
}

fn is_url(content: &str) -> bool {
    let bytes = content.as_bytes();
    let after_scheme = if bytes.len() > 8 && bytes[..8].eq_ignore_ascii_case(b"https://") {
        Some(8)
    } else if bytes.len() > 7 && bytes[..7].eq_ignore_ascii_case(b"http://") {
        Some(7)
    } else if bytes.len() > 6 && bytes[..6].eq_ignore_ascii_case(b"ftp://") {
        Some(6)
    } else {
        None
    };
    after_scheme.is_some_and(|pos| bytes.get(pos).is_some_and(|&b| b != b' '))
}

/// Single-pass token handler for UsingHardcodePath diagnostic.
#[inline]
pub fn check_token(token: &SyntaxToken, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::UsingHardcodePath;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    if token.kind() != SyntaxKind::STRING {
        return;
    }

    let text = token.text();
    if text.len() <= 4 {
        return;
    }

    let Some(content) = extract_string_content(text) else {
        return;
    };

    if is_url(&content) {
        return;
    }

    if !is_hardcode_path(&content) {
        return;
    }

    if content.starts_with('/') {
        let config = Config::from_context(ctx);
        if !config.search_words_std_paths_unix.is_match(&content) {
            return;
        }
    }

    acc.push(Diagnostic {
        code,
        message: "Используется хранение в коде пути к файлу".to_string(),
        severity: ctx.severity(code),
        range: token.text_range(),
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

/// Legacy check function (delegates to single-pass).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            check_token(&token, &mut diagnostics, ctx);
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{
        assert_diagnostic_range, check_ast_diagnostic, check_ast_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_comprehensive() {
        let code = r#"// Пути к файлам и каталогам
//////////////////////////////////////////

Массив = Новый Массив;
// windows
Массив.Добавить("\\test\test\TEST.xls"); // <-- ошибка
Массив.Добавить("\\server\share\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("\\server\share\myfile.txt"); // <-- ошибка
Массив.Добавить("\\123.123.123.123\share\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("c:\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("c:\folder\myfileWithoutExtension"); // <-- ошибка
Массив.Добавить("c:\folder"); // <-- ошибка
Массив.Добавить("c:/folder"); // <-- ошибка
Массив.Добавить("c://folder"); // <-- ошибка
Массив.Добавить("//test/test/TEST.xls"); // <-- ошибка
Массив.Добавить("C:\Program Files (x86)\"); // <-- ошибка
Массив.Добавить("F:\DATA\Develop\Module.bsl"); // <-- ошибка
// unix
Массив.Добавить("/dev/test"); // <-- ошибка
Массив.Добавить("/dev/test/test.bsl"); // <-- ошибка
Массив.Добавить("/home/root/test.bsl"); // <-- ошибка
Массив.Добавить("/etc/php-fpm/php.ini"); // <-- ошибка
Массив.Добавить("/less/test");

// не сработает, там нет путей
ПутьОтносительный = "./catalog"; // не должно срабатывать
Путь = "/catalog"; // исключаем, т.к. может использовано для URL и будет FP
ПутьКФайлу = "d:тутНетПутиКФайлу";
ПутьКоВторомуФайлу = "тут/нетпутикфайлу";
Структура = Новый Структура("Путь", "Съешь ещё этих мягких французских булок, да выпей же чаю.");

// Снова проверки путей
ПутьWin = "~/temp/file.txt"; // <-- ошибка
ПутьДляСомневающихся = "C:\temp/обратный слеш\тоже/работает"; // <-- ошибка
НеПуть = "!/bin/sh";

// Тут невозможно отличить fp с доменным путем от обычного сетевого пути в windows.
ДоменныйПуть = "\\domain\user"; // <-- ошибка

ПутьОтКаталогаЮзера = "%UserProfile%/что/то/там"; // <-- ошибка

ТипыНастроек.Добавить("/CurrentVarianKey");

КомандаЗапуска.Добавить("/LoadExternalDataProcessorOrReportFromFile");

Разрешение = ФабрикаXDTO.Создать(
	ФабрикаXDTO.Тип(
		"http://www.1c.ru/1cFresh/ApplicationExtensions/Permissions/1.0.0.1",
		РазрешениеВДанных.ВидРазрешения
	)
);
"#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 19, "Expected 19 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 5, 16, 38);
        assert_diagnostic_range(code, &diagnostics[1], 6, 16, 50);
        assert_diagnostic_range(code, &diagnostics[2], 7, 16, 43);
        assert_diagnostic_range(code, &diagnostics[3], 8, 16, 59);
        assert_diagnostic_range(code, &diagnostics[4], 9, 16, 38);
        assert_diagnostic_range(code, &diagnostics[5], 10, 16, 50);
        assert_diagnostic_range(code, &diagnostics[6], 11, 16, 27);
        assert_diagnostic_range(code, &diagnostics[7], 12, 16, 27);
        assert_diagnostic_range(code, &diagnostics[8], 13, 16, 28);
        assert_diagnostic_range(code, &diagnostics[9], 15, 16, 41);
        assert_diagnostic_range(code, &diagnostics[10], 16, 16, 44);
        assert_diagnostic_range(code, &diagnostics[11], 18, 16, 27);
        assert_diagnostic_range(code, &diagnostics[12], 19, 16, 36);
        assert_diagnostic_range(code, &diagnostics[13], 20, 16, 37);
        assert_diagnostic_range(code, &diagnostics[14], 21, 16, 38);
        assert_diagnostic_range(code, &diagnostics[15], 32, 10, 27);
        assert_diagnostic_range(code, &diagnostics[16], 33, 23, 60);
        assert_diagnostic_range(code, &diagnostics[17], 37, 15, 30);
        assert_diagnostic_range(code, &diagnostics[18], 39, 22, 48);
    }

    #[test]
    fn test_configure() {
        let code = r#"// Пути к файлам и каталогам
//////////////////////////////////////////

Массив = Новый Массив;
// windows
Массив.Добавить("\\test\test\TEST.xls"); // <-- ошибка
Массив.Добавить("\\server\share\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("\\server\share\myfile.txt"); // <-- ошибка
Массив.Добавить("\\123.123.123.123\share\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("c:\folder\myfile.txt"); // <-- ошибка
Массив.Добавить("c:\folder\myfileWithoutExtension"); // <-- ошибка
Массив.Добавить("c:\folder"); // <-- ошибка
Массив.Добавить("c:/folder"); // <-- ошибка
Массив.Добавить("c://folder"); // <-- ошибка
Массив.Добавить("//test/test/TEST.xls"); // <-- ошибка
Массив.Добавить("C:\Program Files (x86)\"); // <-- ошибка
Массив.Добавить("F:\DATA\Develop\Module.bsl"); // <-- ошибка
// unix
Массив.Добавить("/dev/test"); // <-- ошибка
Массив.Добавить("/dev/test/test.bsl"); // <-- ошибка
Массив.Добавить("/home/root/test.bsl"); // <-- ошибка
Массив.Добавить("/etc/php-fpm/php.ini"); // <-- ошибка
Массив.Добавить("/less/test");

// не сработает, там нет путей
ПутьОтносительный = "./catalog"; // не должно срабатывать
Путь = "/catalog"; // исключаем, т.к. может использовано для URL и будет FP
ПутьКФайлу = "d:тутНетПутиКФайлу";
ПутьКоВторомуФайлу = "тут/нетпутикфайлу";
Структура = Новый Структура("Путь", "Съешь ещё этих мягких французских булок, да выпей же чаю.");

// Снова проверки путей
ПутьWin = "~/temp/file.txt"; // <-- ошибка
ПутьДляСомневающихся = "C:\temp/обратный слеш\тоже/работает"; // <-- ошибка
НеПуть = "!/bin/sh";

// Тут невозможно отличить fp с доменным путем от обычного сетевого пути в windows.
ДоменныйПуть = "\\domain\user"; // <-- ошибка

ПутьОтКаталогаЮзера = "%UserProfile%/что/то/там"; // <-- ошибка

ТипыНастроек.Добавить("/CurrentVarianKey");

КомандаЗапуска.Добавить("/LoadExternalDataProcessorOrReportFromFile");

Разрешение = ФабрикаXDTO.Создать(
	ФабрикаXDTO.Тип(
		"http://www.1c.ru/1cFresh/ApplicationExtensions/Permissions/1.0.0.1",
		РазрешениеВДанных.ВидРазрешения
	)
);
"#;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::UsingHardcodePath,
            serde_json::json!({
                "searchWordsStdPathsUnix": "home|lib"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 16, "Expected 16 diagnostics with reduced Unix paths");
    }

    #[test]
    fn test_windows_path_detection() {
        let code = r#"
Процедура Тест()
    Путь = "C:\folder\file.txt";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UsingHardcodePath);
    }

    #[test]
    fn test_unix_path_detection() {
        let code = r#"
Процедура Тест()
    Путь = "/home/user/file.txt";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_unc_path_detection() {
        let code = r#"
Процедура Тест()
    Путь = "\\server\share\folder";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_tilde_path_detection() {
        let code = r#"
Процедура Тест()
    Путь = "~/Documents/file.txt";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_env_var_path_detection() {
        let code = r#"
Процедура Тест()
    Путь = "%UserProfile%/Documents/file.txt";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_url_excluded() {
        let code = r#"
Процедура Тест()
    URL = "http://www.1c.ru/path/to/resource";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "URLs should be excluded");
    }

    #[test]
    fn test_relative_path_excluded() {
        let code = r#"
Процедура Тест()
    Путь = "./catalog";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Relative paths should be excluded");
    }

    #[test]
    fn test_single_slash_excluded() {
        let code = r#"
Процедура Тест()
    Путь = "/catalog";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Single path component without standard root should be excluded"
        );
    }

    #[test]
    fn test_non_standard_unix_root_excluded() {
        let code = r#"
Процедура Тест()
    Путь = "/less/test";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Non-standard Unix root should be excluded");
    }
}
