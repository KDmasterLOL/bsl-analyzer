//! ForbiddenMetadataName diagnostic.
//!
//! Checks that metadata object names don't use reserved/forbidden names
//! that match built-in 1C:Enterprise type names.
//!
//! This diagnostic checks both parent metadata objects AND their child elements
//! (attributes, tabular sections) to ensure no reserved names are used.
//!
//! ## Examples
//!
//! **Bad (forbidden names):**
//! - Catalog named "Catalog" or "Справочник"
//!   - Diagnostic: `Запрещено использовать имя 'Справочник' для 'Справочник.Справочник'`
//! - Document named "Document" or "Документ"
//!   - Diagnostic: `Запрещено использовать имя 'Документ' для 'Документ.Документ'`
//! - Catalog with attribute named "РегистрСведений"
//!   - Diagnostic: `Запрещено использовать имя 'РегистрСведений' для 'Справочник.MyCatalog.Реквизит.РегистрСведений'`
//!
//! **Good:**
//! - Catalog named "Products" or "Товары"
//! - Document named "SalesOrder" or "ЗаказПокупателя"
//! - Attributes named "ProductCode", "OrderDate", etc.
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
//! - Java: `ForbiddenMetadataNameDiagnostic.java`
//! - Rust (tree-sitter): `bsl-language-server-rust/crates/bsl-diagnostics/src/rules/forbidden_metadata_name.rs`
//!
//! Note: Rust implementation has child checking as TODO. This implementation includes full child validation.

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

    /// Build hierarchical path for metadata object.
    ///
    /// Format: `{MdoType}.{Name}` for parent, or `{ParentPath}.{ChildType}.{ChildName}` for children.
    ///
    /// Examples:
    /// - Parent catalog: `Справочник.Справочник1`
    /// - Child attribute: `Справочник.Справочник1.Реквизит.Name`
    /// - Child tabular section: `Справочник.Справочник1.ТабличнаяЧасть.Name`
    fn build_hierarchical_path(
        mdo_type_name: &str,
        object_name: &str,
        parent_path: Option<&str>,
    ) -> String {
        if let Some(parent) = parent_path {
            // Child element: include child type in path
            // For now, we use generic "Реквизит" (Attribute) as Java determines this from metadata structure
            format!("{}.Реквизит.{}", parent, object_name)
        } else {
            // Parent object
            format!("{}.{}", mdo_type_name, object_name)
        }
    }

    /// Check metadata object name recursively (including children).
    ///
    /// Returns diagnostics for the object and all its children with forbidden names.
    fn check_name_recursive(
        mdo: &MetadataObject,
        parent_path: Option<&str>,
        range: TextRange,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        // Build hierarchical path for this object
        let mdo_type_name = mdo.mdo_type.russian_name();
        let current_path = Self::build_hierarchical_path(mdo_type_name, &mdo.name, parent_path);

        // Check current object's name
        if Self::is_forbidden(&mdo.name) {
            diagnostics.push(Diagnostic {
                range,
                message: format!(
                    "Запрещено использовать имя '{}' для '{}'",
                    mdo.name, current_path
                ),
                severity: DiagnosticSeverity::Error,
            });
        }

        // Recursively check all children
        for child in &mdo.children {
            let child_diagnostics = Self::check_name_recursive(child, Some(&current_path), range);
            diagnostics.extend(child_diagnostics);
        }

        diagnostics
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
        // Check metadata object and all its children recursively
        Self::check_name_recursive(mdo, None, range)
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
        assert_eq!(
            results[0].message,
            "Запрещено использовать имя 'Catalog' для 'Справочник.Catalog'"
        );
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

    #[test]
    fn test_check_metadata_with_forbidden_children() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = ForbiddenMetadataName;

        // Create catalog with forbidden name and forbidden children
        let mut mdo = MetadataObject::new(MdoType::Catalog, "Справочник");
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "РегистрСведений"));
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "Документ"));

        let range = TextRange::empty(0.into());
        let results = diagnostic.check_metadata(&db, &mdo, range);

        // Should have 3 diagnostics: 1 parent + 2 children
        assert_eq!(results.len(), 3);

        // Check parent diagnostic
        assert_eq!(
            results[0].message,
            "Запрещено использовать имя 'Справочник' для 'Справочник.Справочник'"
        );

        // Check first child diagnostic
        assert_eq!(
            results[1].message,
            "Запрещено использовать имя 'РегистрСведений' для 'Справочник.Справочник.Реквизит.РегистрСведений'"
        );

        // Check second child diagnostic
        assert_eq!(
            results[2].message,
            "Запрещено использовать имя 'Документ' для 'Справочник.Справочник.Реквизит.Документ'"
        );
    }

    #[test]
    fn test_check_metadata_with_allowed_children() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = ForbiddenMetadataName;

        // Create catalog with allowed name and allowed children
        let mut mdo = MetadataObject::new(MdoType::Catalog, "Products");
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "ProductCode"));
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "ProductName"));

        let range = TextRange::empty(0.into());
        let results = diagnostic.check_metadata(&db, &mdo, range);

        // Should have no diagnostics
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_check_metadata_mixed_children() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = ForbiddenMetadataName;

        // Create catalog with allowed parent but forbidden child
        let mut mdo = MetadataObject::new(MdoType::Catalog, "Products");
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "РегистрСведений"));
        mdo.add_child(MetadataObject::new(MdoType::Catalog, "ProductName"));

        let range = TextRange::empty(0.into());
        let results = diagnostic.check_metadata(&db, &mdo, range);

        // Should have 1 diagnostic for forbidden child only
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].message,
            "Запрещено использовать имя 'РегистрСведений' для 'Справочник.Products.Реквизит.РегистрСведений'"
        );
    }

    #[test]
    fn test_build_hierarchical_path() {
        // Parent object
        let path =
            ForbiddenMetadataName::build_hierarchical_path("Справочник", "Справочник1", None);
        assert_eq!(path, "Справочник.Справочник1");

        // Child object
        let parent_path = "Справочник.Справочник1";
        let child_path = ForbiddenMetadataName::build_hierarchical_path(
            "Справочник",
            "РегистрСведений",
            Some(parent_path),
        );
        assert_eq!(child_path, "Справочник.Справочник1.Реквизит.РегистрСведений");
    }
}
