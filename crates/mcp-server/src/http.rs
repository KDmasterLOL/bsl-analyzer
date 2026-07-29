use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::HOST, uri::Authority, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
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

/// How long a cancelled server waits for requests already in flight. Reading a
/// request body observes no cancellation token, so a client that sends headers and
/// then stalls would otherwise hold the process — and its single-instance lock —
/// open indefinitely.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct HealthState {
    profile: McpProfile,
    address: SocketAddr,
    started_at: Instant,
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

fn normalize_host(host: &str) -> String {
    host.trim_matches('[').trim_matches(']').to_ascii_lowercase()
}

/// Parse a `Host` header strictly, as rmcp does. Leniency belongs to the allowlist,
/// never to the request: normalization strips brackets, so accepting a malformed value
/// as a bare name would let `[[localhost]]` match an allowed `localhost`, widening the
/// very gate that is supposed to narrow things.
fn parse_request_authority(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let authority = Authority::try_from(value).ok()?;
    Some((normalize_host(authority.host()), authority.port_u16()))
}

/// Parse one allowlist entry, which the operator writes by hand and may give as a bare
/// name rather than a full authority.
fn parse_allowed_authority(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    match Authority::try_from(value) {
        Ok(authority) => Some((normalize_host(authority.host()), authority.port_u16())),
        Err(_) => Some((normalize_host(value), None)),
    }
}

/// Whether a request's `Host` is in the allowlist, matching rmcp's rule: an entry
/// without a port accepts any port. rmcp keeps its copy of this check private to
/// the MCP route, so `/health` needs its own to sit behind the same anti-DNS-rebinding
/// gate rather than beside it.
fn host_is_allowed(host_header: &str, allowed_hosts: &[String]) -> bool {
    let Some((host, port)) = parse_request_authority(host_header) else {
        return false;
    };
    allowed_hosts.iter().filter_map(|allowed| parse_allowed_authority(allowed)).any(
        |(allowed_host, allowed_port)| {
            allowed_host == host && allowed_port.is_none_or(|expected| port == Some(expected))
        },
    )
}

/// Refuse a disallowed `Host` before anything reads the request body.
///
/// rmcp applies this check inside its own service, which reaches only `/mcp` and only
/// once the body has been collected. Gating the whole router here puts the readiness
/// endpoint behind the same anti-DNS-rebinding rule, and stops a refused client from
/// holding a connection open by never finishing the body it announced.
async fn host_gate(
    State(allowed_hosts): State<Arc<[String]>>,
    request: Request,
    next: Next,
) -> Response {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    if parse_request_authority(&host).is_none() {
        tracing::warn!(host, "rejected request with a malformed Host header");
        return StatusCode::BAD_REQUEST.into_response();
    }
    if !host_is_allowed(&host, &allowed_hosts) {
        tracing::warn!(host, "rejected request with disallowed Host header");
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

async fn health(State(state): State<HealthState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        profile: state.profile.as_str(),
        mode: "http",
        host: state.address.ip().to_string(),
        port: state.address.port(),
        pid: std::process::id(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
    })
}

/// The authorities this server answers. One list drives both routes, so the MCP
/// endpoint and the readiness endpoint cannot drift apart on what they accept.
///
/// With no operator allowlist the bound address joins the defaults: the CLI treats
/// every address in 127.0.0.0/8 as safe to bind without one, while rmcp's defaults name
/// only `127.0.0.1`, so a server on `127.0.0.2` would otherwise refuse its own address.
fn effective_allowed_hosts(address: SocketAddr, allowed_hosts: Vec<String>) -> Arc<[String]> {
    if allowed_hosts.is_empty() {
        DEFAULT_ALLOWED_HOSTS
            .iter()
            .map(|host| (*host).to_owned())
            .chain(std::iter::once(address.ip().to_string()))
            .collect()
    } else {
        allowed_hosts.into()
    }
}

/// Enforce the body limit ourselves so the answer does not depend on framing.
/// `RequestBodyLimitLayer` only synthesizes 413 when `Content-Length` already exceeds
/// the limit; a chunked body of the same size fails mid-read and reaches the client as
/// rmcp's generic 500.
async fn limit_request_body(request: Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_HTTP_REQUEST_BODY_BYTES).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    next.run(Request::from_parts(parts, Body::from(bytes))).await
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
    let allowed_hosts = effective_allowed_hosts(address, allowed_hosts);
    let config = StreamableHttpServerConfig::default()
        .with_cancellation_token(cancellation.clone())
        .with_allowed_hosts(allowed_hosts.iter().cloned());
    let mcp = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let health_state = HealthState { profile, address, started_at: Instant::now() };

    let app = Router::new()
        .route_service("/mcp", mcp)
        .route("/health", get(health))
        .with_state(health_state)
        .layer(middleware::from_fn(limit_request_body))
        .layer(RequestBodyLimitLayer::new(MAX_HTTP_REQUEST_BODY_BYTES))
        .layer(middleware::from_fn_with_state(Arc::clone(&allowed_hosts), host_gate));

    // `IntoFuture` rather than a bare await: the shutdown grace below needs to select
    // against this future, and drop it if the grace runs out.
    let serve = std::future::IntoFuture::into_future(
        axum::serve(listener, app).with_graceful_shutdown(cancellation.clone().cancelled_owned()),
    );
    tokio::pin!(serve);

    tokio::select! {
        result = &mut serve => result?,
        () = grace_after_cancel(&cancellation) => {
            tracing::warn!(
                grace_seconds = SHUTDOWN_GRACE.as_secs(),
                "MCP HTTP shutdown grace elapsed with requests still in flight; dropping them"
            );
        }
    }
    Ok(())
}

