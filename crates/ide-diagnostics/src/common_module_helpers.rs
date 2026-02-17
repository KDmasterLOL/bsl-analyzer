//! Helper functions for CommonModule diagnostics
//!
//! Ported from Java AbstractCommonModuleNameDiagnostic helper methods
//! Source: bsl-language-server/.../diagnostics/AbstractCommonModuleNameDiagnostic.java

use bsl_metadata::traits::{MdObject, Module};
use bsl_metadata::CommonModule;
use hir::ModuleMetadata;
use ide_db::TextRange;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

/// Check if module is Client type:
/// !serverCall && !server && !externalConnection && (clientOrdinary || clientManaged)
pub fn is_client(module: &CommonModule, ordinary_app_support: bool) -> bool {
    !module.is_server_call()
        && !module.is_server()
        && !module.is_external_connection()
        && is_client_ordinary_app_if_need(module, ordinary_app_support)
        && module.is_client_managed_application()
}

/// Check if module is ClientServer type:
/// !serverCall && server && externalConnection && (clientOrdinary || clientManaged)
pub fn is_client_server(module: &CommonModule, ordinary_app_support: bool) -> bool {
    !module.is_server_call()
        && module.is_server()
        && module.is_external_connection()
        && is_client_ordinary_app_if_need(module, ordinary_app_support)
        && module.is_client_managed_application()
}

/// Check if module is ServerCall type:
/// serverCall && server && !externalConnection && !clientOrdinary && !clientManaged
pub fn is_server_call(module: &CommonModule) -> bool {
    module.is_server_call()
        && module.is_server()
        && !module.is_external_connection()
        && !module.is_client_ordinary_application()
        && !module.is_client_managed_application()
}

/// Check if module is Server type:
/// !serverCall && server && externalConnection && clientOrdinaryIfNeed && !clientManaged
pub fn is_server(module: &CommonModule, ordinary_app_support: bool) -> bool {
    !module.is_server_call()
        && module.is_server()
        && module.is_external_connection()
        && is_client_ordinary_app_if_need(module, ordinary_app_support)
        && !module.is_client_managed_application()
}

fn is_client_ordinary_app_if_need(module: &CommonModule, ordinary_app_support: bool) -> bool {
    module.is_client_ordinary_application() || !ordinary_app_support
}

/// Find CommonModule metadata for given file
pub fn find_common_module_for_file(
    ctx: &crate::DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
) -> Option<bsl_metadata::CommonModule> {
    let file_uri = file_uri(ctx.db, ctx.file_id)?;

    configuration
        .common_modules()
        .iter()
        .find(|module| {
            if let Some(module_uri) = module.uri() {
                module_uri.to_lowercase() == file_uri.to_lowercase()
            } else {
                false
            }
        })
        .cloned()
}

fn file_uri(db: &dyn ide_db::RootDatabase, file_id: vfs::FileId) -> Option<String> {
    let source_root_input = db.file_source_root_input(file_id);
    let source_root_id = source_root_input.source_root_id(db);
    let source_root_input = db.source_root_input(source_root_id);
    let source_root = source_root_input.root(db);
    let file_set = source_root.file_set();
    let vfs_path = file_set.path_for_file(&file_id)?;
    Some(vfs_path.as_path().to_string_lossy().to_string())
}

/// Reusable check for CommonModuleName* diagnostics.
///
/// `name_should_contain` = true  — error if name does NOT contain keyword
/// `name_should_contain` = false — error if name DOES contain keyword (GlobalClient case)
pub fn check_common_module_name(
    metadata: &ModuleMetadata,
    ctx: &DiagnosticsContext,
    code: DiagnosticCode,
    predicate: impl Fn(&CommonModule, bool) -> bool,
    keywords: &[&str],
    name_should_contain: bool,
    message: &str,
) -> Vec<Diagnostic> {
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    if !predicate(module, ctx.config.ordinary_app_support) {
        return Vec::new();
    }

    let name_lower = module.name().to_lowercase();
    let contains_keyword = keywords.iter().any(|kw| name_lower.contains(kw));

    if contains_keyword == name_should_contain {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range: TextRange::empty(0.into()),
        tags: ctx.tags(code),
        fixes: vec![],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::CommonModule;

    #[test]
    fn test_is_client() {
        let module = CommonModule::builder()
            .server_call(false)
            .server(false)
            .external_connection(false)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        assert!(is_client(&module, true));
        assert!(is_client(&module, false));
    }

    #[test]
    fn test_is_client_server() {
        let module = CommonModule::builder()
            .server_call(false)
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(true)
            .build();

        assert!(is_client_server(&module, true));
    }

    #[test]
    fn test_is_server_call() {
        let module = CommonModule::builder()
            .server_call(true)
            .server(true)
            .external_connection(false)
            .client_ordinary_application(false)
            .client_managed_application(false)
            .build();

        assert!(is_server_call(&module));
    }

    #[test]
    fn test_is_server() {
        let module = CommonModule::builder()
            .server_call(false)
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(false)
            .build();

        assert!(is_server(&module, true));
    }
}
