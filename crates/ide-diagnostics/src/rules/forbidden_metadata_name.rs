//! ForbiddenMetadataName diagnostic.
//!
//! Checks that metadata object names don't use reserved/forbidden names
//! that match built-in 1C:Enterprise type names.
//!
//! ## Examples
//!
//! **Bad (forbidden names):**
//! - Catalog named "Catalog" or "Справочник"
//! - Document named "Document" or "Документ"
//! - Register named "InformationRegister" or "РегистрСведений"
//!
//! **Good:**
//! - Catalog named "Products" or "Товары"
//! - Document named "SalesOrder" or "ЗаказПокупателя"
//!
//! ## Diagnostic Code
//!
//! Code: `ForbiddenMetadataName`
//! Severity: Error (BLOCKER)
//! Type: Design
//! Tags: Standard, SQL, Design
//!
//! ## References
//!
//! Ported from bsl-language-server:
//! `ForbiddenMetadataNameDiagnostic.java`

use crate::metadata_diagnostic::{Diagnostic, DiagnosticSeverity, MetadataDiagnostic};
use bsl_metadata::{MdoType, MetadataObject};
use ide_db::RootDatabase;
use syntax::TextRange;

/// List of forbidden names (both Russian and English).
///
/// These names are reserved by 1C:Enterprise platform and should not be used
/// for metadata object names as they cause conflicts in queries and code.
const FORBIDDEN_NAMES: &[&str] = &[
    // English names
    "AccountingRegister",
    "AccountingRegisters",
    "AccumulationRegister",
    "AccumulationRegisters",
    "BusinessProcess",
    "BusinessProcesses",
    "CalculationRegister",
    "CalculationRegisters",
    "Catalog",
    "Catalogs",
    "ChartOfAccounts",
    "ChartOfCalculationTypes",
    "ChartOfCharacteristicTypes",
    "ChartsOfAccounts",
    "ChartsOfCalculationTypes",
    "ChartsOfCharacteristicTypes",
    "Constant",
    "Constants",
    "Document",
    "DocumentJournal",
    "DocumentJournals",
    "Documents",
    "Enum",
    "Enums",
    "ExchangePlan",
    "ExchangePlans",
    "FilterCriteria",
    "FilterCriterion",
    "InformationRegister",
    "InformationRegisters",
    "Task",
    "Tasks",
    // Russian names
    "БизнесПроцесс",
    "БизнесПроцессы",
    "Документ",
    "Документы",
    "ЖурналДокументов",
    "ЖурналыДокументов",
    "Задача",
    "Задачи",
    "Константа",
    "Константы",
    "КритерииОтбора",
    "КритерийОтбора",
    "Перечисление",
    "Перечисления",
    "ПланВидовРасчета",
    "ПланВидовХарактеристик",
    "ПланОбмена",
    "ПланСчетов",
    "ПланыВидовРасчета",
    "ПланыВидовХарактеристик",
    "ПланыОбмена",
    "ПланыСчетов",
    "РегистрБухгалтерии",
    "РегистрНакопления",
    "РегистрРасчета",
    "РегистрСведений",
    "РегистрыБухгалтерии",
    "РегистрыНакопления",
    "РегистрыРасчета",
    "РегистрыСведений",
    "Справочник",
    "Справочники",
];

/// ForbiddenMetadataName diagnostic.
///
/// Checks that metadata object names don't conflict with built-in 1C type names.
pub struct ForbiddenMetadataName;

impl ForbiddenMetadataName {
    /// Check if a name is forbidden (case-insensitive).
    ///
    /// Uses Unicode-aware case-insensitive comparison to support Cyrillic characters.
    fn is_forbidden(name: &str) -> bool {
        let name_lower = name.to_lowercase();
        FORBIDDEN_NAMES.iter().any(|forbidden| forbidden.to_lowercase() == name_lower)
    }
}

impl MetadataDiagnostic for ForbiddenMetadataName {
    fn filter_mdo_types(&self) -> &[MdoType] {
        // Check all main metadata types
        &[
            MdoType::AccountingRegister,
            MdoType::AccumulationRegister,
            MdoType::BusinessProcess,
            MdoType::CalculationRegister,
            MdoType::Catalog,
            MdoType::ChartOfAccounts,
            MdoType::ChartOfCalculationTypes,
            MdoType::ChartOfCharacteristicTypes,
            MdoType::Constant,
            MdoType::Document,
            MdoType::Enum,
            MdoType::InformationRegister,
            MdoType::Task,
        ]
    }

    fn check_metadata(
        &self,
        _db: &dyn RootDatabase,
        mdo: &MetadataObject,
        range: TextRange,
    ) -> Vec<Diagnostic> {
        if Self::is_forbidden(&mdo.name) {
            vec![Diagnostic {
                range,
                message: format!(
                    "Forbidden metadata object name: '{}'. This name conflicts with built-in 1C:Enterprise types.",
                    mdo.name
                ),
                severity: DiagnosticSeverity::Error,
            }]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_forbidden_english() {
        assert!(ForbiddenMetadataName::is_forbidden("Catalog"));
        assert!(ForbiddenMetadataName::is_forbidden("catalog")); // case-insensitive
        assert!(ForbiddenMetadataName::is_forbidden("CATALOG"));
        assert!(ForbiddenMetadataName::is_forbidden("Document"));
        assert!(ForbiddenMetadataName::is_forbidden("InformationRegister"));
    }

    #[test]
    fn test_is_forbidden_russian() {
        assert!(ForbiddenMetadataName::is_forbidden("Справочник"));
        assert!(ForbiddenMetadataName::is_forbidden("справочник")); // case-insensitive
        assert!(ForbiddenMetadataName::is_forbidden("Документ"));
        assert!(ForbiddenMetadataName::is_forbidden("РегистрСведений"));
    }

    #[test]
    fn test_is_not_forbidden() {
        assert!(!ForbiddenMetadataName::is_forbidden("Products"));
        assert!(!ForbiddenMetadataName::is_forbidden("Товары"));
        assert!(!ForbiddenMetadataName::is_forbidden("SalesOrder"));
        assert!(!ForbiddenMetadataName::is_forbidden("МойСправочник"));
    }

    #[test]
    fn test_check_metadata_forbidden() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = ForbiddenMetadataName;
        let mdo = MetadataObject::new(MdoType::Catalog, "Catalog");
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 1);
        assert!(results[0].message.contains("Forbidden metadata object name"));
        assert!(results[0].message.contains("Catalog"));
        assert_eq!(results[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn test_check_metadata_allowed() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = ForbiddenMetadataName;
        let mdo = MetadataObject::new(MdoType::Catalog, "Products");
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_filter_types() {
        let diagnostic = ForbiddenMetadataName;
        let types = diagnostic.filter_mdo_types();

        // Should check main metadata types
        assert!(types.contains(&MdoType::Catalog));
        assert!(types.contains(&MdoType::Document));
        assert!(types.contains(&MdoType::InformationRegister));
    }
}
