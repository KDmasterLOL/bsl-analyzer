//! End-to-end `metadata` over the real MCP transport, covering how a consumer tells
//! "not ready yet" from an answer.
//!
//! The distinction has to be readable from a field. A retry envelope a consumer fails to
//! recognize is handed to a model as prose: it spends a turn on "повторите запрос" or, worse,
//! reads it as the answer and concludes the object does not exist. So this suite classifies
//! responses exactly the way a client must — by `structuredContent.status` — and never by
//! matching the sentence, which can also occur inside a module body and reach a tool's output
//! as a genuine result.

use std::path::Path;
use std::time::Duration;

use mcp_server::{serve_stream, McpProfile, McpServer, SharedState};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Map, Value};
use tempfile::TempDir;

type Client = RunningService<RoleClient, ()>;

/// A minimal but valid configuration in a scratch dir, so the resident's on-disk caches never
/// land in the repo tree and every run starts cold.
fn stage_workspace() -> TempDir {
    let dir = TempDir::new().expect("scratch workspace");
    let root = dir.path();
    std::fs::write(
        root.join("Configuration.xml"),
        "<Configuration><Name>ТестоваяКонфигурация</Name></Configuration>",
    )
    .expect("write Configuration.xml");
    let module = root.join("CommonModules").join("Сервер").join("Ext");
    std::fs::create_dir_all(&module).expect("mkdir module");
    std::fs::write(
        root.join("CommonModules").join("Сервер.xml"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\">\n\
         \t<CommonModule uuid=\"00000000-0000-0000-0000-000000000001\">\n\
         \t\t<Properties><Name>Сервер</Name><Server>true</Server></Properties>\n\
         \t</CommonModule>\n\
         </MetaDataObject>\n",
    )
    .expect("write module descriptor");
    std::fs::write(
        module.join("Module.bsl"),
        "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
    )
    .expect("write module body");
    dir
}

async fn workspace_client(root: &Path) -> Client {
    let server = McpServer::new(
        McpProfile::Workspace,
        SharedState::workspace(root.to_path_buf()).expect("valid workspace project"),
    );
    let (client_io, server_io) = tokio::io::duplex(4 * 1024 * 1024);
    tokio::spawn(serve_stream(server, server_io));
    ().serve(client_io).await.expect("session initialized")
}

async fn call(client: &Client, args: &[(&str, Value)]) -> CallToolResult {
    let args: Map<String, Value> =
        args.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
    client
        .call_tool(CallToolRequestParams::new("metadata").with_arguments(args))
        .await
        .expect("transport ok")
}

fn text_of(result: &CallToolResult) -> &str {
    result.content[0].raw.as_text().expect("text content").text.as_str()
}

/// A call issued while the resident builds answers with the retry envelope, and one issued
/// after it is ready answers with the configuration summary — both distinguishable by
/// `structuredContent.status` alone. Reverting the envelope to a bare sentence leaves the
/// first response indistinguishable from the second for anything but a phrase match.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cold_info_call_is_classifiable_by_status_alone() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut saw_envelope = false;
    loop {
        let result = call(&client, &[("action", json!("info"))]).await;
        let loading =
            result.structured_content.as_ref().is_some_and(|body| body["status"] == "loading");
        if !loading {
            let text = text_of(&result);
            assert!(
                text.starts_with("# Конфигурация:") && text.contains("Общие модули: 1"),
                "a ready `info` answers with the configuration summary: {text}",
            );
            // The resident is idle until a tool call kicks it, and the answer is computed on
            // the request thread immediately after that kick — so the first call always lands
            // mid-build and the envelope branch above is genuinely exercised, not skipped by a
            // build that happened to finish first.
            assert!(saw_envelope, "the cold first call must answer with the retry envelope");
            break;
        }

        saw_envelope = true;
        let body = result.structured_content.as_ref().expect("the envelope is structured");
        assert!(
            body["detail"].as_str().is_some_and(|detail| !detail.is_empty()),
            "the retry envelope says why it is not ready: {body}",
        );
        assert!(
            text_of(&result).starts_with("Метаданные загружаются"),
            "the human sentence stays alongside the envelope: {}",
            text_of(&result),
        );
        assert!(tokio::time::Instant::now() < deadline, "the resident never became ready: {body}",);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `status` answers off the lifecycle, not off a built resident, so it must report a state
/// immediately — that is what makes polling it cheaper than firing `info` and reading the
/// envelope. Its shape is the resident's, shared with `diagnostics status`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_reports_the_resident_lifecycle_right_away() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let result = call(&client, &[("action", json!("status"))]).await;
    let body = result.structured_content.as_ref().expect("status is structured");

    let state = body["state"].as_str().expect("status reports a state");
    assert!(
        matches!(state, "idle" | "loading" | "ready"),
        "an unexpected lifecycle state for a live workspace: {body}",
    );
    assert!(body["generation"].is_number(), "status carries the resident generation: {body}");
    assert!(body["reload"].is_string(), "status carries the reload state: {body}");
}

/// Readiness is a property of this server, not of the requested mode. A client with named 1C
/// connections passes `connection` on every metadata call; if `status` were routed behind the
/// infobase branch it would answer "action unavailable in infobase mode" for that client only,
/// which reads as "this build has no status action" rather than "wrong mode".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn status_answers_even_when_a_connection_is_passed() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let result =
        call(&client, &[("action", json!("status")), ("connection", json!("производственная"))])
            .await;
    let body = result.structured_content.as_ref().expect("status is structured");

    assert!(body["state"].is_string(), "status still reports the resident lifecycle: {body}");
}
