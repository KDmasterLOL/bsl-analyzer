use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Body, BodySourceMap, Expr, Literal};
use once_cell::sync::Lazy;
use regex::Regex;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_SEARCH_WORDS_EXCLUSION: &str =
    "Верси|Version|ЗапуститьПриложение|RunApp|Пространств|Namespace|Драйвер|Driver";

const DEFAULT_POPULAR_VERSION_EXCLUSION: &str = r"^(1|2|3|8\.3|11)\.";

const DOTS_IN_IPV4: usize = 3;

static PATTERN_NETWORK_ADDRESS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)(([0-9a-fA-F]{1,4}:){7,7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|\s:)|fe80:(:[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|::(ffff(:0{1,4}){0,1}:){0,1}((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])|([0-9a-fA-F]{1,4}:){1,4}:((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9]))|((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])"
    ).unwrap()
});

static PATTERN_URL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(ftp|http|https)://[^ \x22].*").unwrap());

static PATTERN_ALPHABET: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)[A-Za-zА-Яа-яЁё]").unwrap());

#[derive(Debug, Clone)]
struct Config {
    search_words_exclusion: Regex,
    search_popular_version_exclusion: Regex,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let words_exclusion_str = ctx
            .config
            .get_string(DiagnosticCode::UsingHardcodeNetworkAddress, "searchWordsExclusion")
            .unwrap_or(DEFAULT_SEARCH_WORDS_EXCLUSION);

        let version_exclusion_str = ctx
            .config
            .get_string(
                DiagnosticCode::UsingHardcodeNetworkAddress,
                "searchPopularVersionExclusion",
            )
            .unwrap_or(DEFAULT_POPULAR_VERSION_EXCLUSION);

        let search_words_exclusion = Regex::new(&format!("(?i){}", words_exclusion_str))
            .unwrap_or_else(|_| {
                Regex::new(&format!("(?i){}", DEFAULT_SEARCH_WORDS_EXCLUSION)).unwrap()
            });

        let search_popular_version_exclusion =
            Regex::new(&format!("(?i){}", version_exclusion_str)).unwrap_or_else(|_| {
                Regex::new(&format!("(?i){}", DEFAULT_POPULAR_VERSION_EXCLUSION)).unwrap()
            });

        Self { search_words_exclusion, search_popular_version_exclusion }
    }
}

fn is_url(content: &str) -> bool {
    PATTERN_URL.is_match(content)
}

fn count_char(s: &str, c: char) -> usize {
    s.chars().filter(|&ch| ch == c).count()
}

fn is_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::LABEL_STMT
            | SyntaxKind::ADD_HANDLER_STMT
            | SyntaxKind::REMOVE_HANDLER_STMT
            | SyntaxKind::EMPTY_STMT
    )
}

fn find_ancestor_by_kind(token: &SyntaxToken, kind: SyntaxKind) -> Option<SyntaxNode> {
    let mut node = token.parent();
    while let Some(current) = node {
        if current.kind() == kind {
            return Some(current);
        }
        node = current.parent();
    }
    None
}

fn find_statement_ancestor(token: &SyntaxToken) -> Option<SyntaxNode> {
    let mut node = token.parent();
    while let Some(current) = node {
        if is_statement_kind(current.kind()) {
            return Some(current);
        }
        node = current.parent();
    }
    None
}

fn skip_statement_context(token: &SyntaxToken, config: &Config) -> bool {
    if let Some(stmt) = find_statement_ancestor(token) {
        let text = stmt.text().to_string();
        return config.search_words_exclusion.is_match(&text);
    }
    false
}

fn skip_param_context(token: &SyntaxToken, config: &Config) -> bool {
    if let Some(param) = find_ancestor_by_kind(token, SyntaxKind::PARAM) {
        let text = param.text().to_string();
        return config.search_words_exclusion.is_match(&text);
    }
    false
}

fn is_version_return(token: &SyntaxToken, config: &Config) -> bool {
    if find_ancestor_by_kind(token, SyntaxKind::RETURN_STMT).is_some() {
        if let Some(func) = find_ancestor_by_kind(token, SyntaxKind::FUNCTION_DEF) {
            let text = func.text().to_string();
            return config.search_words_exclusion.is_match(&text);
        }
    }
    false
}

fn is_letter_before_match(content: &str, match_start: usize) -> bool {
    if match_start == 0 {
        return false;
    }
    let bytes = content.as_bytes();
    if match_start > bytes.len() {
        return false;
    }
    let before = &content[..match_start];
    if let Some(c) = before.chars().last() {
        let lower = c.to_ascii_lowercase();
        return ('g'..='z').contains(&lower) || matches!(c, 'А'..='Я' | 'а'..='я' | 'Ё' | 'ё');
    }
    false
}

/// Find the STRING token in the AST at the given range (for context checks).
fn find_string_token(root: &SyntaxNode, range: ide_db::TextRange) -> Option<SyntaxToken> {
    root.token_at_offset(range.start())
        .find(|t| t.kind() == SyntaxKind::STRING && t.text_range() == range)
}

