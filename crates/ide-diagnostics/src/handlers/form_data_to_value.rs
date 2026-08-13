use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    crate::simple_hir_diagnostic(
        DiagnosticCode::FormDataToValue,
        "Обнаружено использование метода ДанныеФормыВЗначение",
        range,
        ctx,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_qualified_call_is_someone_elses_method() {
        let code = r#"Процедура Тест()
    Форма=Док.ПолучитьФорму("ФормаДокумента");
    ДФ = Форма.ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_global_call_with_server_annotation() {
        let code = r#"&НаСервере
Функция Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецФункции"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 3:10..3:30
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_server_no_context_does_not_trigger() {
        let code = r#"&НаСервереБезКонтекста
Процедура Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_client_server_no_context_does_not_trigger() {
        let code = r#"&НаКлиентеНаСервереБезКонтекста
Процедура Тест2()
    ДФ = ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_english_qualified_call_is_someone_elses_method() {
        let code = r#"Procedure Test()
    Form = Doc.GetForm("DocumentForm");
    FD = Form.FormDataToValue(Object, Type("ValueTable"));
EndProcedure"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_english_global_call_triggers() {
        let code = r#"Function Test2()
    FormDataToValue(Object, Type("ValueTable"));
EndFunction"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 2:5..2:20
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_global_call_with_context() {
        let code = r#"
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 3:5..3:25
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    /// Ни один тип платформы не объявляет `ДанныеФормыВЗначение` — метод живёт
    /// только в глобальном контексте, поэтому `Получатель.ДанныеФормыВЗначение`
    /// это чужой метод с совпавшим написанием.
    fn test_qualified_call_with_receiver_not_reported() {
        let code = r#"
Процедура Тест()
    Форма.ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_no_context_annotation_skipped() {
        let code = r#"
&НаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_client_at_server_no_context_skipped() {
        let code = r#"
&НаКлиентеНаСервереБезКонтекста
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_server_annotation_detected() {
        let code = r#"
&НаСервере
Функция Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецФункции
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 4:5..4:25
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_client_annotation_detected() {
        let code = r#"
&НаКлиенте
Процедура Тест()
    ДанныеФормыВЗначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 4:5..4:25
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    FormDataToValue(Object, Type("ValueTable"));
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 3:5..3:20
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    ДАННЫЕФОРМЫВЗНАЧЕНИЕ(Объект, Тип("ТаблицаЗначений"));
    ДАННЫЕформыВзначение(Объект, Тип("ТаблицаЗначений"));
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#"
            FormDataToValue @ 3:5..3:25
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint
            FormDataToValue @ 4:5..4:25
              message: Обнаружено использование метода ДанныеФормыВЗначение
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &form_diags));
    }

    #[test]
    fn test_no_call_ignored() {
        let code = r#"
Процедура Тест()
    Метод = ДанныеФормыВЗначение;
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let form_diags: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::FormDataToValue).collect();
        expect![[r#""#]].assert_eq(&format_diags(code, &form_diags));
    }
}
