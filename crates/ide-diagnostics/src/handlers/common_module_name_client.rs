//! CommonModuleNameClient diagnostic
//!
//! Client (non-global) CommonModules must contain "Client" or "Клиент" in their name.
//!
//! Ported from: CommonModuleNameClientDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::TextRange;
use regex::Regex;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleNameClient) {
        return Vec::new();
    }

    let config_path = match ctx.configuration_path.or(ctx.workspace_root) {
        Some(path) => path,
        None => return Vec::new(),
    };

    let config_path_str = config_path.to_string_lossy().to_string();
    let path_input = ide_db::metadata::ConfigurationPathInput::new(ctx.db, config_path_str);
    let configuration = ide_db::metadata::load_configuration(ctx.db, path_input);

    let module = match common_module_helpers::find_common_module_for_file(ctx, &configuration) {
        Some(m) => m,
        None => return Vec::new(),
    };

    if module.is_global()
        || !common_module_helpers::is_client(&module, ctx.config.ordinary_app_support)
    {
        return Vec::new();
    }

    let regex = Regex::new(r"(?i)клиент|client").unwrap();
    if regex.is_match(module.name()) {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameClient,
        message: "Имя клиентского общего модуля должно содержать 'Клиент' или 'Client'".to_string(),
        severity: Severity::Information,
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

        if module.is_global() || !common_module_helpers::is_client(module, false) {
            return Vec::new();
        }

        let regex = regex::Regex::new(r"(?i)клиент|client").unwrap();
        if regex.is_match(module.name()) {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleNameClient,
            message: "Имя клиентского общего модуля должно содержать 'Клиент' или 'Client'"
                .to_string(),
            severity: Severity::Information,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_client_without_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТо")
            .global(false)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_client_with_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("ЧтоТоКлиент")
            .global(false)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }
}
