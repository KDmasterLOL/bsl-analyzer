//! SetPermissionsForNewObjects diagnostic.
//!
//! Validates that roles do not have "Set permissions for new objects" flag enabled.
//!
//! ## What it checks
//!
//! This diagnostic validates that roles (except explicitly allowed ones like ПолныеПрава/FullAccess)
//! do not have the "setForNewObjects" flag enabled in their Rights.xml.
//!
//! ## Why?
//!
//! When this flag is enabled, the role automatically gets permissions for any newly created
//! metadata objects. This is a security vulnerability for non-admin roles, as it may grant
//! unintended access to sensitive data.
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL (VULNERABILITY)
//! - **Tags:** STANDARD, BADPRACTICE, DESIGN
//! - **Minutes to fix:** 1
//!
//! ## Parameter
//!
//! - `namesFullAccessRole` (string, default: "FullAccess,ПолныеПрава") - comma-separated list
//!   of role names that are allowed to have this flag enabled
//!
//! ## Scope
//!
//! This diagnostic only runs for **ManagedApplicationModule** files.
//! All diagnostics are reported at the beginning of the module (line 1, columns 1-9).
//!
//! ## Reference
//!
//! Ported from:

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

/// Default allowed role names (can have setForNewObjects=true)
const DEFAULT_FULL_ACCESS_ROLES: &str = "FullAccess,ПолныеПрава";

/// Main entry point for SetPermissionsForNewObjects diagnostic.
///
/// Validates that roles don't have "Set permissions for new objects" flag enabled
/// (except for explicitly allowed roles).
///
/// ## Algorithm
///
/// 1. Early return if disabled or not ManagedApplicationModule
/// 2. Load Configuration metadata via Salsa (cached!)
/// 3. Get allowed roles from config parameter
/// 4. For each role with setForNewObjects=true that is NOT in allowed list, create diagnostic
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::SetPermissionsForNewObjects;

    // 1. Check if disabled
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // 2. ManagedApplicationModule-only scope
    if !is_managed_application_module(ctx) {
        return Vec::new();
    }

    // 3. Load configuration metadata
    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    // 4. Get allowed role names from config parameter
    let allowed_roles = get_allowed_roles(ctx);

    // 5. Find roles with setForNewObjects=true that are NOT in allowed list
    let mut diagnostics = Vec::new();

    for role in configuration.roles() {
        if role.data().set_for_new_objects() && !allowed_roles.contains(role.name()) {
            diagnostics.push(create_diagnostic(ctx, role.name(), code));
        }
    }

    diagnostics
}

/// Check if current file is ManagedApplicationModule
fn is_managed_application_module(ctx: &DiagnosticsContext) -> bool {
    let file_path = match ctx.file_path() {
        Some(path) => path,
        None => return false,
    };

    file_path.ends_with("/Ext/ManagedApplicationModule.bsl")
        || file_path.ends_with("\\Ext\\ManagedApplicationModule.bsl")
}

/// Get allowed role names from config parameter
fn get_allowed_roles(ctx: &DiagnosticsContext) -> FxHashSet<String> {
    let names_str = ctx
        .config
        .get_string(DiagnosticCode::SetPermissionsForNewObjects, "namesFullAccessRole")
        .unwrap_or(DEFAULT_FULL_ACCESS_ROLES);

    names_str.split(',').map(|s| s.trim().to_string()).collect()
}

