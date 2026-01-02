//! DenyIncompleteValues diagnostic.
//!
//! Checks that register dimensions have the `DenyIncompleteValues` flag enabled.
//!
//! ## Why?
//! The `DenyIncompleteValues` flag ensures data integrity by preventing records with
//! incomplete or empty dimension values from being written to the register. Without this
//! flag, invalid data can accumulate in the register, leading to:
//! - Data integrity issues
//! - Incorrect analytical reports
//! - Difficult-to-debug application errors
//!
//! ## Bad practice
//! ```xml
//! <Dimension>
//!   <Properties>
//!     <Name>Справочник1</Name>
//!     <DenyIncompleteValues>false</DenyIncompleteValues>  <!-- ← ERROR! -->
//!   </Properties>
//! </Dimension>
//! ```
//!
//! ## Good practice
//! ```xml
//! <Dimension>
//!   <Properties>
//!     <Name>Справочник1</Name>
//!     <DenyIncompleteValues>true</DenyIncompleteValues>  <!-- ← OK -->
//!   </Properties>
//! </Dimension>
//! ```
//!
//! ## Implementation
//!
//! Ported from:
//! - DenyIncompleteValuesDiagnostic.java (bsl-language-server) - PRIMARY
//!
//! Tier 3 diagnostic: Requires metadata (Register, Dimension).
//! Applies to all 4 register types:
//! - InformationRegister (Регистр сведений)
//! - AccumulationRegister (Регистр накопления)
//! - AccountingRegister (Регистр бухгалтерии)
//! - CalculationRegister (Регистр расчета)

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

/// Main entry point for DenyIncompleteValues diagnostic.
///
/// Checks:
/// 1. File is a register module (ManagerModule or RecordSetModule)
/// 2. Finds register metadata
/// 3. Reports dimensions with DenyIncompleteValues=false
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::DenyIncompleteValues) {
        return Vec::new();
    }

    // Workspace integration required for metadata access
    let config_path = match ctx.configuration_path.or(ctx.workspace_root) {
        Some(path) => path,
        None => {
            tracing::debug!("No workspace root - skipping DenyIncompleteValues check");
            return Vec::new();
        }
    };

    // Load configuration metadata via Salsa
    let config_path_str = config_path.to_string_lossy().to_string();
    let path_input = ide_db::metadata::ConfigurationPathInput::new(ctx.db, config_path_str);
    let configuration = ide_db::metadata::load_configuration(ctx.db, path_input);

    // Find register for current file
    let register = match find_register_for_file(ctx, &configuration) {
        Some(reg) => reg,
        None => {
            // Not a register module - skip check
            return Vec::new();
        }
    };

    // Check dimensions for DenyIncompleteValues flag
    let mut diagnostics = Vec::new();
    for dimension in register.dimensions() {
        if !dimension.is_deny_incomplete_values() {
            let message = format!(
                "Не указан флаг \"Запрет незаполненных значений\" у измерения \"{}\" \
                метаданного \"{}\"",
                dimension.name(),
                format_register_full_name(&register)
            );

            // Report at position (0, 0, 0, 9) - matches Java implementation
            let range = TextRange::new(0.into(), 9.into());

            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DenyIncompleteValues,
                message,
                severity: Severity::Warning,
                range,
                tags: vec![],
                fixes: vec![],
            });
        }
    }

    diagnostics
}

/// Format register full name with Russian type prefix.
fn format_register_full_name(register: &bsl_metadata::Register) -> String {
    let type_prefix = if register.is_information_register() {
        "РегистрСведений"
    } else if register.is_accumulation_register() {
        "РегистрНакопления"
    } else if register.is_accounting_register() {
        "РегистрБухгалтерии"
    } else if register.is_calculation_register() {
        "РегистрРасчета"
    } else {
        "Регистр"
    };

    format!("{}.{}", type_prefix, register.name())
}

/// Find Register metadata for given file.
///
/// Returns None if file is not a register module or metadata not found.
///
/// Matches file path against register names by checking if path contains:
/// - InformationRegisters/<Name>/
/// - AccumulationRegisters/<Name>/
/// - AccountingRegisters/<Name>/
/// - CalculationRegisters/<Name>/
fn find_register_for_file(
    ctx: &DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
) -> Option<bsl_metadata::Register> {
    // Get file path from VFS
    let file_path = file_path(ctx.db, ctx.file_id)?;

    // Extract register name from path
    // Path format: .../InformationRegisters/<Name>/Ext/ManagerModule.bsl
    let register_name = extract_register_name(&file_path)?;

    // Find register in configuration
    configuration.find_register(&register_name).cloned()
}

