use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::BodySourceMap;
use hir::LocalRange;
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Brainoverload],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

const DEFAULT_MAX_ALLOWED_LEVEL: i64 = 4;

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::NestedStatements;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let max_allowed_level =
        ctx.config_int(code, "maxAllowedLevel", DEFAULT_MAX_ALLOWED_LEVEL) as u32;
    let metrics = ctx.hir_metrics();
    if metrics.nesting_leaves.is_empty() {
        return;
    }
    emit_leaves(ctx, code, &metrics, ctx.source_map(), ctx.root(), max_allowed_level, acc);
}

fn emit_leaves(
    ctx: &BodyContext,
    code: DiagnosticCode,
    metrics: &hir::metrics::HirMethodMetrics,
    source_map: &BodySourceMap,
    root: &SyntaxNode,
    max_allowed_level: u32,
    out: &mut Vec<Diagnostic<LocalRange>>,
) {
    for leaf in metrics.nesting_leaves.iter() {
        if leaf.depth <= max_allowed_level {
            continue;
        }
        let Some(stmt_range) = source_map.stmt_range(leaf.stmt) else { continue };
        let keyword_range = first_nesting_keyword(root, stmt_range.in_root())
            .map(LocalRange::of_detached_node)
            .unwrap_or(stmt_range);
        out.push(Diagnostic {
            code,
            message: "Управляющие конструкции не должны быть вложены слишком глубоко".to_string(),
            severity: ctx.severity(code),
            range: keyword_range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn first_nesting_keyword(
    root: &SyntaxNode,
    stmt_range: ide_db::TextRange,
) -> Option<ide_db::TextRange> {
    let node = match root.covering_element(stmt_range) {
        NodeOrToken::Node(n) => n,
        NodeOrToken::Token(t) => return Some(t.text_range()),
    };
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            stmt_range.contains_range(t.text_range())
                && matches!(
                    t.kind(),
                    SyntaxKind::KW_IF
                        | SyntaxKind::KW_WHILE
                        | SyntaxKind::KW_FOR
                        | SyntaxKind::KW_TRY
                )
        })
        .map(|t| t.text_range())
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{
        check_diagnostics_snapshot_for, check_hir_diagnostic_with_config, format_diags,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;
    #[test]
    fn test_no_nesting() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::NestedStatements, expect![[r#""#]]);
    }

    #[test]
    fn test_max_nesting_no_violation() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(code, DiagnosticCode::NestedStatements, expect![[r#""#]]);
    }

    #[test]
    fn test_exceed_max_nesting() {
        let code = r#"Процедура Тест()
Если а Тогда
    Если б Тогда
        Если в Тогда
            Если г Тогда
                Если д Тогда
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецЕсли;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedStatements,
            expect![[r#"
            NestedStatements @ 6:17..6:21
              message: Управляющие конструкции не должны быть вложены слишком глубоко
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура А()
 Если а Тогда     //1
   Если б Тогда   //2
    Если в Тогда  //3
     Если г Тогда //4 Максимуим но не сработало
     КонецЕсли;
    КонецЕсли;
  КонецЕсли;
 КонецЕсли;
КонецПроцедуры

Если аа Тогда    //1
   Пока вв Цикл  //2
    Попытка      //3 Мимо
    Исключение
    КонецПопытки;
  КонецЦикла;
КонецЕсли;

Если ааа Тогда  //Мимо
 Если ббб Тогда
 КонецЕсли;
 Если ввв Тогда
 КонецЕсли;
 Если ггг Тогда
 КонецЕсли;
 Если ддд Тогда
 КонецЕсли;
КонецЕсли;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
        Если дддд Тогда    //5 Сработало
        КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
    Если дддд Тогда        //5
     Если ееее Тогда       //6
      Если жжжж Тогда      //7 Сработало

      КонецЕсли;
     КонецЕсли;
    КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedStatements,
            expect![[r#"
            NestedStatements @ 36:9..36:13
              message: Управляющие конструкции не должны быть вложены слишком глубоко
              severity: Warning
            NestedStatements @ 51:7..51:11
              message: Управляющие конструкции не должны быть вложены слишком глубоко
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_custom_max_level() {
        let code = r#"Процедура А()
 Если а Тогда     //1
   Если б Тогда   //2
    Если в Тогда  //3
     Если г Тогда //4 Максимуим но не сработало
     КонецЕсли;
    КонецЕсли;
  КонецЕсли;
 КонецЕсли;
КонецПроцедуры

Если аа Тогда    //1
   Пока вв Цикл  //2
    Попытка      //3 Мимо
    Исключение
    КонецПопытки;
  КонецЦикла;
КонецЕсли;

Если ааа Тогда  //Мимо
 Если ббб Тогда
 КонецЕсли;
 Если ввв Тогда
 КонецЕсли;
 Если ггг Тогда
 КонецЕсли;
 Если ддд Тогда
 КонецЕсли;
КонецЕсли;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
        Если дддд Тогда    //5 Сработало
        КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;

Пока аааа Цикл             //1
 Если бббб Тогда           //2
 Иначе
  Попытка                  //3
   Для А = 1 По гггг Цикл  //4 Максимуим
    Если дддд Тогда        //5
     Если ееее Тогда       //6
      Если жжжж Тогда      //7 Сработало

      КонецЕсли;
     КонецЕсли;
    КонецЕсли;
   КонецЦикла;
  Исключение
  КонецПопытки;
 КонецЕсли;
КонецЦикла;"#;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::NestedStatements, serde_json::json!({ "maxAllowedLevel": 6 }));

        let diagnostics = check_hir_diagnostic_with_config(code, config, crate::diagnostics);
        let diagnostics: Vec<_> = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::NestedStatements)
            .collect();
        expect![[r#"
            NestedStatements @ 51:7..51:11
              message: Управляющие конструкции не должны быть вложены слишком глубоко
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_hir_detection() {
        let code = r#"
Процедура Тест()
    Если а Тогда
        Если б Тогда
            Если в Тогда
                Если г Тогда
                    Если д Тогда
                    КонецЕсли;
                КонецЕсли;
            КонецЕсли;
        КонецЕсли;
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::NestedStatements,
            expect![[r#"
            NestedStatements @ 7:21..7:25
              message: Управляющие конструкции не должны быть вложены слишком глубоко
              severity: Warning"#]],
        );
    }
}
