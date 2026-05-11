//! Flags enabling `УстановитьПривилегированныйРежим` as a security hotspot.
//!
//! # Track 2 §1.6 Group C
//!
//! Detection moved out of HIR lowering into this handler's [`check`]
//! function, consuming the §1.2 saturating-counter lattice + §1.3 const-
//! propagation overlay through
//! [`hir::dataflow::security_state::open_events`]. Const-prop precision:
//! `Значение = Истина; УстановитьПривилегированныйРежим(Значение)` is
//! folded as `KnownTrue` and emitted; `Значение = Ложь;
//! УстановитьПривилегированныйРежим(Значение)` is folded as
//! `KnownFalse` and **suppressed** (the legacy "any non-literal-False ⇒
//! emit" coarsening over-approximated and is replaced by the lattice
//! decision). Cases that don't fold (`Перем =
//! НепрозрачнаяФункция(); Установить(Перем)`) still surface — the
//! lattice yields `Unknown`, and `open_events` treats `Unknown` as
//! "potentially opens" to preserve the legacy alarm.
//!
//! # Coverage
//!
//! Both per-method bodies AND module-level top-level code are scanned.
//! `module_security_state` carries a separate
//! [`hir::dataflow::DataflowResult`] for the module's top-level body
//! when one exists, so file-scope
//! `УстановитьПривилегированныйРежим(...)` calls outside any procedure
//! are surfaced — matching the legacy HIR-side detector's parity.

use std::sync::Arc;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::dataflow::security_state::{open_events, Category};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Track 2 §1.6 Group C — lattice-driven detection. Replaces the old
/// `from_hir(range, ctx)` adapter that consumed
/// `BodyDiagnostic::SetPrivilegedModeCall` from HIR lowering.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SetPrivilegedMode;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_security: Arc<ide_db::effects::ModuleSecurityState> = ctx.module_security_state();
    if module_security.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(result) = module_security.get(local_id) else { continue };
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_for_result(&result, source_map, code, ctx, &mut diagnostics);
    }
    if let Some(result) = module_security.module_level() {
        if let Some(lower_result) = module_bodies.module_code_result() {
            emit_for_result(&result, &lower_result.source_map, code, ctx, &mut diagnostics);
        }
    }
    // Codex round-1 NIT: emit in source order so handler tests that
    // read `diags[idx]` see deterministic output regardless of CFG
    // vertex iteration order.
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));
    diagnostics
}

fn emit_for_result(
    result: &hir::dataflow::DataflowResult<hir::dataflow::security_state::SecurityModeState>,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    out: &mut Vec<Diagnostic>,
) {
    for event in open_events(result) {
        if !matches!(event.category, Category::PrivilegedMode) {
            continue;
        }
        let Some(range) = source_map.expr_range(event.callee) else { continue };
        out.push(Diagnostic {
            code,
            message: "Проверьте установку привилегированного режима".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_from_java_fixture() {
        let code = r#"&НаСервере
Процедура Метод()
    УстановитьПривилегированныйРежим(Истина); // есть замечание
    Значение = Истина;
    УстановитьПривилегированныйРежим(Значение); // есть замечание

    УстановитьПривилегированныйРежим(Ложь); // нет замечания
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SetPrivilegedMode,
            expect![[r#"
                SetPrivilegedMode @ 3:5..3:37
                  message: Проверьте установку привилегированного режима
                  severity: Warning
                SetPrivilegedMode @ 5:5..5:37
                  message: Проверьте установку привилегированного режима
                  severity: Warning"#]],
        );
    }

    /// Track 2 §1.6 Group C — Codex round-1 MAJOR regression guard:
    /// module-level (top-level) calls outside any procedure must be
    /// surfaced. A regression here would silently lose coverage for
    /// file-scope `SetPrivilegedMode`.
    #[test]
    fn test_module_level_call_emits() {
        let code = r#"УстановитьПривилегированныйРежим(Истина);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SetPrivilegedMode,
            expect![[r#"
                SetPrivilegedMode @ 1:1..1:33
                  message: Проверьте установку привилегированного режима
                  severity: Warning"#]],
        );
    }
}
