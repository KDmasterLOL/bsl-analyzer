//! PrivilegedModuleMethodCall diagnostic.
//!
//! Detects calls to public methods of privileged CommonModules.
//!
//! ## What it checks
//!
//! Privileged modules run with elevated rights, bypassing RLS and role checks.
//! Calls to their methods require review to exclude security vulnerabilities.
//!
//! ## Parameters
//!
//! - `validateNestedCalls` (bool, default: true) - If false, calls from within
//!   a privileged module to itself are not flagged.
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//! - **Type:** SECURITY_HOTSPOT
//!
//! ## Reference
//!

use bsl_metadata::traits::MdObject;

use crate::define_metadata;
use crate::metadata::*;
use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{ExternalRef, ModuleId, PathResolution};
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 60,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_VALIDATE_NESTED_CALLS: bool = true;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::PrivilegedModuleMethodCall;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let validate_nested_calls = ctx
        .config
        .get_bool(DiagnosticCode::PrivilegedModuleMethodCall, "validateNestedCalls")
        .unwrap_or(DEFAULT_VALIDATE_NESTED_CALLS);

    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let privileged_modules: FxHashSet<String> = configuration
        .common_modules()
        .iter()
        .filter(|m| m.is_privileged())
        .map(|m| m.name().to_lowercase())
        .collect();

    if privileged_modules.is_empty() {
        return Vec::new();
    }

    let current_module_name =
        common_module_helpers::find_common_module_for_file(ctx, &configuration)
            .map(|m| m.name().to_string());

    let is_current_privileged = current_module_name
        .as_ref()
        .is_some_and(|name| privileged_modules.contains(&name.to_lowercase()));

    if !validate_nested_calls && is_current_privileged {
        return Vec::new();
    }

    let module_id = ModuleId::new(ctx.file_id);
    let external_refs = ctx.db.file_external_refs(module_id);

    let mut diagnostics = Vec::new();

    for external_ref in external_refs.iter() {
        if let ExternalRef::QualifiedCall { receiver, method, range } = external_ref {
            let receiver_lower = receiver.as_str().to_lowercase();

            if !privileged_modules.contains(&receiver_lower) {
                continue;
            }

            if !validate_nested_calls {
                if let Some(ref current) = current_module_name {
                    if current.to_lowercase() == receiver_lower {
                        continue;
                    }
                }
            }

            let resolution = ctx.resolve_qualified_path(receiver, method);
            if matches!(resolution, PathResolution::Method(_)) {
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Проверьте обращение к методу {} привилегированного модуля",
                        method.as_str()
                    ),
                    severity: ctx.severity(code),
                    range: *range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{FileId, FileSet, VfsPath};
    #[test]
    fn test_no_metadata_returns_empty() {
        let code = r#"
Процедура Тест()
    ПривилегированныйМодуль.Метод();
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
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

    #[test]
    fn test_disabled_config() {
        let mut db = RootDatabaseImpl::new();

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let vfs_path = VfsPath::new("/test/Module.bsl");
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "");

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::PrivilegedModuleMethodCall);

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
}
