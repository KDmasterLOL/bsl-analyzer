use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use once_cell::sync::Lazy;
use regex::Regex;
use syntax::SyntaxKind;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Lockinos],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

static PATTERN_NEW_EXPRESSION: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(COMОбъект|COMObject|Почта|Mail)$").unwrap());

static PATTERN_TYPE_PLATFORM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)Linux_x86|Windows|MacOS").unwrap());

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::UsingObjectNotAvailableUnix` is encountered.
pub fn from_hir(type_name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsingObjectNotAvailableUnix;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Проверить, что задействованы аналоги \"{}\" при работе в Unix-клиенте.",
            type_name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn has_platform_check_in_ancestors(node: &syntax::SyntaxNode) -> bool {
    for ancestor in node.ancestors() {
        if ancestor.kind() == SyntaxKind::IF_STMT {
            let text = ancestor.text().to_string();
            if PATTERN_TYPE_PLATFORM.is_match(&text) {
                return true;
            }
        }
        if ancestor.kind() == SyntaxKind::ELSIF_CLAUSE {
            let text = ancestor.text().to_string();
            if PATTERN_TYPE_PLATFORM.is_match(&text) {
                return true;
            }
        }
    }
    false
}

fn get_type_name(new_expr: &syntax::SyntaxNode) -> Option<String> {
    for child in new_expr.children_with_tokens() {
        if let Some(token) = child.into_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UsingObjectNotAvailableUnix;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        if node.kind() != SyntaxKind::NEW_EXPR {
            continue;
        }

        let Some(type_name) = get_type_name(&node) else {
            continue;
        };

        if !PATTERN_NEW_EXPRESSION.is_match(&type_name) {
            continue;
        }

        if has_platform_check_in_ancestors(&node) {
            continue;
        }

        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Проверить, что задействованы аналоги \"{}\" при работе в Unix-клиенте.",
                type_name
            ),
            severity: ctx.severity(code),
            range: node.text_range(),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/UsingObjectNotAvailableUnixDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics, got {}", diagnostics.len());

        assert_diagnostic_range(code, &diagnostics[0], 3, 11, 54);
        assert_diagnostic_range(code, &diagnostics[1], 4, 11, 83);
        assert_diagnostic_range(code, &diagnostics[2], 20, 9, 20);
    }

    #[test]
    fn test_com_object_without_guard() {
        let code = r#"
Процедура Тест()
    obj = Новый COMОбъект("test");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::UsingObjectNotAvailableUnix);
    }

    #[test]
    fn test_mail_without_guard() {
        let code = r#"
Процедура Тест()
    Почта = Новый Почта;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_with_linux_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Linux_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not trigger with Linux guard");
    }

    #[test]
    fn test_with_windows_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Windows_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not trigger with Windows guard");
    }

    #[test]
    fn test_with_macos_guard() {
        let code = r#"
Процедура Тест()
    Если СистемнаяИнформация.ТипПлатформы = ТипПлатформы.MacOS_x86 Тогда
        Почта = Новый Почта;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not trigger with MacOS guard");
    }

    #[test]
    fn test_nested_if_with_guard() {
        let code = r#"
Процедура Тест()
    Если Не СистемнаяИнформация.ТипПлатформы = ТипПлатформы.Linux_x86 Тогда
        Если Истина Тогда
            Почта = Новый Почта;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not trigger with nested guard");
    }

    #[test]
    fn test_internet_mail_not_triggered() {
        let code = r#"
Процедура Тест()
    Почта = Новый ИнтернетПочта();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "ИнтернетПочта should not trigger");
    }

    #[test]
    fn test_english_mail() {
        let code = r#"
Процедура Тест()
    m = New Mail;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "English Mail should trigger");
    }

    #[test]
    fn test_english_com_object() {
        let code = r#"
Процедура Тест()
    obj = New COMObject("test");
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "English COMObject should trigger");
    }

    #[test]
    fn test_hir_detection() {
        use crate::test_utils::check_hir_diagnostic;

        let code = r#"
Процедура Тест()
    obj = Новый COMОбъект("test");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unix: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingObjectNotAvailableUnix)
            .collect();

        assert_eq!(unix.len(), 1, "HIR should detect UsingObjectNotAvailableUnix");
    }

    #[test]
    fn test_hir_with_platform_guard() {
        use crate::test_utils::check_hir_diagnostic;
        let code = r#"
Процедура Тест()
    Если ТипПлатформы.Windows Тогда
        obj = Новый COMОбъект("test");
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let unix: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingObjectNotAvailableUnix)
            .collect();

        assert_eq!(unix.len(), 0, "HIR should NOT detect with Windows guard");
    }
}
