//! OrderOfParams diagnostic.
//!
//! Reports methods where optional parameters appear before required ones.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::ModItem;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::OrderOfParams;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for item in item_tree.top_level_items() {
        match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                check_method(&proc.params, code, ctx, &mut diagnostics);
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                check_method(&func.params, code, ctx, &mut diagnostics);
            }
            ModItem::Variable(_) => {}
        }
    }

    diagnostics
}

fn check_method(
    params: &[hir::Param],
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Find first optional parameter
    let first_optional_idx = params.iter().position(|p| p.has_default);
    let Some(first_optional_idx) = first_optional_idx else {
        return; // No optional params - nothing to check
    };

    // Report each required parameter that comes after an optional one
    for param in params.iter().skip(first_optional_idx + 1) {
        if !param.has_default {
            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Переместите обязательный параметр '{}' перед необязательными",
                    param.name.as_str()
                ),
                severity: ctx.severity(code),
                range: param.name_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, format_diags};
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура МимоРаз()

КонецПроцедуры

Процедура МимоДва(Раз, Два, Три, Четыре, Пять, Шесть)

КонецПроцедуры


Функция МимоТри(Раз, Два, Три, Четыре, Пять = 5, Шесть = 6, Семь = 7)
    Возврат;
КонецФункции


Процедура СработкаПоНеобязательныйПередОбязательным(Раз, Два = 2, Три = 3, Четыре, Пять, Шесть, Семь=7)

КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            OrderOfParams @ 15:76..15:82
              message: Переместите обязательный параметр 'Четыре' перед необязательными
              severity: Warning
            OrderOfParams @ 15:84..15:88
              message: Переместите обязательный параметр 'Пять' перед необязательными
              severity: Warning
            OrderOfParams @ 15:90..15:95
              message: Переместите обязательный параметр 'Шесть' перед необязательными
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_params() {
        let code = r#"Процедура Тест() КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_all_required() {
        let code = r#"Процедура Тест(А, Б, В) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_all_optional() {
        let code = r#"Процедура Тест(А = 1, Б = 2, В = 3) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_correct_order() {
        let code = r#"Процедура Тест(А, Б, В = 3, Г = 4) КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_wrong_order_single() {
        let code = r#"Процедура Тест(А, Б = 2, В)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            OrderOfParams @ 1:26..1:27
              message: Переместите обязательный параметр 'В' перед необязательными
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_wrong_order_multiple() {
        let code = r#"Процедура Тест(А = 1, Б, В)
КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            OrderOfParams @ 1:23..1:24
              message: Переместите обязательный параметр 'Б' перед необязательными
              severity: Warning
            OrderOfParams @ 1:26..1:27
              message: Переместите обязательный параметр 'В' перед необязательными
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }
}
