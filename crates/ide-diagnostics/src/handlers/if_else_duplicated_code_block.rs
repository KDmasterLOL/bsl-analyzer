use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::IfElseDuplicatedCodeBlock,
        "Ветки Если и Иначе содержат идентичный код",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_simple_if_else_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    Иначе
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#"
            IfElseDuplicatedCodeBlock @ 3:9..5:5
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_different_blocks() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка 1");
    Иначе
        ПоказатьПредупреждение("Ошибка 2");
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_elsif_duplicate() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    ИначеЕсли x = 2 Тогда
        ПоказатьПредупреждение("Ошибка");
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#"
            IfElseDuplicatedCodeBlock @ 3:9..5:5
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_empty_blocks_ignored() {
        let code = r#"Процедура Тест()
    Если x = 1 Тогда
    ИначеЕсли x = 2 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_empty_blocks_all_branches() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
    ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
    Иначе
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_if_else_two_statement_duplicate() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#"
            IfElseDuplicatedCodeBlock @ 3:9..5:5
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_if_else_different_statement_count() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_if_elseif_duplicate_with_else() {
        let code = r#"Процедура Тест()
    Если ПараметрКоманды.Количество() = 0 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        Возврат;
    Иначе
        ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#"
            IfElseDuplicatedCodeBlock @ 3:9..5:5
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
    }

    #[test]
    fn test_nested_duplicates_in_outer_branches() {
        let code = r#"Процедура Тест()
    Если ТипЗнч(ПараметрКоманды) = Тип("Массив") Тогда
        Если ПараметрКоманды.Количество() = 0 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        Иначе
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        КонецЕсли;
    Иначе
        Если ПараметрКоманды.Количество() = 0 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        ИначеЕсли ПараметрКоманды.Количество() = 1 Тогда
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
            Возврат;
        Иначе
            ПоказатьПредупреждение(,НСтр("ru= 'Сообщение'"));
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::IfElseDuplicatedCodeBlock)
            .collect();
        expect![[r#"
            IfElseDuplicatedCodeBlock @ 3:9..12:5
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information
            IfElseDuplicatedCodeBlock @ 4:13..6:9
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information
            IfElseDuplicatedCodeBlock @ 14:13..16:9
              message: Ветки Если и Иначе содержат идентичный код
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diags));
    }
}
