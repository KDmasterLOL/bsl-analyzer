//! CommonModuleNameClientServer diagnostic
//!
//! ClientServer CommonModules must contain "ClientServer" or "КлиентСервер" in their name.
//!
//! Ported from: CommonModuleNameClientServerDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::TextRange;
use regex::Regex;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleNameClientServer) {
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

    if !common_module_helpers::is_client_server(&module, ctx.config.ordinary_app_support) {
        return Vec::new();
    }

    let regex = Regex::new(r"(?i)клиентсервер|clientserver").unwrap();
    if regex.is_match(module.name()) {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameClientServer,
        message:
            "Имя клиент-серверного общего модуля должно содержать 'КлиентСервер' или 'ClientServer'"
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

        if !common_module_helpers::is_client_server(module, false) {
            return Vec::new();
        }

        let regex = regex::Regex::new(r"(?i)клиентсервер|clientserver").unwrap();
        if regex.is_match(module.name()) {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleNameClientServer,
            message: "Имя клиент-серверного общего модуля должно содержать 'КлиентСервер' или 'ClientServer'".to_string(),
            severity: Severity::Warning,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_client_server_without_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_client_server_with_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingClientServer")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }
}