async fn grace_after_cancel(cancellation: &CancellationToken) {
    cancellation.cancelled().await;
    tokio::time::sleep(SHUTDOWN_GRACE).await;
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, SocketAddr};

    use super::{effective_allowed_hosts, host_is_allowed};

    fn address(ip: &str, port: u16) -> SocketAddr {
        SocketAddr::new(ip.parse::<IpAddr>().expect("test address should parse"), port)
    }

    #[test]
    fn a_server_on_a_secondary_loopback_address_answers_its_own_host() {
        let allowed = effective_allowed_hosts(address("127.0.0.2", 8021), Vec::new());

        assert!(
            host_is_allowed("127.0.0.2:8021", &allowed),
            "a server the CLI let bind without an allowlist must answer its own address"
        );
        assert!(host_is_allowed("127.0.0.1:8021", &allowed), "the defaults must survive");
        assert!(!host_is_allowed("attacker.example", &allowed));
    }

    #[test]
    fn an_explicit_allowlist_replaces_the_defaults() {
        let allowed =
            effective_allowed_hosts(address("0.0.0.0", 8021), vec!["mcp.example.test".to_owned()]);

        assert!(host_is_allowed("mcp.example.test", &allowed));
        assert!(host_is_allowed("mcp.example.test:8021", &allowed), "a bare name accepts any port");
        assert!(
            !host_is_allowed("127.0.0.1:8021", &allowed),
            "naming an allowlist must not silently keep loopback"
        );
    }

    #[test]
    fn an_allowlist_entry_with_a_port_binds_that_port_only() {
        let allowed = effective_allowed_hosts(
            address("0.0.0.0", 8021),
            vec!["mcp.example.test:8021".to_owned()],
        );

        assert!(host_is_allowed("mcp.example.test:8021", &allowed));
        assert!(!host_is_allowed("mcp.example.test:9021", &allowed));
    }

    #[test]
    fn a_missing_or_unparsable_host_is_refused() {
        let allowed = effective_allowed_hosts(address("127.0.0.1", 8021), Vec::new());

        assert!(!host_is_allowed("", &allowed));
        assert!(!host_is_allowed("   ", &allowed));
    }

    #[test]
    fn a_malformed_host_cannot_normalize_into_an_allowed_one() {
        let allowed = effective_allowed_hosts(address("127.0.0.1", 8021), Vec::new());

        // None of these is a valid authority. Parsing the request leniently — treating
        // an unparsable value as a bare name — would strip the brackets off the first
        // two and match them against a legitimate allowlist entry.
        for malformed in ["[[localhost]]", "[[::1]]", "::1", "local host"] {
            assert!(
                !host_is_allowed(malformed, &allowed),
                "a malformed Host must not be normalized into an allowed one: {malformed}"
            );
        }

        // The well-formed spellings of the same authorities still pass. `[localhost]`
        // parses as an address literal and normalizes to `localhost`, which is rmcp's
        // rule for the MCP route; matching it here keeps the two gates from disagreeing.
        assert!(host_is_allowed("localhost", &allowed));
        assert!(host_is_allowed("[::1]:8021", &allowed));
        assert!(host_is_allowed("[localhost]", &allowed));
    }

    #[test]
    fn ipv6_loopback_matches_regardless_of_brackets() {
        let allowed = effective_allowed_hosts(address("::1", 8021), Vec::new());

        assert!(host_is_allowed("[::1]:8021", &allowed));
    }
}
