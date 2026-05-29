use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{
    call_graph::{CallTarget, CallerId, EdgeKind, ModuleCallSummary},
    ModItem, ModuleId,
};
use rustc_hash::{FxHashMap, FxHashSet};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 1.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CognitiveComplexity;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let threshold = ctx.config_int(code, "complexityThreshold", 15) as u32;
    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();
    let module_id = ModuleId::new(ctx.file_id);
    let recursive_methods = local_recursive_methods(&ctx.call_summary(module_id));

    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        let recursion_bonus = if recursive_methods.contains(&local_id) { 1 } else { 0 };
        let total = metrics.cognitive + recursion_bonus;
        if total <= threshold {
            continue;
        }
        let Some(item) = item_tree.top_level_items().get(local_id as usize) else { continue };
        let (name, name_range, is_function) = match item {
            ModItem::Procedure(idx) => {
                let p = item_tree.procedure(*idx);
                (p.name.as_str().to_string(), p.name_range, false)
            }
            ModItem::Function(idx) => {
                let f = item_tree.function(*idx);
                (f.name.as_str().to_string(), f.name_range, true)
            }
            ModItem::Variable(_) => continue,
        };
        let method_type = if is_function { "Функция" } else { "Процедура" };
        out.push(Diagnostic {
            code,
            message: format!(
                "{} '{}' имеет когнитивную сложность {} (максимум: {}). \
                 Упростите логику или уменьшите вложенность",
                method_type, name, total, threshold
            ),
            severity: ctx.severity(code),
            range: name_range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
    out
}

fn local_recursive_methods(summary: &ModuleCallSummary) -> FxHashSet<u32> {
    let mut graph: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
    for edge in &summary.call_edges {
        if !matches!(edge.kind, EdgeKind::DirectLocal) {
            continue;
        }
        let CallerId::Method(caller_id) = edge.caller else { continue };
        let CallTarget::Local { callee_local_id } = edge.target else { continue };
        graph.entry(caller_id).or_default().push(callee_local_id);
    }

    let mut recursive = FxHashSet::default();
    for &start in graph.keys() {
        let mut stack = graph.get(&start).cloned().unwrap_or_default();
        let mut visited = FxHashSet::default();
        while let Some(node) = stack.pop() {
            if node == start {
                recursive.insert(start);
                break;
            }
            if !visited.insert(node) {
                continue;
            }
            if let Some(next) = graph.get(&node) {
                stack.extend(next.iter().copied());
            }
        }
    }
    recursive
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::DiagnosticsConfig;
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

        check_diagnostics_snapshot_for(code, DiagnosticCode::CognitiveComplexity, expect![[r#""#]]);
    }

    #[test]
    fn test_nested_if_higher_complexity() {
        let code = r#"Функция ВложенныеУсловия(А, Б)
    Если А > 0 Тогда
        Если Б > 0 Тогда
            Возврат А + Б;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::CognitiveComplexity, expect![[r#""#]]);
    }

    #[test]
    fn test_deeply_nested_complexity() {
        let code = r#"Функция ГлубокаяВложенность(П1, П2, П3)
    Если П1 > 0 Тогда
        Если П2 > 0 Тогда
            Для Каждого Э Из П3 Цикл
                Если Э > 5 Тогда
                    Возврат 1;
                КонецЕсли;
            КонецЦикла;
        КонецЕсли;
    КонецЕсли;
    Возврат 0;
КонецФункции"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::CognitiveComplexity, expect![[r#""#]]);
    }

    #[test]
    fn test_elseif_no_extra_nesting() {
        let code = r#"Функция СМножественнымиУсловиями(Х)
    Если Х = 1 Тогда
        Возврат "один";
    ИначеЕсли Х = 2 Тогда
        Возврат "два";
    ИначеЕсли Х = 3 Тогда
        Возврат "три";
    Иначе
        Возврат "другое";
    КонецЕсли;
КонецФункции"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::CognitiveComplexity, expect![[r#""#]]);
    }

    #[test]
    fn test_custom_threshold() {
        let code = r#"Функция Тест()
    Если А Тогда
        Если Б Тогда
            Возврат 1;
        КонецЕсли;
    КонецЕсли;
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(2.into()));
        config
            .parameters
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CognitiveComplexity)
            .collect();
        expect![[r#"
            CognitiveComplexity @ 1:9..1:13
              message: Функция 'Тест' имеет когнитивную сложность 3 (максимум: 2). Упростите логику или уменьшите вложенность
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    const COMPLEX_FUNCTION: &str = r#"Функция ОбработатьКоллекцию(Данные, Флаг)
    Итог = 0;

    Если Данные = Неопределено Тогда
        Возврат Итог;
    КонецЕсли;

    Для Каждого Элемент Из Данные Цикл
        Если Элемент.Актуален Тогда
            Если Флаг Тогда
                Для Каждого Строка Из Элемент.Строки Цикл
                    Если Строка.Сумма > 0 Тогда
                        Итог = Итог + Строка.Сумма;
                    ИначеЕсли Строка.Ошибка Тогда
                        Продолжить;
                    Иначе
                        Прервать;
                    КонецЕсли;
                КонецЦикла;
            ИначеЕсли Элемент.Важный Тогда
                Пока Элемент.ТребуетПроверки Цикл
                    Итог = Итог + 1;
                    Прервать;
                КонецЦикла;
            Иначе
                Итог = Итог + 2;
            КонецЕсли;
        КонецЕсли;
    КонецЦикла;

    Возврат Итог;
КонецФункции

Процедура БезСложности()
КонецПроцедуры
"#;

    #[test]
    fn test_comprehensive() {
        let code = COMPLEX_FUNCTION;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::CognitiveComplexity,
            expect![[r#"
            CognitiveComplexity @ 1:9..1:28
              message: Функция 'ОбработатьКоллекцию' имеет когнитивную сложность 25 (максимум: 15). Упростите логику или уменьшите вложенность
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_recursion_penalty_self_call() {
        let code = r#"Функция Факториал(N)
    Если N <= 1 Тогда
        Возврат 1;
    КонецЕсли;
    Возврат N * Факториал(N - 1);
КонецФункции"#;

        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("complexityThreshold".to_string(), serde_json::Value::Number(1.into()));
        config
            .parameters
            .insert(DiagnosticCode::CognitiveComplexity, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::CognitiveComplexity)
            .collect();
        expect![[r#"
            CognitiveComplexity @ 1:9..1:18
              message: Функция 'Факториал' имеет когнитивную сложность 2 (максимум: 1). Упростите логику или уменьшите вложенность
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_compute_hir_metrics_cognitive_value() {
        let code = COMPLEX_FUNCTION;
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

        let body = module_bodies.body(0).expect("Should have first method body");
        let metrics = hir::metrics::compute_hir_metrics(body);

        assert_eq!(metrics.cognitive, 25, "ОбработатьКоллекцию should have cognitive 25");
    }
}
