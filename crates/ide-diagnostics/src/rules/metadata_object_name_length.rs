//! MetadataObjectNameLength diagnostic.
//!
//! Checks that metadata object names don't exceed maximum length.
//!
//! ## Rationale
//!
//! 1C:Enterprise has limits on metadata object name lengths.
//! Long names cause issues with:
//! - Database table/column name limits (especially for PostgreSQL, MySQL)
//! - Generated code readability
//! - Query performance
//!
//! ## Examples
//!
//! **Bad (too long):**
//! ```text
//! Catalog.СправочникТоваровИУслугСОченьДлиннымИменемКотороеПревышаетМаксимальнуюДлину
//! ```
//!
//! **Good:**
//! ```text
//! Catalog.Products
//! Catalog.Товары
//! ```
//!
//! ## Configuration
//!
//! - `maxMetadataObjectNameLength` (default: 80)
//!   Maximum allowed length for metadata object names.
//!
//! ## Diagnostic Code
//!
//! Code: `MetadataObjectNameLength`
//! Severity: Error (MAJOR)
//! Type: ERROR
//! Tags: Standard
//!
//! ## References
//!
//! Ported from bsl-language-server:
//! `MetadataObjectNameLengthDiagnostic.java`

use crate::metadata_diagnostic::{Diagnostic, DiagnosticSeverity, MetadataDiagnostic};
use bsl_metadata::{MdoType, MetadataObject};
use ide_db::RootDatabase;
use syntax::TextRange;

/// Default maximum length for metadata object names.
pub const DEFAULT_MAX_NAME_LENGTH: usize = 80;

// Message format matches bsl-language-server:
// EN: "Rename the metadata object `%s` so that the name length is less than %s"
// RU: "Переименуйте объект конфигурации `%s` так, чтобы длина наименования была меньше %s"
// Using English by default; bilingual support to be added when DiagnosticsContext has language field

/// MetadataObjectNameLength diagnostic.
///
/// Checks that metadata object names don't exceed configured maximum length.
pub struct MetadataObjectNameLength {
    /// Maximum allowed name length (configurable).
    max_length: usize,
}

impl MetadataObjectNameLength {
    /// Create diagnostic with default max length (80).
    pub fn new() -> Self {
        Self { max_length: DEFAULT_MAX_NAME_LENGTH }
    }

    /// Create diagnostic with custom max length.
    pub fn with_max_length(max_length: usize) -> Self {
        Self { max_length }
    }
}

impl Default for MetadataObjectNameLength {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataDiagnostic for MetadataObjectNameLength {
    fn filter_mdo_types(&self) -> &[MdoType] {
        // Check ALL metadata types
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
        let name_length = mdo.name.len();

        if name_length > self.max_length {
            vec![Diagnostic {
                range,
                message: format!(
                    "Rename the metadata object `{}` so that the name length is less than {}",
                    mdo.name, self.max_length
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
    fn test_default_max_length() {
        let diagnostic = MetadataObjectNameLength::new();
        assert_eq!(diagnostic.max_length, 80);
    }

    #[test]
    fn test_custom_max_length() {
        let diagnostic = MetadataObjectNameLength::with_max_length(50);
        assert_eq!(diagnostic.max_length, 50);
    }

    #[test]
    fn test_check_metadata_within_limit() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = MetadataObjectNameLength::new();
        let mdo = MetadataObject::new(MdoType::Catalog, "Products");
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_check_metadata_exceeds_limit() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = MetadataObjectNameLength::with_max_length(10);
        let mdo = MetadataObject::new(MdoType::Catalog, "VeryLongCatalogNameThatExceedsLimit");
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 1);
        // Check new message format matches Java
        assert_eq!(
            results[0].message,
            "Rename the metadata object `VeryLongCatalogNameThatExceedsLimit` so that the name length is less than 10"
        );
        assert_eq!(results[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn test_check_metadata_exactly_at_limit() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = MetadataObjectNameLength::with_max_length(8);
        let mdo = MetadataObject::new(MdoType::Catalog, "Products"); // 8 characters
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 0); // Exactly at limit is OK
    }

    #[test]
    fn test_check_metadata_one_over_limit() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = MetadataObjectNameLength::with_max_length(7);
        let mdo = MetadataObject::new(MdoType::Catalog, "Products"); // 8 characters
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 1); // One over limit triggers diagnostic
    }

    #[test]
    fn test_filter_types() {
        let diagnostic = MetadataObjectNameLength::new();
        let types = diagnostic.filter_mdo_types();

        // Should check ALL metadata types
        assert!(types.contains(&MdoType::Catalog));
        assert!(types.contains(&MdoType::Document));
        assert!(types.contains(&MdoType::InformationRegister));
        assert!(types.contains(&MdoType::Constant));
    }
}
