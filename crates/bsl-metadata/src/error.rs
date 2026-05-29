use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetadataError {
    #[error("XML parsing error: {0}")]
    XmlError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Metadata object not found: {0}")]
    NotFound(String),

    #[error("Invalid metadata format: {0}")]
    InvalidFormat(String),
}

pub type Result<T> = std::result::Result<T, MetadataError>;
