use bsl_metadata::traits::MdObject;
use bsl_metadata::CommonModule;
use hir::ModuleMetadata;
use stdx::case::CaseExt;

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub fn is_client(module: &CommonModule, ordinary_app_support: bool) -> bool {
    !module.is_server_call()
        && !module.is_server()
        && !module.is_external_connection()
        && is_client_ordinary_app_if_need(module, ordinary_app_support)
        && module.is_client_managed_application()
}

pub fn is_client_server(module: &CommonModule, ordinary_app_support: bool) -> bool {
    !module.is_server_call()
        && module.is_server()
        && module.is_external_connection()
        && is_client_ordinary_app_if_need(module, ordinary_app_support)
        && module.is_client_managed_application()
}

pub fn is_server_call(module: &CommonModule) -> bool {
    module.is_server_call()
        && module.is_server()
        && !module.is_external_connection()
        && !module.is_client_ordinary_application()
        && !module.is_client_managed_application()
}

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

pub fn find_common_module_for_file_anywhere(
    ctx: &crate::DiagnosticsContext,
) -> Option<bsl_metadata::CommonModule> {
    ctx.common_module_for_file().map(|module| (*module).clone())
}

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

    let name_lower = module.name().fold_lower();
    let contains_keyword = keywords.iter().any(|kw| name_lower.contains(kw));

    if contains_keyword == name_should_contain {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message: message.to_string(),
        severity: ctx.severity(code),
        range: syntax::MODULE_RANGE,
        tags: ctx.tags(code),
        fixes: vec![],
    }]
}

#[macro_export]
macro_rules! define_common_module_name_check {
    (
        code: $code:ident,
        diagnostic_type: $dtype:expr,
        severity: $severity:expr,
        tags: $tags:expr,
        clean_code_attribute: $clean:expr,
        predicate: $predicate:expr,
        keywords: $keywords:expr,
        name_should_contain: $name_should_contain:expr,
        message: $message:expr $(,)?
    ) => {
        pub const METADATA: $crate::DiagnosticMetadata = $crate::define_metadata! {
            diagnostic_type: $dtype,
            severity: $severity,
            scope: $crate::DiagnosticScope::Bsl,
            modules: &[bsl_metadata::ModuleType::CommonModule],
            minutes_to_fix: 5,
            activated_by_default: true,
            compatibility_mode: $crate::DiagnosticCompatibilityMode::Undefined,
            tags: $tags,
            can_locate_on_project: false,
            extra_min_for_complexity: 0.0,
            lsp_severity_override: "",
            clean_code_attribute: $clean,
        };

        pub fn from_metadata(
            metadata: &hir::ModuleMetadata,
            ctx: &$crate::DiagnosticsContext,
        ) -> Vec<$crate::Diagnostic> {
            $crate::common_module_helpers::check_common_module_name(
                metadata,
                ctx,
                $crate::DiagnosticCode::$code,
                $predicate,
                $keywords,
                $name_should_contain,
                $message,
            )
        }
    };
    (
        code: $code:ident,
        diagnostic_type: $dtype:expr,
        severity: $severity:expr,
        tags: $tags:expr,
        predicate: $predicate:expr,
        keywords: $keywords:expr,
        name_should_contain: $name_should_contain:expr,
        message: $message:expr $(,)?
    ) => {
        pub const METADATA: $crate::DiagnosticMetadata = $crate::define_metadata! {
            diagnostic_type: $dtype,
            severity: $severity,
            scope: $crate::DiagnosticScope::Bsl,
            modules: &[bsl_metadata::ModuleType::CommonModule],
            minutes_to_fix: 5,
            activated_by_default: true,
            compatibility_mode: $crate::DiagnosticCompatibilityMode::Undefined,
            tags: $tags,
            can_locate_on_project: false,
            extra_min_for_complexity: 0.0,
            lsp_severity_override: "",
        };

        pub fn from_metadata(
            metadata: &hir::ModuleMetadata,
            ctx: &$crate::DiagnosticsContext,
        ) -> Vec<$crate::Diagnostic> {
            $crate::common_module_helpers::check_common_module_name(
                metadata,
                ctx,
                $crate::DiagnosticCode::$code,
                $predicate,
                $keywords,
                $name_should_contain,
                $message,
            )
        }
    };
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
