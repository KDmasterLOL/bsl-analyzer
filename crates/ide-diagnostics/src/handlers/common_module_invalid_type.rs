use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode};
use hir::ModuleMetadata;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Unpredictable, MetadataTag::Design],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleInvalidType;

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

    // std469 admits exactly four execution-context combinations; any other mix is
    // non-standard. `Клиент (обычное приложение)` is part of every server/client/
    // client-server template, so it is required here regardless of the configuration's
    // ordinary-application support — unlike the naming checks, which apply that leniency.
    // `Глобальный` is a free overlay and does not affect type validity.
    let sc = module.is_server_call();
    let s = module.is_server();
    let ec = module.is_external_connection();
    let coa = module.is_client_ordinary_application();
    let cma = module.is_client_managed_application();

    let server = !sc && s && ec && coa && !cma;
    let server_call = sc && s && !ec && !coa && !cma;
    let client = !sc && !s && !ec && coa && cma;
    let client_server = !sc && s && ec && coa && cma;

    let is_valid = server || server_call || client || client_server;

    if is_valid {
        return Vec::new();
    }

    vec![Diagnostic {
        code,
        message:
            "Общий модуль должен быть одного из типов: Server, ServerCall, Client, ClientServer"
                .to_string(),
        severity: ctx.severity(code),
        range: syntax::MODULE_RANGE,
        tags: ctx.tags(code),
        fixes: vec![],
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    #[test]
    fn test_from_metadata_invalid_type() {
        let module = bsl_metadata::CommonModule::builder().name("InvalidModule").build();

        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            integration_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::CommonModuleInvalidType);
    }

    #[test]
    fn test_from_metadata_valid_server() {
        let module = bsl_metadata::CommonModule::builder()
            .name("ServerModule")
            .server(true)
            .external_connection(true)
            .client_ordinary_application(true)
            .client_managed_application(false)
            .build();

        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: Some(hir::ExecutionContext::Server),
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            integration_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }

    fn fires(module: bsl_metadata::CommonModule) -> bool {
        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            integration_service: None,
            form: None,
        };
        !crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata).is_empty()
    }

    fn builder() -> bsl_metadata::CommonModuleBuilder {
        bsl_metadata::CommonModule::builder().name("M")
    }

    #[test]
    fn valid_templates_stay_silent() {
        // ВызовСервера, Клиентский, Клиент-серверный (Серверный covered above).
        assert!(!fires(builder().server_call(true).server(true).build()));
        assert!(!fires(
            builder().client_ordinary_application(true).client_managed_application(true).build()
        ));
        assert!(!fires(
            builder()
                .server(true)
                .external_connection(true)
                .client_ordinary_application(true)
                .client_managed_application(true)
                .build()
        ));
        // Глобальный is a free overlay over a valid client module.
        assert!(!fires(
            builder()
                .global(true)
                .client_ordinary_application(true)
                .client_managed_application(true)
                .build()
        ));
    }

    #[test]
    fn non_standard_flag_mixes_fire() {
        // Server flag alone (no ExternalConnection / Клиент(обычное)) — not the Серверный template.
        assert!(fires(builder().server(true).build()));
        // Server + ExternalConnection but missing Клиент(обычное).
        assert!(fires(builder().server(true).external_connection(true).build()));
        // Клиент(управляемое) alone, without Клиент(обычное) — not the Клиентский template.
        assert!(fires(builder().client_managed_application(true).build()));
        // ВызовСервера combined with a client context.
        assert!(fires(
            builder().server_call(true).server(true).client_managed_application(true).build()
        ));
    }

    #[test]
    fn exhaustive_flag_table_matches_four_templates() {
        // Independent contract: across all 2^5 (sc, s, ec, coa, cma) combinations, exactly
        // the four std469 templates are valid; every other combination must fire. Hardcoding
        // the valid set here (rather than reusing the production expression) guards against a
        // one-bit edit to a template in `from_metadata`.
        let valid: [(bool, bool, bool, bool, bool); 4] = [
            (false, true, true, true, false),  // Серверный
            (true, true, false, false, false), // ВызовСервера
            (false, false, false, true, true), // Клиентский
            (false, true, true, true, true),   // Клиент-серверный
        ];
        for bits in 0u8..32 {
            let sc = bits & 1 != 0;
            let s = bits & 2 != 0;
            let ec = bits & 4 != 0;
            let coa = bits & 8 != 0;
            let cma = bits & 16 != 0;
            let module = builder()
                .server_call(sc)
                .server(s)
                .external_connection(ec)
                .client_ordinary_application(coa)
                .client_managed_application(cma)
                .build();
            let expected_fires = !valid.contains(&(sc, s, ec, coa, cma));
            assert_eq!(
                fires(module),
                expected_fires,
                "combo sc={sc} s={s} ec={ec} coa={coa} cma={cma}"
            );
        }
    }

    #[test]
    fn test_from_metadata_non_common_module() {
        let metadata = hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            integration_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 0);
    }
}
