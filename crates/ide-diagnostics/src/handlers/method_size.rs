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
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

const DEFAULT_MAX_METHOD_SIZE: i64 = 200;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MethodSize;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_method_size = ctx.config_int(code, "maxMethodSize", DEFAULT_MAX_METHOD_SIZE) as u32;

    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let item_tree = ctx.item_tree();

    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        if metrics.size_lines == 0 || metrics.size_lines <= max_method_size {
            continue;
        }
        let Some(item) = item_tree.top_level_items().get(local_id as usize) else { continue };
        let (name, name_range) = match item {
            ModItem::Procedure(idx) => {
                let p = item_tree.procedure(*idx);
                (p.name.as_str().to_string(), p.name_range)
            }
            ModItem::Function(idx) => {
                let f = item_tree.function(*idx);
                (f.name.as_str().to_string(), f.name_range)
            }
            ModItem::Variable(_) => continue,
        };
        out.push(Diagnostic {
            code,
            message: format!(
                "Длина метода \"{}\" равна {}, что больше установленного лимита в {} строк",
                name, metrics.size_lines, max_method_size
            ),
            severity: ctx.severity(code),
            range: name_range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    fn make_method_size_code() -> String {
        let mut s = String::new();
        s.push_str("Процедура ПустаяПроцедура()\n\n КонецПроцедуры\n\n");
        s.push_str("Функция ФункцияВОднуСтроку() КонецФункции\n\n");
        s.push_str("Процедура Процедура201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        s.push_str("Процедура Процедура200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        s.push_str("Функция Функция201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        s.push_str("Функция Функция200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        s.push_str("Функция А(А=0)\n\n КонецФункции\n");
        s
    }

    #[test]
    fn test_comprehensive() {
        let code = make_method_size_code();
        check_diagnostics_snapshot_for(
            &code,
            DiagnosticCode::MethodSize,
            expect![[r#"
            MethodSize @ 7:11..7:29
              message: Длина метода "Процедура201Строка" равна 201, что больше установленного лимита в 200 строк
              severity: Warning
            MethodSize @ 420:9..420:25
              message: Длина метода "Функция201Строка" равна 201, что больше установленного лимита в 200 строк
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_configure_threshold_20() {
        let code = make_method_size_code();
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("maxMethodSize".to_string(), serde_json::Value::Number(20.into()));
        config.parameters.insert(DiagnosticCode::MethodSize, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(&code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.into_iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        expect![[r#"
            MethodSize @ 7:11..7:29
              message: Длина метода "Процедура201Строка" равна 201, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 214:11..214:28
              message: Длина метода "Процедура200Строк" равна 200, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 420:9..420:25
              message: Длина метода "Функция201Строка" равна 201, что больше установленного лимита в 20 строк
              severity: Warning
            MethodSize @ 627:9..627:24
              message: Длина метода "Функция200Строк" равна 200, что больше установленного лимита в 20 строк
              severity: Warning"#]].assert_eq(&format_diags(&code, &diagnostics));
    }

    #[test]
    fn test_empty_method() {
        let code = r#"Процедура Пустая()

КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::MethodSize, expect![[r#""#]]);
    }

    #[test]
    fn test_one_liner() {
        let code = r#"Функция Тест() КонецФункции"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::MethodSize, expect![[r#""#]]);
    }
}
