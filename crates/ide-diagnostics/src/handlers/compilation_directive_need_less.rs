use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::AnnotationKind;
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ApplicationModule,
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ExternalConnectionModule,
        bsl_metadata::ModuleType::ManagedApplicationModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::OrdinaryApplicationModule,
        bsl_metadata::ModuleType::RecordSetModule,
        bsl_metadata::ModuleType::SessionModule,
        bsl_metadata::ModuleType::ValueManagerModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Clumsy, MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

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
    use crate::test_utils::{check_ast_diagnostic, check_metadata_diagnostic, format_diags};
    use expect_test::expect;

    fn object_module_metadata() -> hir::ModuleMetadata {
        hir::ModuleMetadata::unknown(bsl_metadata::ModuleType::ObjectModule)
    }

    #[test]
    fn test_reports_redundant_directives_in_object_module() {
        let code = r#"Процедура ПодготовитьДанные()
КонецПроцедуры

&НаСервере
&НаСервере
Процедура СохранитьИзменения()
КонецПроцедуры

&НаСервере
Процедура ПересчитатьИтоги()
КонецПроцедуры
"#;
        let diagnostics =
            check_metadata_diagnostic(object_module_metadata(), code, |_, ctx| super::check(ctx));

        expect![[r#"
            CompilationDirectiveNeedLess @ 4:1..4:11
              message: Удалите директиву компиляции
              severity: Warning
            CompilationDirectiveNeedLess @ 5:1..5:11
              message: Удалите директиву компиляции
              severity: Warning
            CompilationDirectiveNeedLess @ 9:1..9:11
              message: Удалите директиву компиляции
              severity: Warning"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_no_directives_ok() {
        let code = "Процедура А()\nКонецПроцедуры";
        let diagnostics =
            check_metadata_diagnostic(object_module_metadata(), code, |_, ctx| super::check(ctx));
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_extension_annotations_not_reported() {
        let code = r#"
&Вместо("ОригинальныйМетод")
Процедура Расширение()
КонецПроцедуры
"#;
        let diagnostics =
            check_metadata_diagnostic(object_module_metadata(), code, |_, ctx| super::check(ctx));
        assert!(diagnostics.is_empty(), "Extension annotations should not be reported");
    }

    #[test]
    fn test_not_applicable_for_command_module() {
        let code = r#"
&НаКлиенте
Процедура ОбработкаКоманды(ПараметрКоманды, ПараметрыВыполненияКоманды)
КонецПроцедуры
"#;
        let metadata = hir::ModuleMetadata::unknown(bsl_metadata::ModuleType::CommandModule);
        let diagnostics = check_metadata_diagnostic(metadata, code, |_, ctx| super::check(ctx));
        assert!(diagnostics.is_empty(), "CommandModule should not trigger this diagnostic");
    }

    #[test]
    fn test_not_applicable_for_unknown_module() {
        let code = r#"
&НаКлиенте
Процедура Тест()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, super::check);
        assert!(diagnostics.is_empty(), "Unknown module type should not trigger this diagnostic");
    }
}
