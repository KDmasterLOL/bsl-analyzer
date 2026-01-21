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

use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
use hir_def::ModuleMetadata;
use ide_db::TextRange;

/// Collect diagnostics from module metadata.
///
/// Checks register dimensions for DenyIncompleteValues=false flag.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    config: &DiagnosticsConfig,
    file_text: &str,
) -> Vec<Diagnostic> {
    if config.is_disabled(DiagnosticCode::DenyIncompleteValues) {
        return Vec::new();
    }

    // Only process register modules
    let Some(ref register) = metadata.register else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();

    for dimension in register.dimensions() {
        if !dimension.is_deny_incomplete_values() {
            let message = format!(
                "Не указан флаг \"Запрет незаполненных значений\" у измерения \"{}\" \
                метаданного \"{}\"",
                dimension.name(),
                format_register_full_name(register)
            );

            // Use range [0, min(9, file_len)) to avoid exceeding file bounds
            // Java implementation uses (0, 0, 0, 9) but we need to be safe for small files
            let file_len = file_text.len();
            let end_offset = std::cmp::min(9, file_len);
            let range = TextRange::new(0.into(), (end_offset as u32).into());

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_metadata_with_register(register: bsl_metadata::Register) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: Some(Arc::new(register)),
            form: None,
        }
    }

    fn default_config() -> DiagnosticsConfig {
        DiagnosticsConfig::all_enabled()
    }

    #[test]
    fn test_dimension_without_deny_incomplete_values() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрСведений1")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Справочник1")
                    .deny_incomplete_values(false) // Should trigger diagnostic
                    .build(),
            )
            .build();

        let metadata = make_metadata_with_register(register);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = from_metadata(&metadata, &default_config(), file_text);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Справочник1"));
        assert!(diagnostics[0].message.contains("РегистрСведений.РегистрСведений1"));
    }

    #[test]
    fn test_dimension_with_deny_incomplete_values() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрСведений1")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Справочник1")
                    .deny_incomplete_values(true) // OK - no diagnostic
                    .build(),
            )
            .build();

        let metadata = make_metadata_with_register(register);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = from_metadata(&metadata, &default_config(), file_text);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_multiple_dimensions() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрНакопления1")
            .mdo_type(bsl_metadata::MdoType::AccumulationRegister)
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Измерение1")
                    .deny_incomplete_values(false) // Should trigger
                    .build(),
            )
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Измерение2")
                    .deny_incomplete_values(true) // OK
                    .build(),
            )
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Измерение3")
                    .deny_incomplete_values(false) // Should trigger
                    .build(),
            )
            .build();

        let metadata = make_metadata_with_register(register);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = from_metadata(&metadata, &default_config(), file_text);

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("Измерение1"));
        assert!(diagnostics[1].message.contains("Измерение3"));
    }

    #[test]
    fn test_not_a_register() {
        let metadata = ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            form: None,
        };

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = from_metadata(&metadata, &default_config(), file_text);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let register = bsl_metadata::Register::builder()
            .name("Регистр1")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Измерение")
                    .deny_incomplete_values(false)
                    .build(),
            )
            .build();

        let metadata = make_metadata_with_register(register);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::DenyIncompleteValues);

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = from_metadata(&metadata, &config, file_text);

        assert!(diagnostics.is_empty());
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

        let reg = bsl_metadata::Register::builder()
            .name("РегистрБухгалтерии1")
            .mdo_type(bsl_metadata::MdoType::AccountingRegister)
            .build();
        assert_eq!(format_register_full_name(&reg), "РегистрБухгалтерии.РегистрБухгалтерии1");

        let reg = bsl_metadata::Register::builder()
            .name("РегистрРасчета1")
            .mdo_type(bsl_metadata::MdoType::CalculationRegister)
            .build();
        assert_eq!(format_register_full_name(&reg), "РегистрРасчета.РегистрРасчета1");
    }

    #[test]
    fn test_small_file_range() {
        let register = bsl_metadata::Register::builder()
            .name("Регистр")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .add_dimension(
                bsl_metadata::Dimension::builder()
                    .name("Изм")
                    .deny_incomplete_values(false)
                    .build(),
            )
            .build();

        let metadata = make_metadata_with_register(register);
        let file_text = "//"; // Very small file (2 chars)
        let diagnostics = from_metadata(&metadata, &default_config(), file_text);

        assert_eq!(diagnostics.len(), 1);
        // Range should not exceed file length
        assert_eq!(diagnostics[0].range.end(), 2.into());
    }
}
