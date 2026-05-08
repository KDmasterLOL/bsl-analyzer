//! OrdinaryAppSupport diagnostic.
//!
//! Validates ordinary-application support settings in configuration metadata.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::OrdinaryAppSupport;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !ctx.config.ordinary_app_support {
        return Vec::new();
    }

    if !is_session_module(ctx) {
        return Vec::new();
    }

    let configuration = match ctx.main_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let mut diagnostics = Vec::new();
    let range = get_module_range(ctx);

    if !configuration.use_managed_form_in_ordinary_application() {
        diagnostics.push(Diagnostic {
            code,
            message: "Установите свойство \"Использовать управляемые формы в обычном приложении\" \
                      в Истина"
                .to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    if configuration.use_ordinary_form_in_managed_application() {
        diagnostics.push(Diagnostic {
            code,
            message: "Установите свойство \"Использовать обычные формы в управляемом приложении\" \
                      в Ложь"
                .to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    diagnostics
}

fn is_session_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    file_path.ends_with("/Ext/SessionModule.bsl") || file_path.ends_with("\\Ext\\SessionModule.bsl")
}

fn get_module_range(ctx: &DiagnosticsContext) -> TextRange {
    let file_text = ctx.file_text();
    // Use byte offset for 14 chars
    let end_offset: usize = file_text.chars().take(14).map(|c| c.len_utf8()).sum();
    TextRange::new(0.into(), (end_offset as u32).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_with_config(
        code: &str,
        fixtures_dir: &str,
        is_session_module: bool,
        ordinary_app_support: bool,
    ) -> (Vec<Diagnostic>, String) {
        let mut db = RootDatabaseImpl::new();

        let workspace_root = PathBuf::from(fixtures_dir);
        let mut file_set = FileSet::default();

        let file_id = FileId(0);
        let file_path = if is_session_module {
            format!("{}/Ext/SessionModule.bsl", fixtures_dir)
        } else {
            format!("{}/CommonModules/Test/Ext/Module.bsl", fixtures_dir)
        };
        let vfs_path = VfsPath::new(file_path);
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::new(&db, Some(configuration_path_input));
        let config = DiagnosticsConfig { ordinary_app_support, ..Default::default() };

        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_session_module_with_bad_settings() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = r#"Процедура ПриОпределенииПараметровСеанса()

КонецПроцедуры
"#;
        let (diagnostics, file_content) = check_with_config(code, fixtures_dir, true, true);

        // Designer fixture has both flags set to false (default),
        // so we expect 1 diagnostic for UseManagedFormInOrdinaryApplication = false
        // (UseOrdinaryFormInManagedApplication = false is correct, so no diagnostic)
        assert!(
            !diagnostics.is_empty(),
            "Expected at least 1 diagnostic, found {}",
            diagnostics.len()
        );

        for diagnostic in &diagnostics {
            assert_diagnostic_range(&file_content, diagnostic, 0, 0, 14);
            assert_eq!(diagnostic.code, DiagnosticCode::OrdinaryAppSupport);
        }
    }

    #[test]
    fn test_disabled_config() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = r#"Процедура ПриОпределенииПараметровСеанса()

КонецПроцедуры
"#;
        let (diagnostics, _) = check_with_config(code, fixtures_dir, true, false);

        assert_eq!(diagnostics.len(), 0, "Disabled config should produce no diagnostics");
    }

    #[test]
    fn test_not_session_module() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = r#"Процедура ПриОпределенииПараметровСеанса()

КонецПроцедуры
"#;
        let (diagnostics, _) = check_with_config(code, fixtures_dir, false, true);

        assert_eq!(diagnostics.len(), 0, "Non-SessionModule should produce no diagnostics");
    }
}
