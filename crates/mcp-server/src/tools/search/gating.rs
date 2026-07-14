use crate::baseline::{ConfiguredBaselineStatus, ExternalBaselineService};
use crate::state::WorkspaceSearchMode;
use bsl_search::SearchError;
use rmcp::ErrorData as McpError;
use serde_json::json;
use std::sync::Arc;
use tracing::warn;

pub(super) fn map_reference_baseline_resolution<T>(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    resolution: Result<Option<T>, SearchError>,
    failure_message: &'static str,
) -> Result<Option<T>, McpError> {
    match resolution {
        Ok(Some(value)) => Ok(Some(value)),
        Ok(None) => {
            if configured_baseline.is_some_and(|baseline| baseline.backend == "postgres") {
                return Err(reference_baseline_unavailable_mcp_error(configured_baseline));
            }
            Ok(None)
        }
        Err(error) => {
            if error.is_terminal() {
                return Err(external_baseline_mcp_error(&error));
            }
            warn!(%error, "{failure_message}");
            Ok(None)
        }
    }
}

pub(super) fn reference_baseline_unavailable_mcp_error(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
) -> McpError {
    let selection = configured_baseline
        .map(|baseline| baseline.selection.as_str())
        .unwrap_or("configured shared reference baseline");
    McpError::invalid_params(
        format!(
            "Shared reference baseline is unavailable. No published snapshot matched {selection}. Publish the selected baseline, restart MCP, and retry."
        ),
        Some(json!({
            "reasonCode": "baseline_unavailable",
            "selection": selection,
        })),
    )
}

pub(super) fn external_baseline_mcp_error(error: &SearchError) -> McpError {
    let reason_code = error.reason_code();
    let details = json!({
        "reasonCode": reason_code,
        "details": error.to_string(),
    });

    let invalid_params_reason = matches!(
        reason_code,
        Some(
            "helper_spawn_failed"
                | "helper_timeout"
                | "helper_protocol_error"
                | "helper_rejected"
                | "resolved_target_mismatch"
                | "missing_config"
                | "storage_not_initialized"
                | "schema_version_mismatch"
        )
    );

    if invalid_params_reason {
        return McpError::invalid_params(
            format!("external baseline error: {error}"),
            Some(details),
        );
    }

    McpError::internal_error(format!("external baseline error: {error}"), Some(details))
}

pub(super) fn ensure_workspace_search_allowed(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
) -> Result<(), McpError> {
    let Some(configured_baseline) = configured_baseline else {
        return Ok(());
    };
    let Some(support) = configured_baseline.support.as_ref() else {
        return Ok(());
    };

    if !configured_baseline.search_is_expired() {
        return Ok(());
    }

    Err(McpError::invalid_params(
        format!(
            "Shared baseline access is expired for this branch. {}. Update the branch from develop, restart MCP, and retry.",
            support.reason
        ),
        Some(json!({
            "reasonCode": "expired_branch",
            "supportState": support.state.as_str(),
            "workspaceBranch": support.workspace_branch,
            "selectedBranch": support.selected_branch,
            "snapshotAgeDays": support.snapshot_age_days,
            "staleAfterDays": support.stale_after_days,
            "expireAfterDays": support.expire_after_days
        })),
    ))
}

pub(super) fn ensure_workspace_baseline_runtime_ready(
    workspace_search_mode: WorkspaceSearchMode,
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<&Arc<ExternalBaselineService>>,
) -> Result<(), McpError> {
    if matches!(workspace_search_mode, WorkspaceSearchMode::SqliteLocal) {
        return Ok(());
    }
    let Some(configured_baseline) = configured_baseline else {
        return Err(McpError::invalid_params(
            "Postgres workspace mode requires a configured PostgreSQL baseline. Restart MCP in sqlite mode if PostgreSQL is not intended."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_required",
            })),
        ));
    };
    if configured_baseline.backend != "postgres" {
        return Ok(());
    }
    if let Some(issue) = configured_baseline.issue.as_deref() {
        return Err(McpError::invalid_params(
            format!(
                "Shared baseline is unavailable. Fix the PostgreSQL baseline configuration, restart MCP, and retry. {issue}"
            ),
            Some(json!({
                "reasonCode": "baseline_unavailable",
                "details": issue,
            })),
        ));
    }
    if external_baseline.is_none() {
        return Err(McpError::invalid_params(
            "Shared baseline is unavailable. Restart MCP after fixing the PostgreSQL baseline configuration and retry."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_unavailable",
            })),
        ));
    }
    Ok(())
}

