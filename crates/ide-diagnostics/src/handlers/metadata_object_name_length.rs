//! MetadataObjectNameLength diagnostic
//!
//! Checks that metadata object names don't exceed maximum allowed length.
//!

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use hir::ModuleMetadata;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 10,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

/// Default maximum metadata object name length.
const DEFAULT_MAX_LENGTH: usize = 80;

/// Collect diagnostics from module metadata.
///
/// Checks metadata object name length based on module type:
/// - CommonModule: check common_module name
/// - MetadataObject modules: check mdo name and children
/// - Register modules: check register name
pub fn from_metadata(metadata: &ModuleMetadata, ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MetadataObjectNameLength;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_length = ctx
        .config
        .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
        .unwrap_or(DEFAULT_MAX_LENGTH as i64) as usize;

    let mut diagnostics = Vec::new();

    // Check CommonModule
    if let Some(ref common_module) = metadata.common_module {
        check_common_module(common_module, max_length, code, ctx, &mut diagnostics);
    }

    // Check MetadataObject
    if let Some(ref mdo) = metadata.mdo {
        check_metadata_object(mdo, max_length, code, ctx, &mut diagnostics);
    }

    // Check Register
    if let Some(ref register) = metadata.register {
        check_register(register, max_length, code, ctx, &mut diagnostics);
    }

    diagnostics
}

/// Check for SessionModule - needs full configuration access.
///
/// SessionModule is special: it checks ALL metadata objects without modules.
/// This is called from runner.rs with DiagnosticsContext.
pub fn check_session_module(
    configuration: &bsl_metadata::Configuration,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MetadataObjectNameLength;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let max_length = ctx
        .config
        .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
        .unwrap_or(DEFAULT_MAX_LENGTH as i64) as usize;

    let mut diagnostics = Vec::new();

    for mdo in configuration.metadata_objects() {
        if !has_modules(&mdo.mdo_type) {
            let name_length = mdo.name.chars().count();
            if name_length > max_length {
                diagnostics.push(Diagnostic {
                    code,
                    message: format!(
                        "Rename the metadata object `{}` so that the name length is less than {}",
                        mdo.name, max_length
                    ),
                    severity: ctx.severity(code),
                    range: syntax::MODULE_RANGE,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

fn has_modules(mdo_type: &bsl_metadata::MdoType) -> bool {
    use bsl_metadata::MdoType;
    matches!(
        mdo_type,
        MdoType::Catalog
            | MdoType::Document
            | MdoType::BusinessProcess
            | MdoType::Task
            | MdoType::ChartOfAccounts
            | MdoType::ChartOfCalculationTypes
            | MdoType::ChartOfCharacteristicTypes
    )
}

fn check_common_module(
    module: &bsl_metadata::CommonModule,
    max_length: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = module.name().chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                module.name(),
                max_length
            ),
            severity: ctx.severity(code),
            range: syntax::MODULE_RANGE,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn check_metadata_object(
    mdo: &bsl_metadata::MetadataObject,
    max_length: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = mdo.name.chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                mdo.name, max_length
            ),
            severity: ctx.severity(code),
            range: syntax::MODULE_RANGE,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn check_register(
    register: &bsl_metadata::Register,
    max_length: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = register.name().chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                register.name(),
                max_length
            ),
            severity: ctx.severity(code),
            range: syntax::MODULE_RANGE,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use std::sync::Arc;

    const LONG_NAME: &str =
        "ОченьДлинноеИмяОбъектаКотороеВызываетПроблемыВРаботеАТакжеОшибкиВыгрузкиКонфигурации";

    fn make_metadata_with_common_module(module: bsl_metadata::CommonModule) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    fn make_metadata_with_mdo(mdo: bsl_metadata::MetadataObject) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ObjectModule,
            execution_context: None,
            common_module: None,
            mdo: Some(Arc::new(mdo)),
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    fn make_metadata_with_register(register: bsl_metadata::Register) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: Some(Arc::new(register)),
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    fn default_config() -> DiagnosticsConfig {
        DiagnosticsConfig::default()
    }

    #[test]
    fn test_common_module_long_name() {
        let module = bsl_metadata::CommonModule::builder().name(LONG_NAME).build();

        let metadata = make_metadata_with_common_module(module);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(LONG_NAME));
    }

    #[test]
    fn test_common_module_short_name() {
        let module = bsl_metadata::CommonModule::builder().name("ОбщийМодуль").build();

        let metadata = make_metadata_with_common_module(module);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_metadata_object_long_name() {
        let mdo = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, LONG_NAME);

        let metadata = make_metadata_with_mdo(mdo);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(LONG_NAME));
    }

    #[test]
    fn test_register_long_name() {
        let register = bsl_metadata::Register::builder()
            .name(LONG_NAME)
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .build();

        let metadata = make_metadata_with_register(register);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(LONG_NAME));
    }

    #[test]
    fn test_register_short_name() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрСведений")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .build();

        let metadata = make_metadata_with_register(register);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let module = bsl_metadata::CommonModule::builder().name(LONG_NAME).build();

        let metadata = make_metadata_with_common_module(module);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::MetadataObjectNameLength);

        let diagnostics = crate::test_utils::check_metadata_diagnostic_with_config(
            metadata,
            "",
            config,
            from_metadata,
        );

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_custom_max_length() {
        let module = bsl_metadata::CommonModule::builder().name("ШортМодуль").build();

        let metadata = make_metadata_with_common_module(module);

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MetadataObjectNameLength,
            serde_json::json!({"maxMetadataObjectNameLength": 5}),
        );

        let diagnostics = crate::test_utils::check_metadata_diagnostic_with_config(
            metadata,
            "",
            config,
            from_metadata,
        );

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_has_modules_classification() {
        use bsl_metadata::MdoType;

        assert!(has_modules(&MdoType::Catalog));
        assert!(has_modules(&MdoType::Document));
        assert!(has_modules(&MdoType::BusinessProcess));

        assert!(!has_modules(&MdoType::Enum));
        assert!(!has_modules(&MdoType::Constant));
        assert!(!has_modules(&MdoType::InformationRegister));
    }

    #[test]
    fn test_long_name_is_84_chars() {
        assert_eq!(LONG_NAME.chars().count(), 84);
    }

    #[test]
    fn test_session_module_checks_no_module_objects() {
        use ide_db::RootDatabaseImpl;
        let mut bsl_config = bsl_metadata::Configuration::new("TestConfig");

        // Add object WITH modules - should NOT be checked
        let catalog = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, LONG_NAME);
        bsl_config.add_metadata_object(catalog);

        // Add object WITHOUT modules - SHOULD be checked
        let constant =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Constant, LONG_NAME);
        bsl_config.add_metadata_object(constant);

        // Create minimal database
        let db = RootDatabaseImpl::new();
        let file_id = vfs::FileId(0);
        let diagnostics_config = default_config();

        let provider = ide_db::SalsaProvider::new(&db, None);
        let ctx = crate::DiagnosticsContext::new(&diagnostics_config, file_id, &provider);

        let diagnostics = check_session_module(&bsl_config, &ctx);

        // Only the constant should be flagged (Catalog has modules)
        assert_eq!(diagnostics.len(), 1);
    }
}