/// HIR-based entry point for UsingHardcodeNetworkAddress diagnostic.
///
/// Uses Salsa-cached module_bodies for string literal discovery.
/// Falls back to AST only for context exclusion checks (rare path).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UsingHardcodeNetworkAddress;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let module_bodies = ctx.module_bodies();
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for (_, body, source_map) in module_bodies.method_bodies() {
        check_body(body, source_map, &root, &config, code, ctx, &mut diagnostics);
    }

    if let Some(module_result) = module_bodies.module_code_result() {
        check_body(
            &module_result.body,
            &module_result.source_map,
            &root,
            &config,
            code,
            ctx,
            &mut diagnostics,
        );
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

/// Check a single HIR body for hardcoded network addresses.
#[allow(clippy::too_many_arguments)]
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    root: &SyntaxNode,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (expr_id, expr) in body.exprs_iter() {
        let Expr::Literal(Literal::String(content)) = expr else { continue };

        if content.len() <= 2 || is_url(content) {
            continue;
        }

        let Some(matched) = PATTERN_NETWORK_ADDRESS.find(content) else { continue };

        let first_value = matched.as_str();
        let count_dots = count_char(first_value, '.');
        let count_dots_all = count_char(content, '.');
        let find_alphabet = PATTERN_ALPHABET.is_match(first_value);

        if count_dots > 0 && (count_dots_all > DOTS_IN_IPV4 || find_alphabet) {
            continue;
        }

        if first_value.starts_with(':') && is_letter_before_match(content, matched.start()) {
            continue;
        }

        let Some(range) = source_map.expr_range(expr_id) else { continue };

        // Context exclusion checks via AST (only for matched strings — rare path)
        if let Some(token) = find_string_token(root, range) {
            if skip_statement_context(&token, config)
                || skip_param_context(&token, config)
                || is_version_return(&token, config)
            {
                continue;
            }
        }

        if config.search_popular_version_exclusion.is_match(content) {
            continue;
        }

        diagnostics.push(Diagnostic {
            code,
            message: "Используется хранение в коде ip-адреса".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
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
        let code = include_str!("../../test_data/UsingHardcodeNetworkAddressDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 2, 15, 31);
        assert_diagnostic_range(code, &diagnostics[1], 6, 23, 39);
        assert_diagnostic_range(code, &diagnostics[2], 7, 23, 34);
        assert_diagnostic_range(code, &diagnostics[3], 9, 23, 64);
        assert_diagnostic_range(code, &diagnostics[4], 10, 23, 64);
        assert_diagnostic_range(code, &diagnostics[5], 12, 44, 85);
        assert_diagnostic_range(code, &diagnostics[6], 20, 18, 29);
        assert_diagnostic_range(code, &diagnostics[7], 23, 7, 119);
        assert_diagnostic_range(code, &diagnostics[8], 55, 13, 18);
        assert_diagnostic_range(code, &diagnostics[9], 57, 104, 114);
        assert_diagnostic_range(code, &diagnostics[10], 65, 9, 22);
        assert_diagnostic_range(code, &diagnostics[11], 71, 6, 15);
    }

    #[test]
    fn test_configure_search_words() {
        let code = include_str!("../../test_data/UsingHardcodeNetworkAddressDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::UsingHardcodeNetworkAddress,
            serde_json::json!({
                "searchWordsExclusion": "Version"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 13, "Expected 13 diagnostics with reduced exclusion");
    }

    #[test]
    fn test_configure_version_exclusion() {
        let code = include_str!("../../test_data/UsingHardcodeNetworkAddressDiagnostic.bsl");
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::UsingHardcodeNetworkAddress,
            serde_json::json!({
                "searchPopularVersionExclusion": r"^(1|3|8\.3|11)\."
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 15, "Expected 15 diagnostics without 2.* exclusion");
    }

    #[test]
    fn test_ipv4_detection() {
        let code = r#"
Процедура Тест()
    IP = "192.168.1.1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UsingHardcodeNetworkAddress);
    }

    #[test]
    fn test_ipv6_detection() {
        let code = r#"
Процедура Тест()
    IP = "2001:0db8:85a3:0000:0000:8a2e:0370:7334";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_localhost_detection() {
        let code = r#"
Процедура Тест()
    IP = "127.0.0.1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_ipv6_loopback_detection() {
        let code = r#"
Процедура Тест()
    IP = "::1";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_url_excluded() {
        let code = r#"
Процедура Тест()
    URL = "http://192.168.1.1/api";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "URLs should be excluded");
    }

    #[test]
    fn test_version_excluded() {
        let code = r#"
Процедура Тест()
    Версия = "1.2.3.4";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Popular versions should be excluded");
    }

    #[test]
    fn test_run_app_excluded() {
        let code = r#"
Процедура Тест()
    ЗапуститьПриложение("ping -n 60 127.0.0.1 >nul", , Истина);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "RunApp context should be excluded");
    }

    #[test]
    fn test_driver_excluded() {
        let code = r#"
Процедура Тест()
    Справочники.ДрайверыОборудования.ЗаполнитьПредопределенныйЭлемент(
        "Драйвер", "AddIn.VFCD220E", "1.0.1.1");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Driver context should be excluded");
    }

    #[test]
    fn test_invalid_ip_not_detected() {
        let code = r#"
Процедура Тест()
    НеIP = "300.300.300.300";
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Invalid IPs should not be detected");
    }

    #[test]
    fn test_version_return_excluded() {
        let code = r#"
Функция ВерсияБиблиотеки() Экспорт
    Возврат "2.9.4.0";
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Version function return should be excluded");
    }
}
