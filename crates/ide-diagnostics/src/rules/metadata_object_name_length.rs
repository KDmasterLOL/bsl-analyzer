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
//! Severity: Warning (MINOR)
//! Type: Design
//! Tags: Standard, Badpractice
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
                    "Metadata object name is too long: {} characters (max: {}). Consider shortening '{}'.",
                    name_length, self.max_length, mdo.name
                ),
                severity: DiagnosticSeverity::Warning,
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
        assert!(results[0].message.contains("too long"));
        assert!(results[0].message.contains("35 characters")); // Actual length
        assert!(results[0].message.contains("max: 10"));
        assert_eq!(results[0].severity, DiagnosticSeverity::Warning);
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
