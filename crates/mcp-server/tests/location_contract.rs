//! The location contract across tools, over the real MCP transport.
//!
//! The unit suites check each tool's own shaping; what cannot be checked there is whether
//! the tools AGREE — that a pair one of them hands out addresses a file another one can
//! serve, and that two subsystems answering about one workspace name the same topology.
//! Those are the properties the contract exists for, and they are only observable end to
//! end.

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

/// Copy the checked-in metadata fixture into a scratch dir, so the derived caches never
/// land in the repo tree and each run starts cold.
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

/// The same fixture twice: once as the configuration, once as a declared extension beside
/// it. The point is a DIFFERENT root set, not different code.
fn stage_workspace_with_an_extension() -> TempDir {
    let dir = TempDir::new().expect("scratch dir");
    let ws = dir.path().join("ws");
    let ext = dir.path().join("ext");
    for target in [&ws, &ext] {
        copy_fixture_into(target);
    }
    std::fs::write(
        ws.join("bsl-analyzer.toml"),
        format!("[source]\nroot = \".\"\nextensions = [{{ name = \"a\", path = {ext:?} }}]\n"),
    )
    .expect("write project config");
    dir
}

fn copy_fixture_into(dst: &Path) {
    let src = designer_fixture();
    for entry in walkdir::WalkDir::new(&src) {
        let entry = entry.expect("walk fixture");
        let rel = entry.path().strip_prefix(&src).expect("path under fixture root");
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).expect("mkdir");
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).expect("mkdir parent");
            }
            std::fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
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

fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// Call a tool, retrying past the "still building" envelope. The envelope is told apart by
/// its `status: "loading"` field — how a consumer is meant to read it — never by matching
/// the human sentence beside it.
async fn poll(client: &Client, tool: &'static str, call_args: Map<String, Value>) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    loop {
        let call = CallToolRequestParams::new(tool).with_arguments(call_args.clone());
        let result = client.call_tool(call).await.expect("transport ok");
        if let Some(structured) = result.structured_content {
            if structured["status"] != "loading" {
                return structured;
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{tool} never became ready for {call_args:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A pair handed out by one tool must address the same file when handed back to another.
/// This is the whole point of publishing `(root_id, path)` rather than a rendered string:
/// an address a consumer cannot use is decoration.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_published_pair_addresses_the_same_file_when_fed_back() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let card = poll(
        &client,
        "symbol_info",
        args(&[("symbol", Value::from("ПервыйОбщийМодуль.НеУстаревшаяПроцедура"))]),
    )
    .await;

    let definitions = card["definitions"].as_array().expect("definitions is a list");
    let location = definitions
        .first()
        .and_then(|d| d.get("location"))
        .unwrap_or_else(|| panic!("the method card carries a location: {card}"));
    assert_eq!(location["position_encoding"], "utf-16");
    assert_eq!(location["schema_version"], "1");
    let root_id = location["root_id"].as_str().expect("root_id is a string").to_owned();
    let path = location["path"].as_str().expect("path is a string").to_owned();

    // Fed back to `diagnostics file`, the pair resolves — the file is served, not refused.
    let diagnostics = poll(
        &client,
        "diagnostics",
        args(&[
            ("action", Value::from("file")),
            ("root_id", Value::from(root_id.clone())),
            ("path", Value::from(path.clone())),
        ]),
    )
    .await;
    let result = &diagnostics["result"];
    assert!(
        result.get("error").is_none(),
        "the pair from symbol_info must address a file diagnostics can serve, got {result}",
    );
    assert!(result["result_id"].as_str().unwrap().contains("ПервыйОбщийМодуль"), "{result}");

    // Positive control: the same relative path under a root that is not registered here is
    // refused by name — proving the acceptance above is about THIS pair, not about the tool
    // accepting anything at all.
    let foreign = poll(
        &client,
        "diagnostics",
        args(&[
            ("action", Value::from("file")),
            ("root_id", Value::from("no-such-root")),
            ("path", Value::from(path)),
        ]),
    )
    .await;
    assert_eq!(
        foreign["result"]["error"], "unknown_root",
        "an unregistered root is an honest refusal, not another file: {foreign}",
    );
}

/// The resident and the graph answer about one workspace, so the topology they name must
/// be the same value. A contract where two subsystems publish different fingerprints for
/// one tree tells a consumer nothing at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_resident_and_the_graph_name_the_same_topology() {
    let ws = stage_workspace();
    let client = workspace_client(ws.path()).await;

    let card = poll(
        &client,
        "symbol_info",
        args(&[("symbol", Value::from("ПервыйОбщийМодуль.НеУстаревшаяПроцедура"))]),
    )
    .await;
    let from_resident = card["freshness"]["topology_fingerprint"]
        .as_str()
        .unwrap_or_else(|| panic!("the resident stamps a topology: {card}"))
        .to_owned();

    let overview = poll(&client, "graph", args(&[("action", Value::from("overview"))])).await;
    let from_graph = overview["freshness"]["topology_fingerprint"]
        .as_str()
        .unwrap_or_else(|| panic!("the graph stamps a topology: {overview}"))
        .to_owned();

    assert_eq!(
        from_resident, from_graph,
        "one workspace, one topology — resident said {from_resident}, graph said {from_graph}",
    );
    // Sixteen hex digits, not a JSON number: a u64 loses precision above 2^53 in a JS
    // consumer, and a silently rounded fingerprint compares equal when it should not.
    assert_eq!(from_resident.len(), 16, "{from_resident}");
    assert!(from_resident.chars().all(|c| c.is_ascii_hexdigit()), "{from_resident}");
    assert_ne!(
        from_resident, "0000000000000000",
        "an all-zero fingerprint would make the equality above hold for any two subsystems",
    );

    // Sensitivity control: a workspace that declares an extension has a DIFFERENT topology.
    // Without this, the equality above would also hold for a constant, and the whole field
    // would be decoration.
    let with_extension = stage_workspace_with_an_extension();
    let other = workspace_client(with_extension.path().join("ws").as_path()).await;
    let other_card = poll(
        &other,
        "symbol_info",
        args(&[("symbol", Value::from("ПервыйОбщийМодуль.НеУстаревшаяПроцедура"))]),
    )
    .await;
    let with_ext_fingerprint = other_card["freshness"]["topology_fingerprint"]
        .as_str()
        .unwrap_or_else(|| panic!("the resident stamps a topology: {other_card}"));
    assert_ne!(
        with_ext_fingerprint, from_resident,
        "declaring an extension changes the root set, so it must change the fingerprint",
    );

    // Both envelopes name who answered, so a consumer never has to guess which subsystem's
    // freshness it is holding.
    assert_eq!(card["freshness"]["source"], "resident");
    assert_eq!(overview["freshness"]["source"], "graph");
}
