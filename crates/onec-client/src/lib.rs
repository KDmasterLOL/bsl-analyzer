use reqwest::header;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("invalid JSON response from 1C: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("1C returned status {status}: {body}")]
    Status { status: u16, body: String },

    #[error("{0}")]
    Validation(String),
}

#[derive(Clone)]
pub struct Client {
    base_url: String,
    user: Option<String>,
    password: Option<String>,
    http: reqwest::Client,
}

impl Client {
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

    pub async fn execute_query(&self, request: &QueryRequest) -> Result<QueryResult, Error> {
        self.post("/query", request).await
    }

    pub async fn validate_query(
        &self,
        request: &ValidateQueryRequest,
    ) -> Result<ValidateQueryResult, Error> {
        self.post("/validate-query", request).await
    }

    pub async fn check_syntax(
        &self,
        request: &CheckSyntaxRequest,
    ) -> Result<CheckSyntaxResult, Error> {
        self.post("/check-syntax", request).await
    }

    pub async fn execute_code(&self, request: &ExecuteRequest) -> Result<ExecuteResult, Error> {
        self.post("/execute", request).await
    }

    pub async fn eval_expression(&self, request: &EvalRequest) -> Result<EvalResult, Error> {
        self.post("/eval", request).await
    }

    pub async fn event_log(&self, request: &EventLogRequest) -> Result<EventLogResult, Error> {
        self.post("/event-log", request).await
    }

    pub async fn list_metadata(
        &self,
        request: &MetadataListRequest,
    ) -> Result<MetadataListResult, Error> {
        self.post("/metadata-list", request).await
    }

    pub async fn metadata_structure(
        &self,
        request: &MetadataStructureRequest,
    ) -> Result<MetadataStructureResult, Error> {
        self.post("/metadata-structure", request).await
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

        let body = resp.bytes().await?;
        decode_json(&body)
    }
}

fn decode_json<R: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<R, Error> {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    Ok(serde_json::from_slice(body)?)
}

#[derive(Debug, Serialize)]
pub struct QueryRequest {
    pub query: String,
    pub limit: u32,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total: u32,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct ValidateQueryRequest {
    pub query: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidateQueryResult {
    pub valid: bool,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct CheckSyntaxRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckSyntaxResult {
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecuteRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteResult {
    pub success: bool,
    pub error: Option<String>,
    pub context: Option<serde_json::Map<String, serde_json::Value>>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct EvalRequest {
    pub expression: String,
}

#[derive(Debug, Deserialize)]
pub struct EvalResult {
    pub success: bool,
    pub result: Option<serde_json::Value>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    pub error: Option<String>,
}

/// Filters for a read of the 1C event log (журнал регистрации). Dates are ISO-8601
/// strings; every filter is optional and omitted from the payload when `None` so the
/// 1C side falls back to no restriction on that dimension.
#[derive(Debug, Default, Serialize)]
pub struct EventLogRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct EventLogResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total: u32,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct MetadataListRequest {
    pub meta_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_mask: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
pub struct MetadataListItem {
    pub name: String,
    pub full_name: String,
    pub synonym: String,
}

#[derive(Debug, Deserialize)]
pub struct MetadataListResult {
    pub items: Vec<MetadataListItem>,
    pub returned: u32,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct MetadataStructureRequest {
    pub meta_type: String,
    pub name: String,
}

pub type MetadataStructureResult = serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_list_omits_empty_mask() {
        let request =
            MetadataListRequest { meta_type: "Constants".into(), name_mask: None, limit: 10 };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["meta_type"], "Constants");
        assert!(value.get("name_mask").is_none());
        assert_eq!(value["limit"], 10);
    }

    #[test]
    fn metadata_structure_contract_is_stable() {
        let request = MetadataStructureRequest {
            meta_type: "Catalogs".into(),
            name: "Организации".into(),
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["meta_type"], "Catalogs");
        assert_eq!(value["name"], "Организации");
    }

    #[test]
    fn response_decoder_accepts_utf8_bom_emitted_by_1c() {
        let result: QueryResult = decode_json(
            b"\xEF\xBB\xBF{\"columns\":[\"Value\"],\"rows\":[[1]],\"total\":1,\"truncated\":false}",
        )
        .unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.rows[0][0], 1);
    }
}
