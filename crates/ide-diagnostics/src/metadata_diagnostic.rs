//! Base infrastructure for metadata-based diagnostics (Tier 3).
//!
//! This module provides the `MetadataDiagnostic` trait and runner infrastructure
//! for diagnostics that analyze 1C:Enterprise metadata objects.
//!
//! ## Architecture
//!
//! Ported from bsl-language-server's `AbstractMetadataDiagnostic` pattern:
//!
//! 1. **Diagnostic trait**: Defines which metadata types to check and how
//! 2. **Runner**: Loads configuration, filters objects, runs checks
//! 3. **Result**: Returns list of diagnostics with ranges and messages
//!
//! ## Usage Example
//!
//! ```no_run
//! use ide_diagnostics::metadata_diagnostic::{MetadataDiagnostic, Diagnostic, DiagnosticSeverity};
//! use bsl_metadata::MdoType;
//! use ide_db::RootDatabase;
//! use syntax::TextRange;
//!
//! struct ForbiddenNameDiagnostic;
//!
//! impl MetadataDiagnostic for ForbiddenNameDiagnostic {
//!     fn filter_mdo_types(&self) -> &[MdoType] {
//!         // Check all types
//!         &[MdoType::Catalog, MdoType::Document, MdoType::InformationRegister]
//!     }
//!
//!     fn check_metadata(
//!         &self,
//!         _db: &dyn RootDatabase,
//!         mdo: &bsl_metadata::MetadataObject,
//!         range: TextRange,
//!     ) -> Vec<Diagnostic> {
//!         // Check if name is forbidden
//!         if mdo.name == "Catalog" {
//!             vec![Diagnostic {
//!                 range,
//!                 message: format!("Forbidden name: {}", mdo.name),
//!                 severity: DiagnosticSeverity::Error,
//!             }]
//!         } else {
//!             Vec::new()
//!         }
//!     }
//! }
//! ```

use bsl_metadata::{MdoType, MetadataObject};
use ide_db::RootDatabase;
use syntax::TextRange;

/// Severity level for metadata diagnostics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Error - blocks compilation/deployment
    Error,
    /// Warning - should be fixed
    Warning,
    /// Information - suggestion
    Info,
}

/// A metadata diagnostic result
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Range in source file where diagnostic should be displayed
    pub range: TextRange,
    /// Human-readable diagnostic message
    pub message: String,
    /// Severity level
    pub severity: DiagnosticSeverity,
}

/// Base trait for metadata-based diagnostics.
///
/// Implement this trait to create diagnostics that analyze metadata objects.
/// The diagnostic will be run on all metadata objects of the specified types.
pub trait MetadataDiagnostic {
    /// Filter which metadata object types to check.
    ///
    /// Return a list of [`MdoType`] that this diagnostic should analyze.
    /// The runner will only call [`check_metadata`] for objects of these types.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use bsl_metadata::MdoType;
    /// # struct MyDiagnostic;
    /// # impl ide_diagnostics::metadata_diagnostic::MetadataDiagnostic for MyDiagnostic {
    /// fn filter_mdo_types(&self) -> &[MdoType] {
    ///     // Only check catalogs and documents
    ///     &[MdoType::Catalog, MdoType::Document]
    /// }
    /// # fn check_metadata(&self, _db: &dyn ide_db::RootDatabase, _mdo: &bsl_metadata::MetadataObject, _range: syntax::TextRange) -> Vec<ide_diagnostics::metadata_diagnostic::Diagnostic> { Vec::new() }
    /// # }
    /// ```
    fn filter_mdo_types(&self) -> &[MdoType];

    /// Check a metadata object and return diagnostics.
    ///
    /// This method is called for each metadata object that matches the filter.
    ///
    /// # Arguments
    ///
    /// * `db` - Database for accessing other queries
    /// * `mdo` - Metadata object to check
    /// * `range` - Default range for diagnostic (typically module start)
    ///
    /// # Returns
    ///
    /// Vector of diagnostics found. Return empty vector if no issues.
    fn check_metadata(
        &self,
        db: &dyn RootDatabase,
        mdo: &MetadataObject,
        range: TextRange,
    ) -> Vec<Diagnostic>;
}

