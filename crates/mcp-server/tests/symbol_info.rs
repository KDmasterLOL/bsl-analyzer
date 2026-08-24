//! End-to-end `symbol_info` over the real MCP transport: a workspace-profile server
//! answers a qualified-name lookup with a consolidated semantic card through an rmcp
//! client, and honours the tool's documented contracts — param validation, output
//! budget, and workspace-only availability — across serialization and tool dispatch.
//!
//! The card-resolution logic itself is unit-tested in `ide::symbol_info` (see
//! `crates/ide/tests/symbol_info.rs`); this suite covers the MCP layer those tests do
//! not exercise: the resident lifecycle (loading envelope → ready), the JSON shaping,
//! and the profile gating, driven through a genuine `serve_stream` handshake.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mcp_server::{serve_stream, McpProfile, McpServer, SharedState};
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value};
use tempfile::TempDir;

type Client = RunningService<RoleClient, ()>;

fn designer_fixture() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

/// Copy the checked-in metadata fixture into a scratch dir so the resident host's on-disk
/// caches (graph/search) never land in the repo tree, and each run starts cold.
fn stage_workspace() -> TempDir {
    let src = designer_fixture();
    let dst = TempDir::new().expect("scratch workspace");
    for entry in walkdir::WalkDir::new(&src) {
        let entry = entry.expect("walk fixture");
        let rel = entry.path().strip_prefix(&src).expect("path under fixture root");
        let target = dst.path().join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).expect("mkdir");
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).expect("mkdir parent");
            }
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
    dst
}

/// A workspace-profile server reachable over an in-memory duplex, exactly how the daemon
/// serves a proxy from one `SharedState` (mirrors the broker concurrency test).
async fn workspace_client(root: &Path) -> Client {
    let server = McpServer::new(
        McpProfile::Workspace,
        SharedState::workspace(root.to_path_buf()).expect("valid workspace project"),
    );
    // The buffer must exceed the largest single response: an in-process duplex has no kernel
    // backpressure, so an oversized frame would wedge the pipe (a harness artifact, not the
    // socket transport the daemon uses).
    let (client_io, server_io) = tokio::io::duplex(4 * 1024 * 1024);
    tokio::spawn(serve_stream(server, server_io));
    ().serve(client_io).await.expect("session initialized")
}

fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// Call `symbol_info` and return the parsed card, retrying past the "still loading"
/// envelope until the resident is ready or the budget runs out. The envelope is told apart
/// by its `status: "loading"` field — the way a consumer is meant to read it. Matching the
/// human sentence, or inferring "not ready" from an absent envelope, is what this suite
/// must not model: a resolved card, a resident miss AND the retry envelope all carry
/// structured content.
async fn poll_card(client: &Client, call_args: Map<String, Value>) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        let call = CallToolRequestParams::new("symbol_info").with_arguments(call_args.clone());
        let result = client.call_tool(call).await.expect("transport ok");
        if let Some(structured) = result.structured_content {
            if structured["status"] != "loading" {
                return structured;
            }
            assert_eq!(structured["schema_version"], "1");
            assert!(structured["detail"].is_string());
            assert!(structured["state"].is_string());
            assert!(structured["generation"].is_u64());
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "resident never became ready for {call_args:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn symbol_info_serves_semantic_cards_over_the_transport() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let listed = client.list_tools(Default::default()).await.expect("tools/list");
    let tool = listed.tools.iter().find(|tool| tool.name == "symbol_info").unwrap();
    let schema = tool.output_schema.as_ref().expect("symbol_info outputSchema");
    let encoded = serde_json::to_string(schema).unwrap();
    for value in
        ["ok", "not_found", "ambiguous", "loading", "availability", "type_variants", "signature"]
    {
        assert!(encoded.contains(value), "missing {value} in tools/list outputSchema");
    }

    // A qualified metadata-object name resolves to a full card end-to-end: kind, container,
    // and the object's members straight from the metadata substrate.
    let object =
        poll_card(&client, args(&[("symbol", Value::from("Справочник.Справочник1"))])).await;
    assert_eq!(object["schema_version"], "1");
    assert_eq!(object["status"], "ok");
    assert_eq!(object["symbol"], "Справочник.Справочник1");
    assert_eq!(object["kind"], "metadata object");
    assert_eq!(object["container"]["kind"], "Справочник");
    let members = object["members"].as_array().expect("object lists members");
    assert!(
        members.iter().any(|m| m["name"] == "Реквизит1" && m["kind"] == "Реквизит"),
        "members were {members:?}"
    );
    let attribute_member = members.iter().find(|m| m["name"] == "Реквизит1").unwrap();
    assert_eq!(attribute_member["member_kind"], "attribute");
    assert_eq!(attribute_member["origin"], "metadata");
    assert_eq!(attribute_member["availability"]["context_status"], "not_evaluated");
    assert!(attribute_member["availability"]["contexts"].is_null());
    let variants = attribute_member["type_variants"].as_array().unwrap();
    assert!(!variants.is_empty());
    assert!(variants.iter().all(|variant| variant["resolution"] == "static"));
    assert!(variants.iter().all(|variant| variant["technical_name"].is_string()));
    assert!(attribute_member.get("signature").is_none());

    for symbol in [
        "СправочникОбъект.Справочник1",
        "СправочникСсылка.Справочник1",
        "СправочникМенеджер.Справочник1",
        "ОбработкаОбъект.ТестоваяОбработка",
    ] {
        let facet = poll_card(&client, args(&[("symbol", Value::from(symbol))])).await;
        assert_eq!(facet["symbol"], symbol, "facet card: {facet:?}");
    }

    // An attribute of that object carries its type and ownership from the substrate — the
    // metadata-member path the issue calls out explicitly.
    let attribute =
        poll_card(&client, args(&[("symbol", Value::from("Справочник.Справочник1.Реквизит1"))]))
            .await;
    assert_eq!(attribute["kind"], "attribute");
    assert!(attribute.get("return_type").is_some(), "attribute carries its type: {attribute:?}");
    assert_eq!(attribute["container"]["kind"], "Справочник");
    assert_eq!(attribute["container"]["name"], "Справочник1");

    // The output budget is honoured over the wire: a tiny budget trims the member list and
    // stamps the card `truncated`.
    let trimmed = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("Справочник.Справочник1")),
            ("max_output_tokens", Value::from(1)),
        ]),
    )
    .await;
    assert_eq!(trimmed["truncated"], true, "tiny budget trims the card: {trimmed:?}");

    let form_args = args(&[
        ("symbol", Value::from("Документ.Документ1.Форма.ФормаДокумента")),
        ("max_output_tokens", Value::from(1_000)),
    ]);
    let first_form = poll_card(&client, form_args.clone()).await;
    let second_form = poll_card(&client, form_args).await;
    assert_eq!(first_form["truncated"], true, "managed form exceeds the test budget");
    let members = first_form["members"].as_array().expect("budget keeps a member prefix");
    assert!(!members.is_empty());
    assert_eq!(members, second_form["members"].as_array().unwrap());

    // An imprecise name that no resident symbol matches is a structured "not resolved"
    // envelope, never a transport error.
    let miss =
        poll_card(&client, args(&[("symbol", Value::from("НетТакогоМодуля.НетМетода"))])).await;
    assert_eq!(miss["schema_version"], "1", "versioned miss: {miss:?}");
    assert_eq!(miss["status"], "not_found", "versioned miss: {miss:?}");
    assert_eq!(miss["resolved"], false, "unknown name is a structured miss: {miss:?}");

    for symbol in ["НеизвестнаяГрань.Справочник1", "СправочникОбъект.НетТакого"]
    {
        let miss = poll_card(&client, args(&[("symbol", Value::from(symbol))])).await;
        assert_eq!(miss["schema_version"], "1");
        assert_eq!(miss["status"], "not_found");
        assert_eq!(miss["symbol"], symbol);
    }

    let malformed = client
        .call_tool(
            CallToolRequestParams::new("symbol_info")
                .with_arguments(args(&[("symbol", Value::from("СправочникОбъект..Справочник1"))])),
        )
        .await;
    assert!(malformed.is_err(), "malformed symbol must be invalid params: {malformed:?}");

    let path = "Catalogs/Справочник1/Ext/ObjectModule.bsl";
    for positional in [
        args(&[("path", Value::from(path)), ("line", Value::from(0))]),
        args(&[
            ("root_id", Value::from("")),
            ("path", Value::from(path)),
            ("line", Value::from(0)),
        ]),
    ] {
        let _ = poll_card(&client, positional).await;
    }
    let object_facet = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("max_output_tokens", Value::from(60_000)),
        ]),
    )
    .await;
    let facet_members = object_facet["members"].as_array().unwrap();
    let sample_name = facet_members[0]["name"].as_str().unwrap();
    let sample_kind = facet_members[0]["member_kind"].as_str().unwrap();
    let by_kind = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("member_kind", Value::from(sample_kind)),
        ]),
    )
    .await;
    assert!(by_kind["members"]
        .as_array()
        .unwrap()
        .iter()
        .all(|member| member["member_kind"] == sample_kind));
    let by_name = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("member_name", Value::from(sample_name.to_lowercase())),
        ]),
    )
    .await;
    let named = by_name["members"].as_array().unwrap();
    assert!(!named.is_empty());
    assert!(named.iter().all(|member| {
        member["name"]
            .as_str()
            .is_some_and(|name| name.to_lowercase() == sample_name.to_lowercase())
    }));
    let legacy_include = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("include", Value::Array(vec!["definition".into(), "type".into(), "doc".into()])),
            ("max_output_tokens", Value::from(60_000)),
        ]),
    )
    .await;
    assert_eq!(legacy_include["members"].as_array().unwrap().len(), facet_members.len());
    let contextual = poll_card(
        &client,
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("path", Value::from(path)),
            ("line", Value::from(0)),
            ("column", Value::from(0)),
            ("max_output_tokens", Value::from(60_000)),
        ]),
    )
    .await;
    let contextual_members = contextual["members"].as_array().unwrap();
    assert_eq!(
        contextual_members.len(),
        object_facet["members"].as_array().unwrap().len(),
        "position must not filter members"
    );
    assert!(contextual_members.iter().all(|member| {
        matches!(
            member["availability"]["context_status"].as_str(),
            Some("available" | "unavailable" | "unknown")
        )
    }));

    for incomplete in [
        args(&[("path", Value::from(path))]),
        args(&[("line", Value::from(0))]),
        args(&[
            ("symbol", Value::from("СправочникОбъект.Справочник1")),
            ("root_id", Value::from("")),
        ]),
    ] {
        let result = client
            .call_tool(CallToolRequestParams::new("symbol_info").with_arguments(incomplete))
            .await;
        assert!(result.is_err(), "incomplete positional context must be rejected: {result:?}");
    }

    // A call with neither `symbol` nor `path` is a parameter error, surfaced as a
    // JSON-RPC error through the transport.
    let bad = client
        .call_tool(CallToolRequestParams::new("symbol_info").with_arguments(Map::new()))
        .await;
    assert!(bad.is_err(), "missing symbol/path must be a param error, got {bad:?}");

    client.cancel().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn symbol_info_is_workspace_only() {
    // The reference profile has no resident analysis host, so `symbol_info` is not part of
    // its tool surface — a call must be rejected, not served.
    let server = McpServer::new(McpProfile::Reference, SharedState::reference(None));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve_stream(server, server_io));
    let client = ().serve(client_io).await.expect("reference session initialized");

    let call = CallToolRequestParams::new("symbol_info")
        .with_arguments(args(&[("symbol", Value::from("Справочник.Справочник1"))]));
    let result = client.call_tool(call).await;
    assert!(result.is_err(), "reference profile must not serve symbol_info, got {result:?}");

    client.cancel().await.ok();
}
