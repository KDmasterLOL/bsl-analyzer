use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Name;
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &Name, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::GlobalPropertyNotWritable;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Свойство глобального контекста '{}' недоступно для записи. \
             Присваивание не создаёт переменную — выберите другое имя",
            name.as_str()
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;
    use expect_test::expect;

    fn writes(code: &str) -> Vec<crate::Diagnostic> {
        check_hir_diagnostic(code)
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::GlobalPropertyNotWritable)
            .collect()
    }

    #[test]
    fn assigning_to_a_collection_name_is_flagged() {
        let code = r#"
Процедура Тест()
    Справочники = Новый Структура;
КонецПроцедуры
"#;
        expect![[r#"
            GlobalPropertyNotWritable @ 3:5..3:16
              message: Свойство глобального контекста 'Справочники' недоступно для записи. Присваивание не создаёт переменную — выберите другое имя
              severity: Major"#]]
        .assert_eq(&format_diags(code, &writes(code)));
    }

    #[test]
    fn english_collection_name_is_flagged_too() {
        let code = r#"
Процедура Тест()
    Catalogs = Новый Структура;
КонецПроцедуры
"#;
        assert_eq!(writes(code).len(), 1, "the English spelling names the same property");
    }

    #[test]
    fn a_declared_owner_takes_the_name_and_is_not_flagged() {
        // Every declared owner makes the name its own, so the assignment stores
        // into that owner and is perfectly legal.
        for code in [
            "Перем Справочники;\n\nПроцедура Тест()\n    Справочники = Новый Структура;\nКонецПроцедуры\n",
            "Процедура Тест(Справочники)\n    Справочники = Новый Структура;\nКонецПроцедуры\n",
            "Процедура Тест()\n    Перем Справочники;\n    Справочники = Новый Структура;\nКонецПроцедуры\n",
        ] {
            assert!(
                writes(code).is_empty(),
                "a declared owner owns the name, assignment is legal:\n{code}"
            );
        }
    }

    #[test]
    fn a_name_that_is_not_a_collection_is_not_flagged() {
        let code = r#"
Процедура Тест()
    МояПеременная = Новый Структура;
КонецПроцедуры
"#;
        assert!(writes(code).is_empty(), "an ordinary name declares a local as always");
    }

    /// Одно нарушение — одна диагностика. Синтаксическая проверка самоприсваивания
    /// видит только равные стороны и называет цель переменной; здесь переменной нет,
    /// поэтому точный вердикт её вытесняет.
    ///
    /// Спрашивать надо весь конвейер: вытеснение — свойство ИТОГОВОГО набора, и до
    /// выхода вердикт-победитель ещё может быть отозван базовым модулем пары.
    #[test]
    fn a_refused_self_assignment_is_reported_once() {
        let code = "Процедура Тест()\n    Справочники = Справочники;\n    А = А;\nКонецПроцедуры\n";
        let diags = crate::test_utils::check_file_diagnostics(code);
        assert_eq!(
            diags.iter().filter(|d| d.code == DiagnosticCode::GlobalPropertyNotWritable).count(),
            1,
            "{diags:#?}"
        );
        let self_assigns: Vec<_> =
            diags.iter().filter(|d| d.code == DiagnosticCode::SelfAssign).collect();
        assert_eq!(
            self_assigns.len(),
            1,
            "only the ordinary self-assignment survives:\n{self_assigns:#?}"
        );
    }

    #[test]
    fn repeated_assignment_is_flagged_once_per_statement() {
        let code = r#"
Процедура Тест()
    Справочники = Новый Структура;
    Справочники = Новый Массив;
КонецПроцедуры
"#;
        assert_eq!(writes(code).len(), 2, "each illegal write is its own defect");
    }
}
