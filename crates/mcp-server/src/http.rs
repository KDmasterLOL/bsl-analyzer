use std::{net::SocketAddr, sync::Arc, time::Instant};

use axum::{
    extract::State,
    http::{header::HOST, uri::Authority, HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::limit::RequestBodyLimitLayer;

use crate::{McpProfile, McpServer};

/// Maximum accepted HTTP request body. MCP requests are small JSON-RPC
/// envelopes; a finite limit prevents unbounded buffering before dispatch.
pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Authorities accepted when the operator passed no explicit allowlist. Mirrors
/// rmcp's own default, so an unconfigured server answers the same set of names on
/// both routes.
const DEFAULT_ALLOWED_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

#[derive(Clone)]
struct HealthState {
    profile: McpProfile,
    address: SocketAddr,
    started_at: Instant,
    allowed_hosts: Arc<[String]>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    profile: &'static str,
    mode: &'static str,
    host: String,
    port: u16,
    pid: u32,
    uptime_seconds: u64,
}

/// Split an authority into a comparable host and an optional port, normalizing the
/// way rmcp does: IPv6 brackets stripped and the host lowercased. A value that is
/// not a valid authority is still comparable as a bare host name.
fn split_authority(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let normalize = |host: &str| host.trim_matches('[').trim_matches(']').to_ascii_lowercase();
    match Authority::try_from(value) {
        Ok(authority) => Some((normalize(authority.host()), authority.port_u16())),
        Err(_) => Some((normalize(value), None)),
    }
}

/// Whether a request's `Host` is in the allowlist, matching rmcp's rule: an entry
/// without a port accepts any port. rmcp keeps its copy of this check private to
/// the MCP route, so `/health` needs its own to sit behind the same anti-DNS-rebinding
/// gate rather than beside it.
fn host_is_allowed(host_header: &str, allowed_hosts: &[String]) -> bool {
    let Some((host, port)) = split_authority(host_header) else {
        return false;
    };
    allowed_hosts.iter().filter_map(|allowed| split_authority(allowed)).any(
        |(allowed_host, allowed_port)| {
            allowed_host == host && allowed_port.is_none_or(|expected| port == Some(expected))
        },
    )
}

async fn health(
    State(state): State<HealthState>,
    headers: HeaderMap,
) -> Result<Json<HealthResponse>, StatusCode> {
    let host = headers.get(HOST).and_then(|value| value.to_str().ok()).unwrap_or_default();
    if !host_is_allowed(host, &state.allowed_hosts) {
        tracing::warn!(host, "rejected /health request with disallowed Host header");
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        profile: state.profile.as_str(),
        mode: "http",
        host: state.address.ip().to_string(),
        port: state.address.port(),
        pid: std::process::id(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
    }))
}

/// Serve stateful MCP sessions and a readiness endpoint on an already-bound
/// listener. Binding remains the caller's responsibility so startup can fail
/// before expensive shared state is constructed.
pub async fn serve_http(
    listener: TcpListener,
    server: McpServer,
    profile: McpProfile,
    address: SocketAddr,
    allowed_hosts: Vec<String>,
    cancellation: CancellationToken,
) -> anyhow::Result<()> {
    // One list drives both routes, so the MCP endpoint and the readiness endpoint
    // cannot drift apart on which authorities they answer.
    let allowed_hosts: Arc<[String]> = if allowed_hosts.is_empty() {
        DEFAULT_ALLOWED_HOSTS.iter().map(|host| (*host).to_owned()).collect()
    } else {
        allowed_hosts.into()
    };
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancellation.clone())
        .with_allowed_hosts(allowed_hosts.iter().cloned());
    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let health_state = HealthState { profile, address, started_at: Instant::now(), allowed_hosts };

    let app = Router::new()
        .route_service("/mcp", mcp)
        .route("/health", get(health))
        .with_state(health_state)
        .layer(RequestBodyLimitLayer::new(MAX_HTTP_REQUEST_BODY_BYTES));

    axum::serve(listener, app).with_graceful_shutdown(cancellation.cancelled_owned()).await?;
    Ok(())
}
