//! ProtectedModule diagnostic.
//!
//! Detects password-protected CommonModules in the configuration.
//!
//! ## What it checks
//!
//! When a CommonModule is protected by password, its source code is replaced
//! with a `.bin` file instead of `.bsl`. This diagnostic reports all protected
//! modules found in the configuration.
//!
//! ## Scope
//!
//! This diagnostic only runs for SessionModule (`/Ext/SessionModule.bsl`).
//! One diagnostic is created for each protected module.
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//!
//! ## Reference
//!
//! Ported from ProtectedModuleDiagnostic.java (bsl-language-server)

use bsl_metadata::traits::{MdObject, Module};
use ide_db::TextRange;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::SessionModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Suspicious],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ProtectedModule;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !is_session_module(ctx) {
        return Vec::new();
    }

    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let diagnostic_range = get_diagnostic_range(ctx);
    let mut diagnostics = Vec::new();

    for common_module in configuration.common_modules() {
        if common_module.is_protected() {
            let mdo_ref = format!("ОбщийМодуль.{}", common_module.name());
            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Исходный код модуля отсутствует из-за защиты паролем. {}",
                    mdo_ref
                ),
                severity: ctx.severity(code),
                range: diagnostic_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
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

fn get_diagnostic_range(ctx: &DiagnosticsContext) -> TextRange {
    let file_text = ctx.file_text();
    let file_len = file_text.len();
    let end_offset = std::cmp::min(9, file_len);
    TextRange::new(0.into(), (end_offset as u32).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};
    #[test]
    fn test_not_session_module_returns_empty() {
        let code = r#"
Процедура Метод()
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        // Not a SessionModule path
        let vfs_path = VfsPath::new("/test/CommonModules/Module1/Ext/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let config = DiagnosticsConfig::default();

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "Not session module should return empty diagnostics");
    }

    #[test]
    fn test_disabled_config_returns_empty() {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Ext/SessionModule.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "");

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::ProtectedModule);

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "Disabled config should return empty diagnostics");
    }

    #[test]
    fn test_no_metadata_returns_empty() {
        let code = r#"
Процедура Метод()
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Ext/SessionModule.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        let config = DiagnosticsConfig::default();

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = check(&ctx);
        assert!(diagnostics.is_empty(), "No metadata should return empty diagnostics");
    }
}
