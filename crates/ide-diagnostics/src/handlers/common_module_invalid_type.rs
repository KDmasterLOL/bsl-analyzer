//! CommonModuleInvalidType diagnostic
//!
//! CommonModule must be one of four types: Server, ServerCall, Client, ClientServer.
//!
//! Ported from: CommonModuleInvalidTypeDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleInvalidType) {
        return Vec::new();
    }

    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let module = match common_module_helpers::find_common_module_for_file(ctx, &configuration) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let is_valid = common_module_helpers::is_server(&module, ctx.config.ordinary_app_support)
        || common_module_helpers::is_server_call(&module)
        || common_module_helpers::is_client(&module, ctx.config.ordinary_app_support)
        || common_module_helpers::is_client_server(&module, ctx.config.ordinary_app_support);

    if is_valid {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleInvalidType,
        message:
            "Общий модуль должен быть одного из типов: Server, ServerCall, Client, ClientServer"
                .to_string(),
        severity: Severity::Warning,
        range: TextRange::empty(0.into()),
        tags: vec![],
        fixes: vec![],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_with_module(code: &str, module: &bsl_metadata::CommonModule) -> Vec<Diagnostic> {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let _db = Rc::new(db) as Rc<dyn RootDatabase>;

        let _config = DiagnosticsConfig::default();

        let is_valid = common_module_helpers::is_server(module, false)
            || common_module_helpers::is_server_call(module)
            || common_module_helpers::is_client(module, false)
            || common_module_helpers::is_client_server(module, false);

        if is_valid {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleInvalidType,
            message:
                "Общий модуль должен быть одного из типов: Server, ServerCall, Client, ClientServer"
                    .to_string(),
            severity: Severity::Warning,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_invalid_type() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder().name("Something").build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_valid_server_call() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }
}
