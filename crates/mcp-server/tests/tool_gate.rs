//! The launch-time tool gate over a real MCP session.
//!
//! The build ships no opt-in tool yet, so a test driven by the declaration alone would
//! pass against any implementation. These tests drive the gate directly instead: they hide
//! `syntax_help`, a real tool of the `reference` profile that answers from compile-time
//! platform tables without network, index or workspace, and check the surface a client
//! actually sees.

use mcp_server::{serve_stream, McpProfile, McpServer, SharedState, ToolGate};
use rmcp::model::CallToolRequestParams;
use rmcp::service::ServiceError;
use rmcp::ServiceExt;

/// A name no profile declares, used as the yardstick for "this tool does not exist".
const UNKNOWN_TOOL: &str = "no_such_tool_in_any_profile";

const GATED_TOOL: &str = "syntax_help";

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

async fn session(gate: &ToolGate) -> Client {
    let server = McpServer::with_gate(McpProfile::Reference, SharedState::reference(None), gate);
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(serve_stream(server, server_io));
    ().serve(client_io).await.expect("reference session initialized")
}

async fn tool_names(client: &Client) -> Vec<String> {
    let mut names: Vec<String> = client
        .list_tools(Default::default())
        .await
        .expect("tools/list")
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    names
}

/// The `code` and `message` a `tools/call` was refused with, or `None` if it was served.
async fn refusal(client: &Client, tool: &'static str) -> Option<(i32, String)> {
    match client.call_tool(CallToolRequestParams::new(tool)).await {
        Ok(_) => None,
        Err(ServiceError::McpError(error)) => Some((error.code.0, error.message.to_string())),
        Err(other) => panic!("expected an MCP error from {tool}, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_tool_is_absent_from_list() {
    let client = session(&ToolGate::hiding([GATED_TOOL])).await;
    assert!(
        !tool_names(&client).await.contains(&GATED_TOOL.to_owned()),
        "a gated tool must not appear in tools/list"
    );
    client.cancel().await.ok();
}

/// Calling a gated tool must be answered exactly as calling a name that was never built:
/// a client cannot tell the build could serve it. Equality is checked against a live call
/// with an undeclared name rather than against a hardcoded string, so an rmcp change that
/// reworded either path keeps the two in step or fails here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hidden_tool_call_is_indistinguishable_from_unknown() {
    let client = session(&ToolGate::hiding([GATED_TOOL])).await;
    let gated = refusal(&client, GATED_TOOL).await.expect("a gated tool must be refused");
    let unknown = refusal(&client, UNKNOWN_TOOL).await.expect("an unknown tool must be refused");
    assert_eq!(gated, unknown, "a gated tool must be refused exactly like an unknown one");
    client.cancel().await.ok();
}

/// Positive control for the two tests above: without the gate the same name is listed and
/// answers, while the undeclared name is still refused. Without this, both assertions
/// above would hold in a build that refuses everything.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enabled_tool_is_visible_and_callable() {
    let client = session(&ToolGate::default()).await;
    assert!(
        tool_names(&client).await.contains(&GATED_TOOL.to_owned()),
        "an ungated tool must appear in tools/list"
    );
    let served =
        client
            .call_tool(CallToolRequestParams::new(GATED_TOOL).with_arguments(
                serde_json::json!({ "name": "Массив" }).as_object().unwrap().clone(),
            ))
            .await
            .expect("an ungated tool must answer");
    assert_ne!(served.is_error, Some(true), "ungated call returned an error result");
    assert!(
        refusal(&client, UNKNOWN_TOOL).await.is_some(),
        "an undeclared name must stay refused when nothing is gated"
    );
    client.cancel().await.ok();
}

/// The branch a launch with the flag actually takes. `--enable-tool <default tool>` is an
/// identity operation, so the served surface must equal a launch without the flag; a gate
/// built as `declared \ enabled` would leave one tool listed and fail here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enabling_a_default_tool_changes_nothing() {
    let plain = session(&ToolGate::for_launch(McpProfile::Reference, &[])).await;
    let expected = tool_names(&plain).await;
    plain.cancel().await.ok();
    assert!(expected.contains(&GATED_TOOL.to_owned()), "fixture picked a tool nobody serves");

    let enabled =
        session(&ToolGate::for_launch(McpProfile::Reference, &[GATED_TOOL.to_owned()])).await;
    assert_eq!(tool_names(&enabled).await, expected);
    enabled.cancel().await.ok();
}

/// `McpServer::new` must serve the composition the declaration promises by default.
/// Today the opt-in set is empty, so this cannot fail; it is a forward guard that gains
/// teeth with the first opt-in tool, and it is not counted as a live check.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_construction_applies_the_declared_gate() {
    let plain = McpServer::new(McpProfile::Reference, SharedState::reference(None));
    let (client_io, server_io) = tokio::io::duplex(1024 * 1024);
    tokio::spawn(serve_stream(plain, server_io));
    let client: Client = ().serve(client_io).await.expect("reference session initialized");
    let served = tool_names(&client).await;
    client.cancel().await.ok();

    let gated = session(&ToolGate::for_launch(McpProfile::Reference, &[])).await;
    assert_eq!(served, tool_names(&gated).await);
    gated.cancel().await.ok();
}
