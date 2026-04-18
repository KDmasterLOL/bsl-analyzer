//! Error types for bsl-search.

/// Current schema version for the PostgreSQL baseline storage.
/// Bumped whenever an incompatible schema change is introduced.
pub const SCHEMA_VERSION_CURRENT: i32 = 1;

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

    /// PostgreSQL baseline storage has not been initialized.
    /// Run `bsl-analyzer search baseline admin migrate` to create schema and tables.
    #[error("storage_not_initialized: PostgreSQL baseline storage has not been initialized (schema: {schema}); run `admin migrate` first")]
    StorageNotInitialized { schema: String },

    /// PostgreSQL baseline storage has a schema version incompatible with this
    /// version of bsl-analyzer.
    #[error("schema_version_mismatch: expected {expected}, got {actual:?}")]
    SchemaVersionMismatch { expected: i32, actual: Option<i32> },
}

impl SearchError {
    /// Returns `true` if this error indicates the storage needs initialization.
    /// Used by callers to distinguish between terminal and recoverable errors.
    pub fn is_storage_not_initialized(&self) -> bool {
        matches!(self, Self::StorageNotInitialized { .. })
    }

    /// Returns `true` if this error indicates a schema version incompatibility
    /// that requires running migrations.
    pub fn is_schema_version_mismatch(&self) -> bool {
        matches!(self, Self::SchemaVersionMismatch { .. })
    }

    /// Returns a machine-readable reason code for errors that should be
    /// surfaced explicitly to CLI / MCP callers.
    pub fn reason_code(&self) -> Option<&'static str> {
        match self {
            Self::StorageNotInitialized { .. } => Some("storage_not_initialized"),
            Self::SchemaVersionMismatch { .. } => Some("schema_version_mismatch"),
            Self::ExternalBaseline(message) => external_baseline_reason_code(message),
            _ => None,
        }
    }

    /// Returns `true` if the error is potentially retryable through
    /// credential refresh / connection retry (e.g. transient PostgreSQL
    /// connectivity or authentication failure caused by an expired Vault lease).
    ///
    /// Returns `false` for terminal errors: config issues, storage not
    /// initialized, schema version mismatch, malformed helper responses,
    /// target mismatch, and generic `ExternalBaseline` strings that are
    /// known to be non-retryable (invalid connection string, schema validation).
    pub fn is_retryable(&self) -> bool {
        match self {
            // Terminal: config / schema / storage state errors.
            Self::StorageNotInitialized { .. } | Self::SchemaVersionMismatch { .. } => false,
            Self::ExternalBaseline(message) => matches!(
                external_baseline_reason_code(message),
                Some("postgres_connect_failed" | "postgres_auth_failed")
            ),
            // Raw PostgreSQL errors are only retryable for connectivity/auth cases.
            Self::Postgres(error) => postgres_error_is_retryable(error),
            // IO errors during query execution may be transient.
            Self::Io(_) => true,
            // Embedder / index errors are not related to the external
            // baseline connection and should not trigger a refresh.
            Self::Embedder(_) | Self::Index(_) => false,
            // SQLite errors are not related to the external baseline.
            Self::Sqlite(_) => false,
        }
    }

    /// Returns `true` if this error is terminal with respect to the external
    /// baseline and must **not** be silently swallowed or fallen back to SQLite.
    ///
    /// Terminal errors include storage/schema failures and credential resolution
    /// errors that have exhausted the refreshable wrapper's retry.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::StorageNotInitialized { .. }
                | Self::SchemaVersionMismatch { .. }
                | Self::Embedder(_)
                | Self::Index(_)
        ) || matches!(self, Self::Postgres(error) if !postgres_error_is_retryable(error))
            || matches!(
                self,
                Self::ExternalBaseline(message)
                    if !matches!(
                        external_baseline_reason_code(message),
                        Some(
                            "postgres_connect_failed"
                                | "postgres_auth_failed"
                                | "serving_lexical_unavailable"
                                | "serving_semantic_unavailable"
                        )
                    )
            )
    }
}

fn external_baseline_reason_code(message: &str) -> Option<&'static str> {
    const KNOWN_REASON_CODES: &[&str] = &[
        "helper_spawn_failed",
        "helper_timeout",
        "helper_protocol_error",
        "helper_rejected",
        "resolved_target_mismatch",
        "missing_config",
        "postgres_connect_failed",
        "postgres_auth_failed",
        "refresh_retry_exhausted",
        "serving_lexical_unavailable",
        "serving_semantic_unavailable",
    ];

    KNOWN_REASON_CODES
        .iter()
        .copied()
        .find(|code| message.strip_prefix(code).is_some_and(|rest| rest.starts_with(':')))
}

