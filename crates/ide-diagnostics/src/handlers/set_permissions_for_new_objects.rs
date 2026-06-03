use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::ManagedApplicationModule],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

const DEFAULT_FULL_ACCESS_ROLES: &str = "FullAccess,ПолныеПрава";

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SetPermissionsForNewObjects;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !is_managed_application_module(ctx) {
        return Vec::new();
    }

    let configuration = match ctx.main_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let allowed_roles = get_allowed_roles(ctx);

    let mut diagnostics = Vec::new();

    for role in configuration.roles() {
        if role.data().set_for_new_objects() && !allowed_roles.contains(role.name()) {
            diagnostics.push(create_diagnostic(ctx, role.name(), code));
        }
    }

    diagnostics
}

fn is_managed_application_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    file_path.ends_with("/Ext/ManagedApplicationModule.bsl")
        || file_path.ends_with("\\Ext\\ManagedApplicationModule.bsl")
}

fn get_allowed_roles(ctx: &DiagnosticsContext) -> FxHashSet<String> {
    let names_str = ctx
        .config
        .get_string(DiagnosticCode::SetPermissionsForNewObjects, "namesFullAccessRole")
        .unwrap_or(DEFAULT_FULL_ACCESS_ROLES);

    names_str.split(',').map(|s| s.trim().to_string()).collect()
}

fn create_diagnostic(
    ctx: &DiagnosticsContext,
    role_name: &str,
    code: DiagnosticCode,
) -> Diagnostic {
    let message = format!(
        "У роли \"{}\" не должен быть установлен флаг \"Устанавливать права для новых объектов\"",
        role_name
    );

    let file_text = ctx.file_text();
    let file_len = file_text.len();

    let end_offset = std::cmp::min(9, file_len);
    let range = TextRange::new(0.into(), (end_offset as u32).into());

    Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::format_diags;
    use crate::DiagnosticsConfig;
    use expect_test::expect;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_diagnostic(code: &str, fixtures_dir: &str) -> (Vec<Diagnostic>, String) {
        let mut db = RootDatabaseImpl::new();

        let workspace_root = PathBuf::from(fixtures_dir);

        let mut file_set = FileSet::default();

        let file_id = FileId(0);
        let module_path =
            VfsPath::new(format!("{}/Ext/ManagedApplicationModule.bsl", fixtures_dir));
        file_set.insert(file_id, module_path);

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
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_set_permissions_for_new_objects() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        let code = "//test - ManagedApplicationModule";
        let (diagnostics, file_content) = check_diagnostic(code, fixtures_dir);

        expect![[r#"
            SetPermissionsForNewObjects @ 1:1..1:10
              message: У роли "Роль1" не должен быть установлен флаг "Устанавливать права для новых объектов"
              severity: Critical"#]].assert_eq(&format_diags(&file_content, &diagnostics));
    }

    #[test]
    fn test_not_managed_application_module() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        let mut db = RootDatabaseImpl::new();

        let vfs_path = VfsPath::new(format!("{}/CommonModules/Test/Ext/Module.bsl", fixtures_dir));

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "Процедура Тест()\nКонецПроцедуры");

        let provider = ide_db::SalsaProvider::new(&db, None);
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        expect![[r#""#]].assert_eq(&format_diags("Процедура Тест()\nКонецПроцедуры", &diagnostics));
    }

    #[test]
    fn test_custom_allowed_roles() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        let mut db = RootDatabaseImpl::new();

        let workspace_root = PathBuf::from(fixtures_dir);

        let mut file_set = FileSet::default();

        let file_id = FileId(0);
        let module_path =
            VfsPath::new(format!("{}/Ext/ManagedApplicationModule.bsl", fixtures_dir));
        file_set.insert(file_id, module_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "//test");

        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::new(&db, Some(configuration_path_input));

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::SetPermissionsForNewObjects,
            serde_json::json!({"namesFullAccessRole": "Роль2"}),
        );

        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        expect![[r#"
            SetPermissionsForNewObjects @ 1:1..1:7
              message: У роли "ПолныеПрава" не должен быть установлен флаг "Устанавливать права для новых объектов"
              severity: Critical
            SetPermissionsForNewObjects @ 1:1..1:7
              message: У роли "Роль1" не должен быть установлен флаг "Устанавливать права для новых объектов"
              severity: Critical"#]].assert_eq(&format_diags("//test", &diagnostics));
    }
}
