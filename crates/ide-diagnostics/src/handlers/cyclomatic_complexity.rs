use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 25,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::CyclomaticComplexity;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let threshold = ctx.config_int(code, "complexityThreshold", 20) as u32;
    let (Some(decl), Some(complexity), Some(name_range)) =
        (ctx.decl(), ctx.cyclomatic(), ctx.method_name_range())
    else {
        return;
    };
    if complexity <= threshold {
        return;
    }
    let method_type = if decl.is_function { "Функция" } else { "Процедура" };
    acc.push(Diagnostic {
        code,
        message: format!(
            "{} '{}' имеет цикломатическую сложность {} (максимум: {}). \
             Рассмотрите возможность упрощения или разбиения на более мелкие функции",
            method_type,
            decl.name.as_str(),
            complexity,
            threshold
        ),
        severity: ctx.severity(code),
        range: name_range,
        tags: ctx.tags(code),
        fixes: vec![],
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    use hir::ModuleId;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::vfs::{FileSet, VfsPath};
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;
    #[test]
    fn test_simple_function() {
        let code = r#"Функция ПростаяФункция(Параметр)
    Возврат Параметр + 1;
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CyclomaticComplexity,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_else_counts() {
        let code = r#"Функция Тест()
    Если А Тогда
        Возврат 1;
    Иначе
        Возврат 2;
    КонецЕсли;
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CyclomaticComplexity,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_high_complexity_triggers_diagnostic() {
        let code = r#"Функция РассчитатьМаршрут(Сумма, ТипКлиента, Режим, ЕстьСкидка)
    Результат = 0;
    Если Сумма > 100 Тогда
        Результат = 1;
    ИначеЕсли Сумма > 50 Тогда
        Результат = 2;
    Иначе
        Результат = 3;
    КонецЕсли;
    Если ТипКлиента = "VIP" Тогда
        Результат = Результат + 10;
    ИначеЕсли ТипКлиента = "Retail" Тогда
        Результат = Результат + 5;
    Иначе
        Результат = Результат + 1;
    КонецЕсли;
    Для Индекс = 0 По 3 Цикл
        Если Индекс % 2 = 0 Тогда
            Результат = Результат + Индекс;
        Иначе
            Результат = Результат - Индекс;
        КонецЕсли;
    КонецЦикла;
    Пока Результат < 20 Цикл
        Если Режим = "A" Тогда
            Результат = Результат + 2;
        ИначеЕсли Режим = "B" Тогда
            Результат = Результат + 3;
        Иначе
            Результат = Результат + 4;
        КонецЕсли;
        Прервать;
    КонецЦикла;
    Попытка
        Значение = Результат;
    Исключение
        Значение = 0;
    КонецПопытки;
    Если ЕстьСкидка Тогда
        Результат = ?(Режим = "A", 10, ?(Режим = "B", 20, 30));
    КонецЕсли;
    Условие = Сумма > 0 И ТипКлиента <> "";
    Условие2 = Режим = "A" ИЛИ Режим = "B";
    Условие3 = ЕстьСкидка И Сумма > 50 ИЛИ Режим = "C" И ТипКлиента <> "";
    Если Условие И Условие2 Тогда
        Результат = Результат + 1;
    КонецЕсли;
    Если Условие3 ИЛИ ЕстьСкидка Тогда
        Результат = Результат + 2;
    КонецЕсли;
    Возврат Результат;
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CyclomaticComplexity,
            expect![[r#"
                CyclomaticComplexity @ 1:9..1:26
                  message: Функция 'РассчитатьМаршрут' имеет цикломатическую сложность 23 (максимум: 20). Рассмотрите возможность упрощения или разбиения на более мелкие функции
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_calculate_complexity_directly() {
        let code = r#"Функция РассчитатьМаршрут(Сумма, ТипКлиента, Режим, ЕстьСкидка)
    Результат = 0;
    Если Сумма > 100 Тогда
        Результат = 1;
    ИначеЕсли Сумма > 50 Тогда
        Результат = 2;
    Иначе
        Результат = 3;
    КонецЕсли;
    Если ТипКлиента = "VIP" Тогда
        Результат = Результат + 10;
    ИначеЕсли ТипКлиента = "Retail" Тогда
        Результат = Результат + 5;
    Иначе
        Результат = Результат + 1;
    КонецЕсли;
    Для Индекс = 0 По 3 Цикл
        Если Индекс % 2 = 0 Тогда
            Результат = Результат + Индекс;
        Иначе
            Результат = Результат - Индекс;
        КонецЕсли;
    КонецЦикла;
    Пока Результат < 20 Цикл
        Если Режим = "A" Тогда
            Результат = Результат + 2;
        ИначеЕсли Режим = "B" Тогда
            Результат = Результат + 3;
        Иначе
            Результат = Результат + 4;
        КонецЕсли;
        Прервать;
    КонецЦикла;
    Попытка
        Значение = Результат;
    Исключение
        Значение = 0;
    КонецПопытки;
    Если ЕстьСкидка Тогда
        Результат = ?(Режим = "A", 10, ?(Режим = "B", 20, 30));
    КонецЕсли;
    Условие = Сумма > 0 И ТипКлиента <> "";
    Условие2 = Режим = "A" ИЛИ Режим = "B";
    Условие3 = ЕстьСкидка И Сумма > 50 ИЛИ Режим = "C" И ТипКлиента <> "";
    Если Условие И Условие2 Тогда
        Результат = Результат + 1;
    КонецЕсли;
    Если Условие3 ИЛИ ЕстьСкидка Тогда
        Результат = Результат + 2;
    КонецЕсли;
    Возврат Результат;
КонецФункции"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let module_id = ModuleId::new(file_id);
        let module_bodies = db.module_bodies(module_id);

        let (_, body) = module_bodies.iter_bodies().next().expect("Should have first method body");

        let cfg = hir::cfg::CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body);
        let complexity = hir::cfg::cyclomatic_complexity(&cfg);
        assert_eq!(complexity, 14, "РассчитатьМаршрут CFG-based cyclomatic should be 14");

        let metrics = hir::metrics::compute_hir_metrics(body);
        assert_eq!(
            metrics.boolean_ops_count, 7,
            "boolean ops contribute +7 to SonarQube cyclomatic"
        );
        assert_eq!(metrics.ternary_count, 2, "ternary expressions contribute +2");
        assert_eq!(complexity + metrics.boolean_ops_count + metrics.ternary_count, 23);
    }
}