fn postgres_error_is_retryable(error: &postgres::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    [
        "connection refused",
        "connection reset",
        "connection closed",
        "could not connect",
        "broken pipe",
        "timed out",
        "timeout",
        "password authentication failed",
        "authentication failed",
        "no connection to the server",
        "server closed the connection",
        "eof",
        "tls",
        "ssl",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_not_initialized_is_not_retryable() {
        let err = SearchError::StorageNotInitialized { schema: "bsl_search".to_owned() };
        assert!(err.is_storage_not_initialized());
        assert!(!err.is_retryable());
    }

    #[test]
    fn schema_version_mismatch_is_not_retryable() {
        let err = SearchError::SchemaVersionMismatch { expected: 1, actual: Some(0) };
        assert!(err.is_schema_version_mismatch());
        assert!(!err.is_retryable());
    }

    #[test]
    fn postgres_parse_error_is_terminal() {
        // Parsing failures are configuration errors and must not trigger
        // a credential refresh loop.
        let result: Result<postgres::Config, _> = "invalid".parse();
        if let Err(pg_err) = result {
            let err: SearchError = pg_err.into();
            assert!(!err.is_retryable());
            assert!(err.is_terminal());
        }
    }

    #[test]
    fn io_error_is_retryable() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        let err: SearchError = io_err.into();
        assert!(err.is_retryable());
    }

    #[test]
    fn external_baseline_error_is_not_retryable() {
        let err = SearchError::ExternalBaseline("credential refresh failed: resolve: …".to_owned());
        assert!(!err.is_retryable());
    }

    #[test]
    fn embedder_error_is_not_retryable() {
        let err = SearchError::Embedder("embedder timeout".to_owned());
        assert!(!err.is_retryable());
    }

    #[test]
    fn index_error_is_not_retryable() {
        let err = SearchError::Index("index corrupted".to_owned());
        assert!(!err.is_retryable());
    }

    #[test]
    fn external_baseline_reason_code_is_detected() {
        let err = SearchError::ExternalBaseline(
            "helper_protocol_error: credential helper returned invalid JSON".to_owned(),
        );
        assert_eq!(err.reason_code(), Some("helper_protocol_error"));
    }

    #[test]
    fn external_baseline_connectivity_error_is_retryable_and_non_terminal() {
        let err = SearchError::ExternalBaseline(
            "postgres_connect_failed: failed to get pooled connection".to_owned(),
        );
        assert!(err.is_retryable());
        assert!(!err.is_terminal());
        assert_eq!(err.reason_code(), Some("postgres_connect_failed"));
    }

    #[test]
    fn external_baseline_auth_error_is_retryable_and_non_terminal() {
        let err = SearchError::ExternalBaseline(
            "postgres_auth_failed: password authentication failed".to_owned(),
        );
        assert!(err.is_retryable());
        assert!(!err.is_terminal());
        assert_eq!(err.reason_code(), Some("postgres_auth_failed"));
    }

    #[test]
    fn external_baseline_retry_exhausted_error_is_terminal() {
        let err = SearchError::ExternalBaseline(
            "refresh_retry_exhausted: postgres error: connection refused".to_owned(),
        );
        assert!(!err.is_retryable());
        assert!(err.is_terminal());
        assert_eq!(err.reason_code(), Some("refresh_retry_exhausted"));
    }

    #[test]
    fn serving_lexical_unavailable_is_non_terminal() {
        let err = SearchError::ExternalBaseline(
            "serving_lexical_unavailable: snapshot has no serving rows".to_owned(),
        );
        assert!(!err.is_retryable());
        assert!(!err.is_terminal());
        assert_eq!(err.reason_code(), Some("serving_lexical_unavailable"));
    }

    #[test]
    fn serving_semantic_unavailable_is_non_terminal() {
        let err = SearchError::ExternalBaseline(
            "serving_semantic_unavailable: snapshot has no semantic serving rows".to_owned(),
        );
        assert!(!err.is_retryable());
        assert!(!err.is_terminal());
        assert_eq!(err.reason_code(), Some("serving_semantic_unavailable"));
    }

    #[test]
    fn storage_errors_are_terminal() {
        assert!(
            SearchError::StorageNotInitialized { schema: "bsl_search".to_owned() }.is_terminal()
        );
        assert!(SearchError::SchemaVersionMismatch { expected: 1, actual: Some(0) }.is_terminal());
        assert!(SearchError::ExternalBaseline("refresh failed".to_owned()).is_terminal());
        assert!(SearchError::Embedder("timeout".to_owned()).is_terminal());
        assert!(SearchError::Index("corrupted".to_owned()).is_terminal());
    }

    #[test]
    fn transient_errors_are_not_terminal() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert!(!SearchError::from(io_err).is_terminal());
        // SQLite errors are local and not terminal for the external baseline.
        assert!(!SearchError::Sqlite(rusqlite::Error::InvalidQuery).is_terminal());
    }
}
