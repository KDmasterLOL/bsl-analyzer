//! ExecuteExternalCodeInCommonModule diagnostic.
//!
//! Detects usage of Execute/Eval in CommonModules that run on server,
//! external connection, or ordinary client application context.
//!
//! ## Severity
//! CRITICAL (SECURITY_HOTSPOT)
//!
//! ## What it checks
//! Execute and Eval calls in CommonModules with server/externalConnection/clientOrdinary flags.
//! Unlike ExecuteExternalCode, this diagnostic does NOT check method annotations -
//! it triggers based on module context only.
//!
//! ## Reference
//! - 1C Standard: https://its.1c.ru/db/v8std#content:770:hdoc

use crate::define_metadata;
use crate::metadata::*;
use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ExecuteExternalCodeInCommonModule;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // CFE-aware "is *this file* a CommonModule with executable scope?":
    // we don't care which configuration declares it, only that the file is
    // a CommonModule whose flags match `should_check_module`.
    let module = match common_module_helpers::find_common_module_for_file_anywhere(ctx) {
        Some(m) => m,
        None => return Vec::new(),
    };

    if !should_check_module(&module, ctx.config.ordinary_app_support) {
        return Vec::new();
    }

    detect_violations(ctx)
}

fn should_check_module(module: &bsl_metadata::CommonModule, ordinary_app_support: bool) -> bool {
    module.is_server()
        || module.is_external_connection()
        || (ordinary_app_support && module.is_client_ordinary_application())
}

fn detect_violations(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ExecuteExternalCodeInCommonModule;
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::EXECUTE_STMT => {
                diagnostics.push(create_diagnostic(code, node.text_range(), ctx));
            }
            SyntaxKind::CALL_EXPR if is_global_eval_call(&node) => {
                diagnostics.push(create_diagnostic(code, node.text_range(), ctx));
            }
            _ => {}
        }
    }

    diagnostics
}

fn create_diagnostic(
    code: DiagnosticCode,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message:
            "Execution of external code in a common module on a server is a potential vulnerability"
                .to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

fn is_global_eval_call(node: &syntax::SyntaxNode) -> bool {
    let first_token = match node.first_token() {
        Some(t) => t,
        None => return false,
    };

    if first_token.kind() != SyntaxKind::IDENT {
        return false;
    }

    if let Some(prev) = first_token.prev_token() {
        if prev.kind() == SyntaxKind::DOT {
            return false;
        }
    }

    // Track 2 §1.6: registry-driven recognition (`Category::ExecuteExternalCode`,
    // `EntryKind::GlobalMethod`). The curated registry covers `Eval` /
    // `Вычислить` and any future bilingual aliases as a single source of
    // truth, replacing the hardcoded `name == "eval" || name == "вычислить"`
    // pair. The `EXECUTE_STMT` (`Выполнить`) match in `detect_violations`
    // stays a SyntaxKind branch — it's not a name-based match.
    bsl_platform::security::registry().lookup_global(first_token.text()).is_some_and(|e| {
        matches!(e.category, bsl_platform::security::Category::ExecuteExternalCode)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::rc::Rc;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_violations_directly(code: &str) -> Vec<Diagnostic> {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, VfsPath::new("/test/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);

        let config = Rc::new(DiagnosticsConfig::all_enabled());
        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        detect_violations(&ctx)
    }

    #[test]
    fn test_detect_execute_statement() {
        let code = r#"
Процедура ВыполнитьПроизвольныйКод(Строка)
    Выполнить(Строка);
КонецПроцедуры

Функция РассчитатьЧтоТоИзСтроки(Строка)
    Возврат Вычислить(Строка);
КонецФункции

Функция БезОшибок(Строка)
    Возврат ВычислитьЧтоТо(Строка);
КонецФункции

"#;
        let diagnostics = check_violations_directly(code);

        assert_eq!(diagnostics.len(), 2, "Expected 2 violations: Execute and Eval");

        assert_diagnostic_range(code, &diagnostics[0], 2, 4, 22);
        assert_diagnostic_range(code, &diagnostics[1], 6, 12, 29);
    }

    #[test]
    fn test_no_configuration_returns_empty() {
        let code = r#"
Процедура Тест()
    Выполнить(Строка);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "No configuration should return empty");
    }

    #[test]
    fn test_qualified_eval_ignored() {
        let code = r#"
Функция ВычислитьЗначение(Объект)
    Возврат Объект.Вычислить();
КонецФункции
"#;
        let diagnostics = check_violations_directly(code);
        assert_eq!(diagnostics.len(), 0, "Qualified Eval calls should be ignored");
    }

    #[test]
    fn test_similar_method_name_ignored() {
        let code = r#"
Функция БезОшибок(Строка)
    Возврат ВычислитьЧтоТо(Строка);
КонецФункции
"#;
        let diagnostics = check_violations_directly(code);
        assert_eq!(diagnostics.len(), 0, "Similar method names should be ignored");
    }

    #[test]
    fn test_should_check_server_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ТестовыйМодуль")
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        assert!(should_check_module(&module, true));
    }

    #[test]
    fn test_should_check_external_connection_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ТестовыйМодуль")
            .server(false)
            .external_connection(true)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        assert!(should_check_module(&module, true));
    }

    #[test]
    fn test_should_check_ordinary_client_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ТестовыйМодуль")
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(false)
            .build();

        assert!(should_check_module(&module, true));
        assert!(!should_check_module(&module, false));
    }

    #[test]
    fn test_should_not_check_client_managed_module() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ТестовыйМодуль")
            .server(false)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(true)
            .build();

        assert!(!should_check_module(&module, true));
        assert!(!should_check_module(&module, false));
    }

    #[test]
    fn test_disabled_config() {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, VfsPath::new("/test/Module.bsl"));

        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "");

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::ExecuteExternalCodeInCommonModule);
        let config = Rc::new(config);

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "Disabled config should return empty diagnostics");
    }
}
