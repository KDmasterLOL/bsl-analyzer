use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir_def::item_tree::AnnotationKind;
use ide_db::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CompilationDirectiveNeedLess;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let metadata = ctx.module_metadata();

    if !matches!(
        metadata.module_type,
        bsl_metadata::ModuleType::ApplicationModule
            | bsl_metadata::ModuleType::CommonModule
            | bsl_metadata::ModuleType::ExternalConnectionModule
            | bsl_metadata::ModuleType::ManagedApplicationModule
            | bsl_metadata::ModuleType::ManagerModule
            | bsl_metadata::ModuleType::ObjectModule
            | bsl_metadata::ModuleType::OrdinaryApplicationModule
            | bsl_metadata::ModuleType::RecordSetModule
            | bsl_metadata::ModuleType::SessionModule
            | bsl_metadata::ModuleType::ValueManagerModule
    ) {
        return Vec::new();
    }

    let item_tree = ctx.item_tree();
    let mut diagnostics = Vec::new();

    for (_, proc) in item_tree.procedures() {
        for ann in proc.annotations.iter() {
            if is_compilation_directive(ann.kind) {
                diagnostics.push(make_diagnostic(ann.range, code, ctx));
            }
        }
    }

    for (_, func) in item_tree.functions() {
        for ann in func.annotations.iter() {
            if is_compilation_directive(ann.kind) {
                diagnostics.push(make_diagnostic(ann.range, code, ctx));
            }
        }
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn is_compilation_directive(kind: AnnotationKind) -> bool {
    matches!(
        kind,
        AnnotationKind::AtClient
            | AnnotationKind::AtServer
            | AnnotationKind::AtClientAtServer
            | AnnotationKind::AtClientAtServerNoContext
            | AnnotationKind::AtServerNoContext
    )
}

fn make_diagnostic(range: TextRange, code: DiagnosticCode, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Удалите директиву компиляции".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};

    #[test]
    fn test_from_java_fixture() {
        let code = include_str!("../test_data/CompilationDirectiveNeedLessDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, super::check);

        assert_eq!(diagnostics.len(), 3, "Expected 3 diagnostics");

        assert_diagnostic_range(code, &diagnostics[0], 3, 0, 10);
        assert_diagnostic_range(code, &diagnostics[1], 4, 0, 10);
        assert_diagnostic_range(code, &diagnostics[2], 8, 0, 10);
    }

    #[test]
    fn test_no_directives_ok() {
        let code = "Процедура А()\nКонецПроцедуры";
        let diagnostics = check_ast_diagnostic(code, super::check);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_extension_annotations_not_reported() {
        let code = r#"
&Вместо("ОригинальныйМетод")
Процедура Расширение()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        assert!(diagnostics.is_empty(), "Extension annotations should not be reported");
    }
}
