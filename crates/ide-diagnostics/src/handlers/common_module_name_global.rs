//! CommonModuleNameGlobal diagnostic
//!
//! Global CommonModules must contain "Global" or "Глобальный" in their name.
//!
//! Ported from: CommonModuleNameGlobalDiagnostic.java (bsl-language-server)
//!
//! ## Severity
//! MAJOR (Warning)
//!
//! ## Tags
//! STANDARD, BADPRACTICE, UNPREDICTABLE

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleNameGlobal) {
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

    if !module.is_global() {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("глобальный") || name_lower.contains("global") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameGlobal,
        message: "Имя глобального общего модуля должно содержать 'Глобальный' или 'Global'"
            .to_string(),
        severity: Severity::Warning,
        range: TextRange::empty(0.into()),
        tags: vec![],
        fixes: vec![],
    }]
}

/// Check metadata-based diagnostics using ModuleMetadata.
///
/// This is the new metadata-driven version that uses HIR-collected metadata
/// instead of loading configuration for each file.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    _config: &crate::DiagnosticsConfig,
) -> Vec<Diagnostic> {
    // Only check CommonModules
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if !module.is_global() {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    if name_lower.contains("глобальный") || name_lower.contains("global") {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameGlobal,
        message: "Имя глобального общего модуля должно содержать 'Глобальный' или 'Global'"
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
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let _config = DiagnosticsConfig::default();
        let _ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &_config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        if !module.is_global() {
            return Vec::new();
        }

        let name_lower = module.name().to_lowercase();
        if name_lower.contains("глобальный") || name_lower.contains("global") {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleNameGlobal,
            message: "Имя глобального общего модуля должно содержать 'Глобальный' или 'Global'"
                .to_string(),
            severity: Severity::Warning,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_global_without_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(true).build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_global_with_russian_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module =
            bsl_metadata::CommonModule::builder().name("ЧтоТоГлобальный").global(true).build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_global_with_english_keyword() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module =
            bsl_metadata::CommonModule::builder().name("SomethingGlobal").global(true).build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_not_global() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(false).build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_global_without_keyword() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleNameGlobal);
    }

    #[test]
    fn test_from_metadata_global_with_russian_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("ЧтоТоГлобальный").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_global_with_english_keyword() {
        let module =
            bsl_metadata::CommonModule::builder().name("SomethingGlobal").global(true).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_not_global() {
        let module = bsl_metadata::CommonModule::builder().name("ЧтоТо").global(false).build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);

        assert_eq!(diagnostics.len(), 0);
    }
}
