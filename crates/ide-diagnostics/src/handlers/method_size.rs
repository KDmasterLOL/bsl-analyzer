//! MethodSize diagnostic.
//!
//! Detects functions and procedures with excessive line count.
//!
//! ## Track 2 Phase B §6.4 migration
//! Pre-migration the legacy `from_hir` adapter consumed
//! `BodyDiagnostic::MethodSize`, which was emitted from
//! `lower::mod::emit_method_scoped_diagnostics` after computing
//! `(end_line - start_line) - 4` from the method's syntax range and
//! the file `LineIndex`. The migrated handler now reads the cached
//! `HirMethodMetrics::size_lines` populated by the Salsa wrapper from
//! `LowerResult::size_lines` (the only entry point that has
//! `LineIndex` access).

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

/// Track 2 Phase B §6.4 — handler-side detection consuming the cached
/// `HirMethodMetrics::size_lines` via `ctx.module_hir_metrics()`.
/// Mirrors the legacy emit-from-`emit_method_scoped_diagnostics`
/// behaviour: skip methods with `size_lines == 0` (unmeasurable —
/// `LineIndex` was unavailable during lowering, e.g. streaming-mode
/// tests), skip when size <= configured threshold.
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

    // `iter_bodies()` walks an `FxHashMap` (non-deterministic order);
    // sort by `local_id` so handler output stays in source order — the
    // legacy lowering-time emit produced lexical order, and consumer
    // tests assert the resulting diagnostic ordering.
    let mut local_ids: Vec<u32> = module_bodies.iter_bodies().map(|(id, _)| id).collect();
    local_ids.sort_unstable();

    let mut out = Vec::new();
    for local_id in local_ids {
        let Some(metrics) = module_metrics.get(local_id) else { continue };
        // Legacy guard: `if method_size > 0` — without `LineIndex` the
        // size is meaningless (`size_lines == 0`), so skip.
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
    use crate::test_utils::{check_hir_diagnostic, check_hir_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};

    fn make_method_size_code() -> String {
        // Large inline regression fixture for method-size coverage.
        //   Line 0:   Процедура ПустаяПроцедура()
        //   Line 1:   (empty)
        //   Line 2:   КонецПроцедуры
        //   Line 3:   (empty)
        //   Line 4:   Функция ФункцияВОднуСтроку() КонецФункции
        //   Line 5:   (empty)
        //   Line 6:   Процедура Процедура201Строка()   <- diagnostic here
        //   Lines 7-208: body (201 lines from start to КонецПроцедуры)
        //   Line 209:   (empty) [after end line 208]
        //   ...
        //   Line 419:  Функция Функция201Строка()       <- diagnostic here
        //
        // Sizes are computed as stop_line - start_line.
        // Процедура201Строка: lines 6-212 → size 206? No — we need size=201.
        // Original has 201 А=0 statements, making the method span 203 lines (start + 201 body + end).
        // Size = stop_line - start_line = 202 lines diff but reports 201.
        // We replicate the exact file structure: same line counts as original .bsl.

        let mut s = String::new();
        // Lines 0-2: ПустаяПроцедура
        s.push_str("Процедура ПустаяПроцедура()\n\n КонецПроцедуры\n\n");
        // Line 4: ФункцияВОднуСтроку
        s.push_str("Функция ФункцияВОднуСтроку() КонецФункции\n\n");
        // Процедура201Строка: size = (end_line - start_line) - 4 must be > 200.
        // Structure: declaration + blank + N body lines + blank + end
        // end_line - start_line = N + 3, size = N + 3 - 4 = N - 1.
        // For size=201: N=202 body lines.
        s.push_str("Процедура Процедура201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        // Процедура200Строк: size=200, should NOT trigger at default threshold 200.
        // For size=200: N=201 body lines.
        s.push_str("Процедура Процедура200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        // Функция201Строка: size=201, N=202 body lines.
        s.push_str("Функция Функция201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        // Функция200Строк: size=200, N=201 body lines.
        s.push_str("Функция Функция200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        // А(А=0) — one-liner with default param
        s.push_str("Функция А(А=0)\n\n КонецФункции\n");
        s
    }

    #[test]
    fn test_comprehensive() {
        let code = make_method_size_code();
        let diagnostics = check_hir_diagnostic(&code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();

        assert_eq!(diagnostics.len(), 2, "Should find 2 diagnostics");

        assert_eq!(diagnostics[0].code, DiagnosticCode::MethodSize);
        assert_eq!(diagnostics[0].severity, Severity::Warning); // CodeSmell + Major -> Warning
        assert!(
            diagnostics[0].message.contains("Процедура201Строка"),
            "Message should contain method name, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("201"),
            "Message should contain size 201, got: {}",
            diagnostics[0].message
        );
        assert!(diagnostics[1].message.contains("Функция201Строка"));
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
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 4, "Should find 4 diagnostics with threshold 20");
    }

    #[test]
    fn test_empty_method() {
        let code = r#"Процедура Пустая()

КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 0, "Empty method should not trigger");
    }

    #[test]
    fn test_one_liner() {
        let code = r#"Функция Тест() КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 0, "One-liner should not trigger");
    }
}
