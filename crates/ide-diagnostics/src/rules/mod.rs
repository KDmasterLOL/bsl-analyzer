//! Metadata-based diagnostic rules.
//!
//! This module contains implementations of Tier 3 diagnostics that analyze
//! 1C:Enterprise metadata objects.
//!
//! ## Available Diagnostics
//!
//! - [`ForbiddenMetadataName`] - Checks for forbidden/reserved names
//! - [`MetadataObjectNameLength`] - Checks name length limits
//!
//! ## Usage
//!
//! ```no_run
//! use ide_diagnostics::rules::ForbiddenMetadataName;
//! use ide_diagnostics::metadata_diagnostic::MetadataDiagnosticRunner;
//! use ide_db::RootDatabaseImpl;
//!
//! let db = RootDatabaseImpl::new();
//! let diagnostic = ForbiddenMetadataName;
//! let results = MetadataDiagnosticRunner::run(
//!     &db,
//!     &diagnostic,
//!     "/path/to/configuration"
//! );
//!
//! for diag in results {
//!     println!("{}: {}", diag.severity as u8, diag.message);
//! }
//! ```

pub mod forbidden_metadata_name;
pub mod metadata_object_name_length;

pub use forbidden_metadata_name::ForbiddenMetadataName;
pub use metadata_object_name_length::{MetadataObjectNameLength, DEFAULT_MAX_NAME_LENGTH};
