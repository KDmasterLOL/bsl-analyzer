//! Error types for bsl-search.

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("postgres error: {0}")]
    Postgres(#[from] postgres::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("embedder error: {0}")]
    Embedder(String),

    #[error("index error: {0}")]
    Index(String),

    #[error("external baseline error: {0}")]
    ExternalBaseline(String),

    #[error("store not initialized")]
    NotInitialized,
}