/// Runner for metadata diagnostics.
///
/// This struct coordinates loading configuration and running metadata diagnostics.
pub struct MetadataDiagnosticRunner;

impl MetadataDiagnosticRunner {
    /// Run a metadata diagnostic.
    ///
    /// # Arguments
    ///
    /// * `db` - Database with metadata loaded (concrete type required for Salsa)
    /// * `diagnostic` - Diagnostic to run
    /// * `config_path` - Path to configuration directory
    ///
    /// # Returns
    ///
    /// Vector of all diagnostics found across all matching metadata objects.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use ide_diagnostics::metadata_diagnostic::MetadataDiagnosticRunner;
    /// # use ide_db::RootDatabaseImpl;
    /// # struct MyDiagnostic;
    /// # impl ide_diagnostics::metadata_diagnostic::MetadataDiagnostic for MyDiagnostic {
    /// #   fn filter_mdo_types(&self) -> &[bsl_metadata::MdoType] { &[] }
    /// #   fn check_metadata(&self, _db: &dyn ide_db::RootDatabase, _mdo: &bsl_metadata::MetadataObject, _range: syntax::TextRange) -> Vec<ide_diagnostics::metadata_diagnostic::Diagnostic> { Vec::new() }
    /// # }
    /// let db = RootDatabaseImpl::new();
    /// let diagnostic = MyDiagnostic;
    /// let results = MetadataDiagnosticRunner::run(
    ///     &db,
    ///     &diagnostic,
    ///     "/path/to/configuration"
    /// );
    /// ```
    pub fn run<DB: RootDatabase, D: MetadataDiagnostic>(
        db: &DB,
        diagnostic: &D,
        config_path: &str,
    ) -> Vec<Diagnostic> {
        use ide_db::metadata::ConfigurationPathInput;

        let _span = tracing::info_span!("metadata_diagnostic_runner").entered();

        // Load configuration via Salsa
        let path_input = ConfigurationPathInput::new(db, config_path.to_string());
        let config = db.load_configuration(path_input);

        let mut results = Vec::new();
        let filter_types = diagnostic.filter_mdo_types();

        // Check metadata objects
        for mdo in config.metadata_objects() {
            if filter_types.contains(&mdo.mdo_type) {
                // For now, use empty range - will be improved in Phase 2.3
                let range = TextRange::empty(0.into());
                let diagnostics = diagnostic.check_metadata(db, mdo, range);
                results.extend(diagnostics);
            }
        }

        tracing::debug!(count = results.len(), "metadata diagnostic completed");

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDiagnostic;

    impl MetadataDiagnostic for TestDiagnostic {
        fn filter_mdo_types(&self) -> &[MdoType] {
            &[MdoType::InformationRegister]
        }

        fn check_metadata(
            &self,
            _db: &dyn RootDatabase,
            mdo: &MetadataObject,
            range: TextRange,
        ) -> Vec<Diagnostic> {
            vec![Diagnostic {
                range,
                message: format!("Test diagnostic for {}", mdo.name),
                severity: DiagnosticSeverity::Warning,
            }]
        }
    }

    #[test]
    fn test_diagnostic_trait() {
        let diagnostic = TestDiagnostic;
        assert_eq!(diagnostic.filter_mdo_types(), &[MdoType::InformationRegister]);
    }

    #[test]
    fn test_diagnostic_check() {
        use ide_db::RootDatabaseImpl;

        let db = RootDatabaseImpl::new();
        let diagnostic = TestDiagnostic;
        let mdo = MetadataObject::new(MdoType::InformationRegister, "TestRegister");
        let range = TextRange::empty(0.into());

        let results = diagnostic.check_metadata(&db, &mdo, range);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message, "Test diagnostic for TestRegister");
        assert_eq!(results[0].severity, DiagnosticSeverity::Warning);
    }
}
