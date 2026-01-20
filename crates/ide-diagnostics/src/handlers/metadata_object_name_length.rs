//! MetadataObjectNameLength diagnostic
//!
//! Checks that metadata object names don't exceed maximum allowed length.
//!
//! Ported from: MetadataObjectNameLengthDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
use bsl_metadata::traits::MdObject;
use hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Default maximum metadata object name length.
const DEFAULT_MAX_LENGTH: usize = 80;

/// Collect diagnostics from module metadata.
///
/// Checks metadata object name length based on module type:
/// - CommonModule: check common_module name
/// - MetadataObject modules: check mdo name and children
/// - Register modules: check register name
pub fn from_metadata(metadata: &ModuleMetadata, config: &DiagnosticsConfig) -> Vec<Diagnostic> {
    if config.is_disabled(DiagnosticCode::MetadataObjectNameLength) {
        return Vec::new();
    }

    let max_length = config
        .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
        .unwrap_or(DEFAULT_MAX_LENGTH as i64) as usize;

    let mut diagnostics = Vec::new();

    // Check CommonModule
    if let Some(ref common_module) = metadata.common_module {
        check_common_module(common_module, max_length, &mut diagnostics);
    }

    // Check MetadataObject
    if let Some(ref mdo) = metadata.mdo {
        check_metadata_object(mdo, max_length, &mut diagnostics);
    }

    // Check Register
    if let Some(ref register) = metadata.register {
        check_register(register, max_length, &mut diagnostics);
    }

    diagnostics
}

/// Check for SessionModule - needs full configuration access.
///
/// SessionModule is special: it checks ALL metadata objects without modules.
/// This is called from runner.rs with DiagnosticsContext.
pub fn check_session_module(
    configuration: &bsl_metadata::Configuration,
    config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    if config.is_disabled(DiagnosticCode::MetadataObjectNameLength) {
        return Vec::new();
    }

    let max_length = config
        .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
        .unwrap_or(DEFAULT_MAX_LENGTH as i64) as usize;

    let mut diagnostics = Vec::new();

    for mdo in configuration.metadata_objects() {
        if !has_modules(&mdo.mdo_type) {
            let name_length = mdo.name.chars().count();
            if name_length > max_length {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::MetadataObjectNameLength,
                    message: format!(
                        "Rename the metadata object `{}` so that the name length is less than {}",
                        mdo.name, max_length
                    ),
                    severity: Severity::Major,
                    range: TextRange::empty(0.into()),
                    tags: vec![],
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = module.name().chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MetadataObjectNameLength,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                module.name(),
                max_length
            ),
            severity: Severity::Major,
            range: TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        });
    }
}

fn check_metadata_object(
    mdo: &bsl_metadata::MetadataObject,
    max_length: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = mdo.name.chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MetadataObjectNameLength,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                mdo.name, max_length
            ),
            severity: Severity::Major,
            range: TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        });
    }
}

fn check_register(
    register: &bsl_metadata::Register,
    max_length: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name_length = register.name().chars().count();
    if name_length > max_length {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MetadataObjectNameLength,
            message: format!(
                "Rename the metadata object `{}` so that the name length is less than {}",
                register.name(),
                max_length
            ),
            severity: Severity::Major,
            range: TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    fn make_metadata_with_mdo(mdo: bsl_metadata::MetadataObject) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ObjectModule,
            execution_context: None,
            common_module: None,
            mdo: Some(Arc::new(mdo)),
            register: None,
        }
    }

    fn make_metadata_with_register(register: bsl_metadata::Register) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: Some(Arc::new(register)),
        }
    }

    fn default_config() -> DiagnosticsConfig {
        DiagnosticsConfig::default()
    }

    #[test]
    fn test_common_module_long_name() {
        let module = bsl_metadata::CommonModule::builder().name(LONG_NAME).build();

        let metadata = make_metadata_with_common_module(module);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains(LONG_NAME));
    }

    #[test]
    fn test_common_module_short_name() {
        let module = bsl_metadata::CommonModule::builder().name("ОбщийМодуль").build();

        let metadata = make_metadata_with_common_module(module);
        let diagnostics = from_metadata(&metadata, &default_config());

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_metadata_object_long_name() {
        let mdo = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, LONG_NAME);

        let metadata = make_metadata_with_mdo(mdo);
        let diagnostics = from_metadata(&metadata, &default_config());

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
        let diagnostics = from_metadata(&metadata, &default_config());

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
        let diagnostics = from_metadata(&metadata, &default_config());

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let module = bsl_metadata::CommonModule::builder().name(LONG_NAME).build();

        let metadata = make_metadata_with_common_module(module);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::MetadataObjectNameLength);

        let diagnostics = from_metadata(&metadata, &config);

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

        let diagnostics = from_metadata(&metadata, &config);

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
        let mut config = bsl_metadata::Configuration::new("TestConfig");

        // Add object WITH modules - should NOT be checked
        let catalog = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, LONG_NAME);
        config.add_metadata_object(catalog);

        // Add object WITHOUT modules - SHOULD be checked
        let constant =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Constant, LONG_NAME);
        config.add_metadata_object(constant);

        let diagnostics = check_session_module(&config, &default_config());

        // Only the constant should be flagged (Catalog has modules)
        assert_eq!(diagnostics.len(), 1);
    }
}
