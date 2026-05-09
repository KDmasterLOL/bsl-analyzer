//! NestedStatements diagnostic.
//!
//! Reports control-flow statements nested deeper than the configured limit.
//!
//! ## Track 2 Phase B §6.4 migration
//! Pre-migration the legacy `from_hir` adapter consumed
//! `BodyDiagnostic::NestedStatements`, which was emitted from
//! `lower::stmt::exit_nesting_stmt` once per **leaf** nesting statement.
//! That leaf-emit pattern is preserved here: the `compute_hir_metrics`
//! visitor records a `NestingLeafMetrics { stmt, depth }` entry for
//! every innermost nesting statement, and this handler replays the
//! threshold filter directly against the cached
//! `module_hir_metrics_query` data — one diagnostic per over-budget
//! leaf, attached to the leaf's first keyword (`Если` / `Пока` / `Для` /
//! `Попытка`) recovered through the parse tree.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
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

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// `HirMethodMetrics::nesting_leaves` via `ctx.module_hir_metrics()`.
/// Emits one diagnostic per leaf nesting statement whose 1-indexed
/// depth exceeds the `maxAllowedLevel` config (mirrors the legacy
/// behaviour the retired `lower::stmt::exit_nesting_stmt` produced).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::NestedStatements;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_allowed_level =
        ctx.config_int(code, "maxAllowedLevel", DEFAULT_MAX_ALLOWED_LEVEL) as u32;

    let module_metrics = ctx.module_hir_metrics();
    if module_metrics.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();
    let parse = ctx.parse();
    let root = parse.syntax_node();

    let mut out = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        if metrics.nesting_leaves.is_empty() {
            continue;
        }
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_leaves(ctx, code, &metrics, source_map, &root, max_allowed_level, &mut out);
    }
    // Module-level code: top-level Если/Пока/Для/Попытка outside any
    // method body. The legacy lowering-time emit ran for these the same
    // way it ran for method bodies — preserve that coverage by walking
    // the synthetic "module body" entry.
    if let Some(metrics) = module_metrics.module_code() {
        if !metrics.nesting_leaves.is_empty() {
            if let Some(lower_result) = module_bodies.module_code_result() {
                emit_leaves(
                    ctx,
                    code,
                    &metrics,
                    &lower_result.source_map,
                    &root,
                    max_allowed_level,
                    &mut out,
                );
            }
        }
    }
    out
}

fn emit_leaves(
    ctx: &DiagnosticsContext,
    code: DiagnosticCode,
    metrics: &hir::metrics::HirMethodMetrics,
    source_map: &hir::BodySourceMap,
    root: &SyntaxNode,
    max_allowed_level: u32,
    out: &mut Vec<Diagnostic>,
) {
    for leaf in metrics.nesting_leaves.iter() {
        if leaf.depth <= max_allowed_level {
            continue;
        }
        let Some(stmt_range) = source_map.stmt_range(leaf.stmt) else { continue };
        let keyword_range = first_nesting_keyword(root, stmt_range).unwrap_or(stmt_range);
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

/// Find the first `Если` / `Пока` / `Для` / `Попытка` keyword token
/// within `stmt_range`. Mirrors the retired
/// `lower::stmt::get_nesting_keyword_range` behaviour, recovering
/// keyword precision so the migrated diagnostic preserves the same
/// per-leaf range the legacy emit produced.
///
/// Codex round-A fix: scope the token walk to the covering syntax
/// element instead of the whole file. Without scoping the cost was
/// `O(file_tokens × violating_leaves)`, which on 50k-LOC modules with
/// many deep violations becomes an LSP-latency risk; covering-element
/// scoping caps each call at `O(stmt_size)`.
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
        assert_diagnostic_range, check_hir_diagnostic, check_hir_diagnostic_with_config,
    };
    use crate::{DiagnosticCode, DiagnosticsConfig};
    #[test]
    fn test_no_nesting() {
        let code = r#"Процедура Тест()
    Если А Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0);
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

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 0, "4 levels is the maximum allowed");
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

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();
        assert_eq!(diagnostics.len(), 1, "5 levels exceeds limit of 4");
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
        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 2, "Should find 2 diagnostics");

        assert_diagnostic_range(code, diagnostics[0], 35, 8, 12);
        assert_diagnostic_range(code, diagnostics[1], 50, 6, 10);
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
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(diagnostics.len(), 1, "With maxAllowedLevel=6, only 7-level nesting triggers");
        assert_diagnostic_range(code, diagnostics[0], 50, 6, 10);
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
        let diagnostics = check_hir_diagnostic(code);
        let nested: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::NestedStatements).collect();

        assert_eq!(nested.len(), 1, "HIR should detect 1 NestedStatements (depth 5)");
    }
}
