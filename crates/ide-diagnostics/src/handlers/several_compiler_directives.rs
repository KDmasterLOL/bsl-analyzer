//! SeveralCompilerDirectives diagnostic.
//!
//! Checks that a module variable or method has no more than one compiler directive.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SeveralCompilerDirectives;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let item_tree = ctx.item_tree();

    for (_, proc) in item_tree.procedures() {
        if proc.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(proc.name_range, code, ctx));
        }
    }

    for (_, func) in item_tree.functions() {
        if func.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(func.name_range, code, ctx));
        }
    }

    for (_, var) in item_tree.variables() {
        if var.annotations.len() > 1 {
            diagnostics.push(make_diagnostic(var.name_range, code, ctx));
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Указано более одной директивы компиляции".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use expect_test::expect;
    #[test]
    fn test_from_java_fixture() {
        let code = r#"&НаКлиенте
Перем ПравильноАннотирована;

&НаКлиенте
// так тоже правильно
Перем ПравильноАннотирована2;

&НаСервере

// так тоже правильно

Перем ПравильноАннотирована3;

&НаКлиенте
&НаКлиенте
Перем НеПравильноАннотирована1;

&НаКлиенте
&НаСервере
Перем НеПравильноАннотирована2;

&НаКлиенте

&НаСервере
// не правильно
Перем НеПравильноАннотирована3;

&НаКлиенте
Функция ПравильноАннотирована1()
    Возврат 1;
КонецФункции

&НаСервере
// Описание метода
Процедура ПравильноАннотирована2()
    Действие();
КонецПроцедуры

&НаСервере
&НаКлиенте
Процедура НеПравильноАннотирована1()
    Действие();
КонецПроцедуры

&НаСервере
// комментарий
&НаКлиенте

// комментарий
Процедура НеПравильноАннотирована2()
    Действие();
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);

        expect![[r#"
            SeveralCompilerDirectives @ 16:7..16:31
              message: Указано более одной директивы компиляции
              severity: Critical
            SeveralCompilerDirectives @ 20:7..20:31
              message: Указано более одной директивы компиляции
              severity: Critical
            SeveralCompilerDirectives @ 26:7..26:31
              message: Указано более одной директивы компиляции
              severity: Critical
            SeveralCompilerDirectives @ 41:11..41:35
              message: Указано более одной директивы компиляции
              severity: Critical
            SeveralCompilerDirectives @ 50:11..50:35
              message: Указано более одной директивы компиляции
              severity: Critical"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_single_directive_ok() {
        let code = "&НаКлиенте\nПерем ОК;";
        let diagnostics = check_ast_diagnostic(code, super::check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_directive_ok() {
        let code = "Перем ОК;\n\nПроцедура Тест()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, super::check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
