//! HTTP client for 1C:Enterprise extension.
//!
//! Connects to an HTTP service published by a 1C extension in an infobase.
//! Provides query execution against live 1C databases.
//!
//! Port of mcp-1c/onec/client.go (~80 lines Go → Rust).

use reqwest::header;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Error type for 1C client operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("1C returned status {status}: {body}")]
    Status { status: u16, body: String },

    #[error("{0}")]
    Validation(String),
}

/// HTTP client for 1C:Enterprise extension.
#[derive(Clone)]
pub struct Client {
    base_url: String,
    user: Option<String>,
    password: Option<String>,
    http: reqwest::Client,
}

impl Client {
    /// Create a new client for a 1C HTTP service.
    ///
    /// - `base_url`: URL of the HTTP service (e.g., `http://localhost/base/hs/bsl-analyzer`)
    /// - `user`: 1C username (empty string for anonymous)
    /// - `password`: 1C password
    pub fn new(base_url: &str, user: &str, password: &str) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::CONNECTION, header::HeaderValue::from_static("close"));

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .default_headers(headers)
            .build()
            .expect("failed to build HTTP client");

        let (user, password) = if user.is_empty() {
            (None, None)
        } else {
            (Some(user.to_string()), Some(password.to_string()))
        };

        Self { base_url: base_url.trim_end_matches('/').to_string(), user, password, http }
    }

    /// Execute a SELECT query against the 1C database.
    pub async fn execute_query(&self, request: &QueryRequest) -> Result<QueryResult, Error> {
        self.post("/query", request).await
    }

    /// Validate SDBL query syntax via СхемаЗапроса.
    pub async fn validate_query(
        &self,
        request: &ValidateQueryRequest,
    ) -> Result<ValidateQueryResult, Error> {
        self.post("/validate-query", request).await
    }

    /// Check BSL code syntax without execution.
    pub async fn check_syntax(
        &self,
        request: &CheckSyntaxRequest,
    ) -> Result<CheckSyntaxResult, Error> {
        self.post("/check-syntax", request).await
    }

    /// Execute BSL code (statements) in the 1C database.
    pub async fn execute_code(&self, request: &ExecuteRequest) -> Result<ExecuteResult, Error> {
        self.post("/execute", request).await
    }

    /// Evaluate a BSL expression and return the result.
    pub async fn eval_expression(&self, request: &EvalRequest) -> Result<EvalResult, Error> {
        self.post("/eval", request).await
    }

    async fn post<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        body: &T,
    ) -> Result<R, Error> {
        let url = format!("{}{}", self.base_url, endpoint);
        let mut req = self.http.post(&url).json(body);

        if let Some(ref user) = self.user {
            req = req.basic_auth(user, self.password.as_deref());
        }

        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let body = if body.len() > 4096 { body[..4096].to_string() } else { body };
            return Err(Error::Status { status: status.as_u16(), body });
        }

        Ok(resp.json().await?)
    }
}

/// Request body for the query endpoint.
#[derive(Debug, Serialize)]
pub struct QueryRequest {
    /// Query text (SELECT/ВЫБРАТЬ only).
    pub query: String,
    /// Maximum number of rows (default: 100, max: 1000).
    pub limit: u32,
    /// Query parameters as key-value pairs.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, serde_json::Value>,
}

/// Response from the query endpoint.
#[derive(Debug, Deserialize)]
pub struct QueryResult {
    /// Column names.
    pub columns: Vec<String>,
    /// Row data (each row is an array of values).
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Total number of rows matched.
    pub total: u32,
    /// Whether the result was truncated by the limit.
    pub truncated: bool,
}

/// Request body for the validate-query endpoint.
#[derive(Debug, Serialize)]
pub struct ValidateQueryRequest {
    /// Query text to validate.
    pub query: String,
}

/// Response from the validate-query endpoint.
#[derive(Debug, Deserialize)]
pub struct ValidateQueryResult {
    /// Whether the query is valid.
    pub valid: bool,
    /// Error descriptions if query is invalid.
    #[serde(default)]
    pub errors: Vec<String>,
}

/// Request body for the check-syntax endpoint.
#[derive(Debug, Serialize)]
pub struct CheckSyntaxRequest {
    /// BSL code to check.
    pub code: String,
}

/// Response from the check-syntax endpoint.
#[derive(Debug, Deserialize)]
pub struct CheckSyntaxResult {
    /// Whether the code is syntactically valid.
    pub valid: bool,
    /// Error description if syntax is invalid.
    pub error: Option<String>,
}

/// Request body for the execute endpoint.
#[derive(Debug, Serialize)]
pub struct ExecuteRequest {
    /// BSL code (statements) to execute.
    pub code: String,
}

/// Response from the execute endpoint.
#[derive(Debug, Deserialize)]
pub struct ExecuteResult {
    /// Whether execution succeeded.
    pub success: bool,
    /// Error description if execution failed.
    pub error: Option<String>,
    /// Context structure populated by the executed code.
    pub context: Option<serde_json::Map<String, serde_json::Value>>,
    /// Execution duration in milliseconds.
    pub duration_ms: Option<u64>,
}

/// Request body for the eval endpoint.
#[derive(Debug, Serialize)]
pub struct EvalRequest {
    /// BSL expression to evaluate.
    pub expression: String,
}

/// Response from the eval endpoint.
#[derive(Debug, Deserialize)]
pub struct EvalResult {
    /// Whether evaluation succeeded.
    pub success: bool,
    /// Evaluated result (serialized to JSON).
    pub result: Option<serde_json::Value>,
    /// 1C type name of the result.
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    /// Error description if evaluation failed.
    pub error: Option<String>,
}