/// Create diagnostic with Russian error message
///
/// All diagnostics are reported at the ManagedApplicationModule start (line 1, columns 1-9).
fn create_diagnostic(
    ctx: &DiagnosticsContext,
    role_name: &str,
    code: DiagnosticCode,
) -> Diagnostic {
    let message = format!(
        "У роли \"{}\" не должен быть установлен флаг \"Устанавливать права для новых объектов\"",
        role_name
    );

    // Get file text to determine safe range
    let file_text = ctx.file_text();
    let file_len = file_text.len();

    // Use range [0, min(9, file_len))
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
    use crate::test_utils::assert_diagnostic_range;
    use crate::{DiagnosticsConfig, Severity};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};
    fn check_diagnostic(code: &str, fixtures_dir: &str) -> (Vec<Diagnostic>, String) {
        // Setup database with VFS
        let mut db = RootDatabaseImpl::new();

        // Create VFS
        let workspace_root = PathBuf::from(fixtures_dir);

        // Create FileSet with ManagedApplicationModule
        let mut file_set = FileSet::default();

        // ManagedApplicationModule file (file_id 0)
        let file_id = FileId(0);
        let module_path =
            VfsPath::new(format!("{}/Ext/ManagedApplicationModule.bsl", fixtures_dir));
        file_set.insert(file_id, module_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        // Set up database
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, code);

        // Set workspace root via Salsa
        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::with_workspace(
            &db,
            Some(configuration_path_input),
            Some(&workspace_root),
            None,
        );
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        (diagnostics, code.to_string())
    }

    #[test]
    fn test_set_permissions_for_new_objects() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        // Use ASCII at start for correct byte range check
        let code = "//test - ManagedApplicationModule";
        let (diagnostics, file_content) = check_diagnostic(code, fixtures_dir);

        // Should find 1 diagnostic for Роль1
        // ПолныеПрава is in allowed list by default
        // Роль2 has setForNewObjects=false
        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic, found {}", diagnostics.len());

        // Check diagnostic details
        let diag = &diagnostics[0];
        assert_eq!(diag.code, DiagnosticCode::SetPermissionsForNewObjects);
        assert_eq!(diag.severity, Severity::Critical); // VULNERABILITY + CRITICAL maps to Critical severity
        assert!(diag.message.contains("Роль1"), "Message should mention Роль1");

        // Check range
        assert_diagnostic_range(&file_content, diag, 0, 0, 9);
    }

    #[test]
    fn test_not_managed_application_module() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        // Setup database with VFS
        let mut db = RootDatabaseImpl::new();

        // Create VFS for a non-ManagedApplicationModule file (CommonModule)
        let workspace_root = PathBuf::from(fixtures_dir);
        let vfs_path = VfsPath::new(format!("{}/CommonModules/Test/Ext/Module.bsl", fixtures_dir));

        // Create FileSet and SourceRoot
        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        file_set.insert(file_id, vfs_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        // Set up database
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "Процедура Тест()\nКонецПроцедуры");

        let provider =
            ide_db::SalsaProvider::with_workspace(&db, None, Some(&workspace_root), None);
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        // Should return empty for non-ManagedApplicationModule
        assert_eq!(diagnostics.len(), 0, "Non-ManagedApplicationModule should have no diagnostics");
    }

    #[test]
    fn test_custom_allowed_roles() {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/set_permissions_for_new_objects");

        // Setup database with VFS
        let mut db = RootDatabaseImpl::new();

        // Create VFS
        let workspace_root = PathBuf::from(fixtures_dir);

        // Create FileSet with ManagedApplicationModule
        let mut file_set = FileSet::default();

        let file_id = FileId(0);
        let module_path =
            VfsPath::new(format!("{}/Ext/ManagedApplicationModule.bsl", fixtures_dir));
        file_set.insert(file_id, module_path);

        let source_root_id = SourceRootId(0);
        let source_root = SourceRoot::new_local(file_set);

        // Set up database
        db.set_source_root(source_root_id, source_root);
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, "//test");

        // Set workspace root via Salsa
        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::with_workspace(
            &db,
            Some(configuration_path_input),
            Some(&workspace_root),
            None,
        );

        // Custom config: only Роль2 is allowed (not ПолныеПрава)
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::SetPermissionsForNewObjects,
            serde_json::json!({"namesFullAccessRole": "Роль2"}),
        );

        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);

        // Should find 2 diagnostics: Роль1 and ПолныеПрава
        // Роль2 is in custom allowed list
        assert_eq!(diagnostics.len(), 2, "Expected 2 diagnostics, found {}", diagnostics.len());

        let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("Роль1")), "Should have diagnostic for Роль1");
        assert!(
            messages.iter().any(|m| m.contains("ПолныеПрава")),
            "Should have diagnostic for ПолныеПрава"
        );
    }
}
