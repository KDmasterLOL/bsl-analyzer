use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Sql],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub(crate) fn dispatch(
    config: &DiagnosticsConfig,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::AmbiguousQualifiedHead { head, offered_by, range } = diag {
        // Same code as the bare-column case: to the reader both are "this name does not identify
        // one thing". The fix differs, so the message does.
        let code = DiagnosticCode::AmbiguousFieldInQuery;
        diagnostics.push(Diagnostic {
            code,
            message: sdbl_hir::ambiguous_qualified_head_message(head, offered_by),
            severity: config.severity(code),
            range: mapper.map_range(*range, query_text),
            tags: config.tags(code),
            fixes: vec![],
        });
    }

    if let sdbl_hir::SdblDiagnostic::AmbiguousColumnRef { column_name, possible_tables, range } =
        diag
    {
        // Naming the candidates is the whole value: the fix is to qualify the column, and the
        // author cannot pick a qualifier without knowing which sources offer the name.
        let code = DiagnosticCode::AmbiguousFieldInQuery;
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Поле \"{}\" неоднозначно: оно есть в таблицах {}. Укажите источник явно",
                column_name,
                possible_tables.join(", "),
            ),
            severity: config.severity(code),
            range: mapper.map_range(*range, query_text),
            tags: config.tags(code),
            fixes: vec![],
        });
    }
}

pub fn check(ctx: &crate::DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::AmbiguousFieldInQuery,
        dispatch,
    )
}