/// Extract register name from file path.
///
/// Examples:
/// - "/path/InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl" → "РегистрСведений1"
/// - "/path/AccumulationRegisters/РегистрНакопления1/Ext/RecordSetModule.bsl" → "РегистрНакопления1"
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
            // Skip leading slash
            let after_slash =
                after_type.strip_prefix('/').or_else(|| after_type.strip_prefix('\\'))?;
            // Take until next slash
            let name = after_slash.split(&['/', '\\'][..]).next()?;
            return Some(name.to_string());
        }
    }

    None
}

/// Get file path from VFS.
fn file_path(db: &dyn ide_db::RootDatabase, file_id: vfs::FileId) -> Option<String> {
    // Get source root for file
    let source_root_input = db.file_source_root_input(file_id);
    let source_root_id = source_root_input.source_root_id(db);

    // Get source root
    let source_root_input = db.source_root_input(source_root_id);
    let source_root = source_root_input.root(db);

    // Get file path from FileSet
    let file_set = source_root.file_set();
    let vfs_path = file_set.path_for_file(&file_id)?;

    // Convert VfsPath to path string
    let path_str = vfs_path.as_path().to_string_lossy().to_string();
    Some(path_str)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    #[test]
    fn test_extract_register_name() {
        assert_eq!(
            extract_register_name(
                "/path/InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl"
            ),
            Some("РегистрСведений1".to_string())
        );

        assert_eq!(
            extract_register_name(
                "C:\\path\\AccumulationRegisters\\РегистрНакопления1\\Ext\\RecordSetModule.bsl"
            ),
            Some("РегистрНакопления1".to_string())
        );

        assert_eq!(
            extract_register_name("/path/AccountingRegisters/Test/Ext/Module.bsl"),
            Some("Test".to_string())
        );

        assert_eq!(
            extract_register_name("/path/CalculationRegisters/CalcReg1/Ext/Module.bsl"),
            Some("CalcReg1".to_string())
        );

        assert_eq!(extract_register_name("/path/CommonModules/Test/Ext/Module.bsl"), None);

        assert_eq!(extract_register_name("/path/no/register/here.bsl"), None);
    }

    #[test]
    fn test_format_register_full_name() {
        let reg = bsl_metadata::Register::builder()
            .name("РегистрСведений1")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .build();

        assert_eq!(format_register_full_name(&reg), "РегистрСведений.РегистрСведений1");

        let reg = bsl_metadata::Register::builder()
            .name("РегистрНакопления1")
            .mdo_type(bsl_metadata::MdoType::AccumulationRegister)
            .build();

        assert_eq!(format_register_full_name(&reg), "РегистрНакопления.РегистрНакопления1");
    }

    #[test]
    #[ignore = "requires VFS setup for metadata loading"]
    fn test_deny_incomplete_values() {
        // This test will be enabled when full VFS integration is ready
        // For now, metadata loading requires proper SourceRoot setup
        let _code = include_str!("../../test_data/DenyIncompleteValuesDiagnostic.bsl");
        let _metadata_path = concat!(env!("CARGO_MANIFEST_DIR"), "/test_data/metadata/designer");

        // TODO: Enable when VFS integration is complete
        // Expected behavior:
        // - Should report 1 diagnostic at (0, 0, 0, 9)
        // - Message should contain "Справочник1" and "РегистрСведений.РегистрСведений1"
    }

    #[test]
    fn test_no_workspace() {
        let code = "Процедура Метод1()\nКонецПроцедуры\n";
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None, // No workspace - should skip check
            configuration_path: None,
            configuration_path_input: None,
        };

        let diagnostics = check(&ctx);
        assert_eq!(diagnostics.len(), 0, "Should skip check without workspace");
    }

    #[test]
    #[ignore = "requires VFS setup for metadata loading"]
    fn test_not_a_register() {
        // This test will be enabled when full VFS integration is ready
        // Expected: Should return 0 diagnostics for non-register files
    }
}
