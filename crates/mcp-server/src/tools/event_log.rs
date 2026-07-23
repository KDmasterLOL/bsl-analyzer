use crate::state::SharedState;
use crate::tools::response::structured;
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde_json::json;

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 1000;

/// Owned filter set forwarded from the `event_log` tool. All dimensions are optional; the
/// 1C side treats an absent filter as "no restriction". `contains`/`metadata` are best-effort
/// (see [`event_log`] docs) — the platform's `Отбор` structure does not cover a free-text
/// substring, so the extension applies `contains` as a post-read row filter.
#[derive(Debug, Default)]
pub struct EventLogQuery {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub level: Option<String>,
    pub user: Option<String>,
    pub event: Option<String>,
    pub metadata: Option<String>,
    pub contains: Option<String>,
    pub limit: Option<u32>,
    pub connection: Option<String>,
}

fn build_request(query: EventLogQuery) -> onec_client::EventLogRequest {
    onec_client::EventLogRequest {
        date_from: query.date_from,
        date_to: query.date_to,
        level: query.level,
        user: query.user,
        event: query.event,
        metadata: query.metadata,
        contains: query.contains,
        limit: query.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT),
    }
}

/// Reads the 1C event log (журнал регистрации) via the `BSL_Analyzer` extension.
///
/// Records come back newest-first, capped by `limit`. `contains` is a case-insensitive
/// substring filter the extension applies to the comment/data columns after the read, since
/// the platform's `Отбор` structure has no free-text dimension. Requires `--onec-url` and a
/// connecting user that holds the `EventLog` right.
pub async fn event_log(
    state: &SharedState,
    query: EventLogQuery,
) -> Result<CallToolResult, McpError> {
    let selected = state
        .onec_connection(query.connection.as_deref())
        .map_err(|e| McpError::invalid_params(e, None))?;

    let request = build_request(query);

    let result = selected.client().event_log(&request).await.map_err(|e| {
        McpError::internal_error(format!("Ошибка чтения журнала регистрации в 1С: {e}"), None)
    })?;

    Ok(structured(json!({
        "columns": result.columns,
        "rows": result.rows,
        "total": result.total,
        "truncated": result.truncated,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_defaults_and_clamps() {
        assert_eq!(build_request(EventLogQuery::default()).limit, DEFAULT_LIMIT);
        assert_eq!(build_request(EventLogQuery { limit: Some(5), ..Default::default() }).limit, 5);
        assert_eq!(
            build_request(EventLogQuery { limit: Some(99_999), ..Default::default() }).limit,
            MAX_LIMIT
        );
    }

    #[test]
    fn filters_pass_through() {
        let req = build_request(EventLogQuery {
            level: Some("Ошибка".into()),
            user: Some("Администратор".into()),
            contains: Some("оштрафован".into()),
            ..Default::default()
        });
        assert_eq!(req.level.as_deref(), Some("Ошибка"));
        assert_eq!(req.user.as_deref(), Some("Администратор"));
        assert_eq!(req.contains.as_deref(), Some("оштрафован"));
    }

    #[test]
    fn omitted_filters_are_not_serialized() {
        let req = build_request(EventLogQuery {
            level: Some("Ошибка".into()),
            limit: Some(10),
            ..Default::default()
        });
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(body["level"], "Ошибка");
        assert_eq!(body["limit"], 10);
        assert!(body.get("date_from").is_none(), "absent filter must be omitted: {body}");
        assert!(body.get("user").is_none());
    }

    #[tokio::test]
    async fn errors_without_onec_client() {
        let state = SharedState::shared();
        let result = event_log(&state, EventLogQuery::default()).await;
        assert!(result.is_err(), "should fail without onec client");
    }
}
