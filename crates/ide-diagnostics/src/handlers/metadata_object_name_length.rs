//! MetadataObjectNameLength diagnostic
//!
//! Checks that metadata object names don't exceed maximum allowed length.
//!
//! Ported from: MetadataObjectNameLengthDiagnostic.java

use crate::metadata_diagnostic::MetadataDiagnostic;
use crate::rules::metadata_object_name_length::MetadataObjectNameLength;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::{MdObject, Module};
use syntax::TextRange;

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::MetadataObjectNameLength) {
        return Vec::new();
    }

    let max_length = ctx
        .config
        .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
        .unwrap_or(80) as usize;

    let config_path = match ctx.configuration_path.or(ctx.workspace_root) {
        Some(path) => path,
        None => return Vec::new(),
    };

    let config_path_str = config_path.to_string_lossy().to_string();
    let path_input = ide_db::metadata::ConfigurationPathInput::new(ctx.db, config_path_str);
    let configuration = ide_db::metadata::load_configuration(ctx.db, path_input);

    let mut diagnostics = Vec::new();

    if is_session_module(ctx) {
        check_metadata_without_modules(&configuration, max_length, &mut diagnostics);
    } else if let Some(common_module) = find_common_module_for_file(ctx, &configuration) {
        check_common_module(&common_module, max_length, &mut diagnostics);
    } else if let Some(metadata_object) = find_metadata_object_for_file(ctx, &configuration) {
        check_metadata_object(ctx.db, &metadata_object, max_length, &mut diagnostics);
    } else if let Some(register) = find_register_for_file(ctx, &configuration) {
        check_register(&register, max_length, &mut diagnostics);
    }

    diagnostics
}

fn is_session_module(ctx: &DiagnosticsContext) -> bool {
    if let Some(file_path) = file_path(ctx.db, ctx.file_id) {
        file_path.ends_with("/Ext/SessionModule.bsl")
            || file_path.ends_with("\\Ext\\SessionModule.bsl")
    } else {
        false
    }
}

fn check_metadata_without_modules(
    configuration: &bsl_metadata::Configuration,
    max_length: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
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
    db: &dyn ide_db::RootDatabase,
    mdo: &bsl_metadata::MetadataObject,
    max_length: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let diagnostic = MetadataObjectNameLength::with_max_length(max_length);
    let range = TextRange::empty(0.into());

    for meta_diag in diagnostic.check_metadata(db, mdo, range) {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MetadataObjectNameLength,
            message: meta_diag.message,
            severity: Severity::Major,
            range: meta_diag.range,
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

fn find_common_module_for_file(
    ctx: &DiagnosticsContext,
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

fn find_metadata_object_for_file(
    ctx: &DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
) -> Option<bsl_metadata::MetadataObject> {
    let file_path = file_path(ctx.db, ctx.file_id)?;
    let object_name = extract_metadata_object_name(&file_path)?;

    configuration
        .metadata_objects()
        .iter()
        .find(|obj| obj.name.to_lowercase() == object_name.to_lowercase())
        .cloned()
}

fn find_register_for_file(
    ctx: &DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
) -> Option<bsl_metadata::Register> {
    let file_path = file_path(ctx.db, ctx.file_id)?;
    let register_name = extract_register_name(&file_path)?;

    configuration.find_register(&register_name).cloned()
}

fn extract_metadata_object_name(file_path: &str) -> Option<String> {
    let object_types = [
        "Catalogs",
        "Documents",
        "DataProcessors",
        "Reports",
        "BusinessProcesses",
        "Tasks",
        "ChartsOfAccounts",
        "ChartsOfCalculationTypes",
        "ChartsOfCharacteristicTypes",
        "ExchangePlans",
        "DocumentJournals",
    ];

    for object_type in &object_types {
        if let Some(pos) = file_path.find(object_type) {
            let after_type = &file_path[pos + object_type.len()..];
            // Skip leading slash(es) first
            let trimmed = after_type.trim_start_matches('/');
            if let Some(name_end) = trimmed.find('/') {
                return Some(trimmed[..name_end].to_string());
            }
        }
    }

    None
}

fn extract_register_name(file_path: &str) -> Option<String> {
    let register_types = [
        "InformationRegisters",
        "AccumulationRegisters",
        "AccountingRegisters",
        "CalculationRegisters",
    ];

    for register_type in &register_types {
        if let Some(pos) = file_path.find(register_type) {
            let after_type = &file_path[pos + register_type.len()..];
            // Skip leading slash(es) first
            let trimmed = after_type.trim_start_matches('/');
            if let Some(name_end) = trimmed.find('/') {
                return Some(trimmed[..name_end].to_string());
            }
        }
    }

    None
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

fn file_path(db: &dyn ide_db::RootDatabase, file_id: vfs::FileId) -> Option<String> {
    file_uri(db, file_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use bsl_metadata::Configuration;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    const LONG_NAME: &str =
        "ОченьДлинноеИмяОбъектаКотороеВызываетПроблемыВРаботеАТакжеОшибкиВыгрузкиКонфигурации";

    fn check_with_metadata(
        code: &str,
        _configuration: Configuration,
        _config_path: &str,
    ) -> Vec<Diagnostic> {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let db = Rc::new(db) as Rc<dyn RootDatabase>;

        let config = DiagnosticsConfig::default();

        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_disabled() {
        let db = RootDatabaseImpl::new();
        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::MetadataObjectNameLength);

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id: vfs::FileId(0),
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_workspace() {
        let db = RootDatabaseImpl::new();
        let config = DiagnosticsConfig::default();

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id: vfs::FileId(0),
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_metadata_file() {
        let configuration = Configuration::new("Configuration");
        let code = "Перем Маркер;";
        let diagnostics = check_with_metadata(code, configuration, "/test.bsl");
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_custom_max_length_parameter() {
        let db = RootDatabaseImpl::new();
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MetadataObjectNameLength,
            serde_json::json!({"maxMetadataObjectNameLength": 10}),
        );

        let ctx = DiagnosticsContext {
            db: &db,
            config: &config,
            file_id: vfs::FileId(0),
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
        };

        let max_length = ctx
            .config
            .get_int(DiagnosticCode::MetadataObjectNameLength, "maxMetadataObjectNameLength")
            .unwrap_or(80) as usize;

        assert_eq!(max_length, 10);
    }

    #[test]
    fn test_long_name_is_84_chars() {
        assert_eq!(LONG_NAME.chars().count(), 84);
    }

    #[test]
    #[ignore = "requires VFS setup for file path resolution"]
    fn test_is_session_module_detection() {
        // SessionModule detection will work when full VFS integration is ready
        // Expected: Files at /Ext/SessionModule.bsl should be detected
    }

    #[test]
    #[ignore = "requires VFS setup for file path resolution"]
    fn test_is_not_session_module() {
        // SessionModule detection will work when full VFS integration is ready
        // Expected: Other files should not be detected as SessionModule
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
}
