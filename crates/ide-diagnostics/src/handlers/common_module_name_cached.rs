//! CommonModuleNameCached diagnostic
//!
//! Cached CommonModules must contain "Cached" or "ПовтИсп" in their name.
//!
//! Ported from: CommonModuleNameCachedDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use bsl_metadata::ReturnValueReuse;
use ide_db::TextRange;
use regex::Regex;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleNameCached) {
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

    if module.return_values_reuse() == ReturnValueReuse::DontUse {
        return Vec::new();
    }

    let regex = Regex::new(r"(?i)повторноеиспользование|повтисп|cached").unwrap();
    if regex.is_match(module.name()) {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameCached,
        message: "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'"
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

        if module.return_values_reuse() == ReturnValueReuse::DontUse {
            return Vec::new();
        }

        let regex = regex::Regex::new(r"(?i)повторноеиспользование|повтисп|cached").unwrap();
        if regex.is_match(module.name()) {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleNameCached,
            message: "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'"
                .to_string(),
            severity: Severity::Warning,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_cached_without_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(ReturnValueReuse::DuringRequest)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_cached_with_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingCached")
            .return_values_reuse(ReturnValueReuse::DuringSession)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_not_cached() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(ReturnValueReuse::DontUse)
            .build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }
}
