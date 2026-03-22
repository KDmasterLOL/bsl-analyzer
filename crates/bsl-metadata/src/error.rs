//! Error types for metadata operations

use thiserror::Error;

/// Errors that can occur when working with metadata
#[derive(Error, Debug)]
pub enum MetadataError {
    /// XML parsing error
    #[error("XML parsing error: {0}")]
    XmlError(String),

    /// IO error when reading metadata files
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Metadata object not found
    #[error("Metadata object not found: {0}")]
    NotFound(String),

    /// Invalid metadata format
    #[error("Invalid metadata format: {0}")]
    InvalidFormat(String),
}

/// Result type for metadata operations
pub type Result<T> = std::result::Result<T, MetadataError>;