pub(super) fn ensure_reference_baseline_runtime_ready(
    configured_baseline: Option<&ConfiguredBaselineStatus>,
    external_baseline: Option<&Arc<ExternalBaselineService>>,
) -> Result<(), McpError> {
    let Some(configured_baseline) = configured_baseline else {
        return Ok(());
    };
    if configured_baseline.backend != "postgres" {
        return Ok(());
    }
    if let Some(issue) = configured_baseline.issue.as_deref() {
        return Err(McpError::invalid_params(
            format!(
                "Shared reference baseline is unavailable. Fix the PostgreSQL baseline configuration, restart MCP, and retry. {issue}"
            ),
            Some(json!({
                "reasonCode": "baseline_unavailable",
                "details": issue,
            })),
        ));
    }
    if external_baseline.is_none() {
        return Err(McpError::invalid_params(
            "Shared reference baseline is unavailable. Restart MCP after fixing the PostgreSQL baseline configuration and retry."
                .to_owned(),
            Some(json!({
                "reasonCode": "baseline_unavailable",
            })),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{external_baseline_mcp_error, map_reference_baseline_resolution};
    use crate::baseline::ConfiguredBaselineStatus;
    use bsl_search::SearchError;
    use rmcp::model::ErrorCode;

    #[test]
    fn missing_reference_snapshot_maps_to_baseline_unavailable_error() {
        let error = map_reference_baseline_resolution::<()>(
            Some(&ConfiguredBaselineStatus {
                backend: "postgres",
                selection: "snapshot reference:0.1.104".to_owned(),
                issue: None,
                support: None,
            }),
            Ok(None),
            "test reference snapshot resolution",
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
        assert!(error.message.contains("Shared reference baseline is unavailable"));
        assert!(error.message.contains("snapshot reference:0.1.104"));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("baseline_unavailable")
        );
    }

    #[test]
    fn missing_reference_snapshot_still_allows_local_sqlite_mode() {
        let result = map_reference_baseline_resolution::<()>(
            Some(&ConfiguredBaselineStatus {
                backend: "sqlite",
                selection: "local reference index".to_owned(),
                issue: None,
                support: None,
            }),
            Ok(None),
            "test reference snapshot resolution",
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn external_baseline_storage_error_maps_to_invalid_params() {
        let error = SearchError::StorageNotInitialized { schema: "bsl_search".to_owned() };
        let mcp_error = external_baseline_mcp_error(&error);

        assert_eq!(mcp_error.code, ErrorCode::INVALID_PARAMS);
        assert!(mcp_error.message.contains("external baseline error"));
        assert_eq!(
            mcp_error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("storage_not_initialized")
        );
    }

    #[test]
    fn external_baseline_connectivity_error_maps_to_internal_error() {
        let error =
            SearchError::ExternalBaseline("postgres_connect_failed: connection refused".to_owned());
        let mcp_error = external_baseline_mcp_error(&error);

        assert_eq!(mcp_error.code, ErrorCode::INTERNAL_ERROR);
        assert!(mcp_error.message.contains("external baseline error"));
        assert_eq!(
            mcp_error
                .data
                .as_ref()
                .and_then(|data| data.get("reasonCode"))
                .and_then(|value| value.as_str()),
            Some("postgres_connect_failed")
        );
    }
}
