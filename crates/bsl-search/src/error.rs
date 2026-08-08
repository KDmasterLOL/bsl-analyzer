/// Version 2 keys a file by `(collection, root_id, path)` in the shared PostgreSQL schema.
///
/// The bump is deliberate and its cost is a hard stop for older builds: a column added to a
/// table is invisible to them, but the DATA is not. Once a build that knows roots publishes an
/// extension, an older one on the same schema folds two files sharing a relative path into one
/// key and silently serves one of them. The choice was never "hard stop versus rolling upgrade"
/// — it was a hard stop versus losing a live file in silence.
pub const SCHEMA_VERSION_CURRENT: i32 = 2;

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

    #[error("storage_not_initialized: PostgreSQL baseline storage has not been initialized (schema: {schema}); run `admin migrate` first")]
    StorageNotInitialized { schema: String },

    #[error("schema_version_mismatch: expected {expected}, got {actual:?}")]
    SchemaVersionMismatch { expected: i32, actual: Option<i32> },
}

impl SearchError {
    pub fn is_storage_not_initialized(&self) -> bool {
        matches!(self, Self::StorageNotInitialized { .. })
    }

    pub fn is_schema_version_mismatch(&self) -> bool {
        matches!(self, Self::SchemaVersionMismatch { .. })
    }

    pub fn reason_code(&self) -> Option<&'static str> {
        match self {
            Self::StorageNotInitialized { .. } => Some("storage_not_initialized"),
            Self::SchemaVersionMismatch { .. } => Some("schema_version_mismatch"),
            Self::ExternalBaseline(message) => external_baseline_reason_code(message),
            _ => None,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::StorageNotInitialized { .. } | Self::SchemaVersionMismatch { .. } => false,
            Self::ExternalBaseline(message) => matches!(
                external_baseline_reason_code(message),
                Some("postgres_connect_failed" | "postgres_auth_failed")
            ),
            Self::Postgres(error) => postgres_error_is_retryable(error),
            Self::Io(_) => true,
            Self::Embedder(_) | Self::Index(_) => false,
            Self::Sqlite(_) => false,
        }
    }

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
        "serving_semantic_rootless",
        "root_id_not_portable",
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
        let result: Result<postgres::Config, _> = "invalid".parse();
        if let Err(pg_err) = result {
            let err: SearchError = pg_err.into();
            assert!(!err.is_retryable());
            assert!(err.is_terminal());
        }
    }

    /// Every refusal this crate names must be recognised by the classifier, or the name reaches
    /// nobody: consumers read `reason_code()`, and an unlisted prefix arrives as null there
    /// while the message still reads as if the failure were identified.
    ///
    /// Checked over the whole class rather than for one member, because the list and the
    /// messages are edited in different files and drift silently apart.
    #[test]
    fn every_named_refusal_of_this_crate_has_a_reason_code() {
        for name in [
            "serving_lexical_unavailable",
            "serving_semantic_unavailable",
            "serving_semantic_rootless",
            "root_id_not_portable",
            "postgres_connect_failed",
        ] {
            let error = SearchError::ExternalBaseline(format!("{name}: something went wrong"));
            assert_eq!(error.reason_code(), Some(name), "{name} is not classified");
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
        assert!(!SearchError::Sqlite(rusqlite::Error::InvalidQuery).is_terminal());
    }
}
