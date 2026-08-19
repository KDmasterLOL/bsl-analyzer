//! End-to-end `references` over the real MCP transport.
//!
//! What the `ide` suite cannot cover is here: that the tool is served only when a launch
//! names it, that every place it hands out is a pair another tool accepts, that the
//! envelope names whoever composed the body, and that narrowing and the caps behave the
//! way the answer says they do. Each check names the input on which it must fail.

use std::path::Path;
use std::time::Duration;

use mcp_server::{serve_stream, McpProfile, McpServer, SharedState, ToolGate};
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Map, Value};
use tempfile::TempDir;

type Client = RunningService<RoleClient, ()>;

const TOOL: &str = "references";

/// The declaration under test. It is a method of a module the fixture's configuration
/// actually declares: a module the metadata does not list is invisible to a caller, so a
/// call to it resolves to something else and no reference walk would find it — which is
/// the visibility rule working, not a defect of this tool.
///
/// Its own module already holds a qualified call and a bare one; the stand adds three
/// callers, so the answer spans four files and a per-file histogram has something to be
/// wrong about.
const STAND_METHOD: &str = "ПервыйОбщийМодуль.НеУстаревшаяФункция";

/// Copy the checked-in metadata fixture into a scratch dir and add the stand's own modules,
/// so derived caches never land in the repo tree and each run starts cold.
fn stage_workspace() -> TempDir {
    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"));
    let dst = TempDir::new().expect("scratch workspace");
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.expect("walk fixture");
        let rel = entry.path().strip_prefix(src).expect("path under fixture root");
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

    for caller in ["Первый", "Второй", "Третий"] {
        // Distinct method names on purpose: one of them has to be a name NO other module
        // declares, or the "short unique name" input below would be ambiguous and would
        // stop testing what it is there for.
        write_module(
            dst.path(),
            caller,
            &format!(
                "Процедура Вызвать{caller}() Экспорт\n    \
                 ПервыйОбщийМодуль.НеУстаревшаяФункция();\nКонецПроцедуры\n"
            ),
        );
    }
    dst
}

fn write_module(root: &Path, name: &str, body: &str) {
    let dir = root.join("CommonModules").join(name).join("Ext");
    std::fs::create_dir_all(&dir).expect("mkdir module");
    std::fs::write(dir.join("Module.bsl"), body).expect("write module");
}

/// A workspace server with the opt-in tool enabled, as `--enable-tool references` does.
async fn client_with_references(root: &Path) -> Client {
    let state = SharedState::workspace(root.to_path_buf()).expect("valid workspace project");
    let gate = ToolGate::for_launch(McpProfile::Workspace, &[TOOL.to_owned()]);
    let server = McpServer::with_gate(McpProfile::Workspace, state, &gate);
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

async fn references(client: &Client, pairs: &[(&str, Value)]) -> Value {
    poll(client, TOOL, args(pairs)).await
}

fn kinds_of(answer: &Value) -> Vec<String> {
    answer["references"]
        .as_array()
        .expect("a resolved answer carries the list")
        .iter()
        .map(|entry| entry["kind"].as_str().expect("a kind").to_owned())
        .collect()
}

/// The whole surface in one pass over a stand with three call sites: the outcome, the
/// kinds, the per-file histogram that replaces a cursor, and the envelope's identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_qualified_name_is_answered_with_kinds_and_a_histogram() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let answer = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;

    assert_eq!(answer["outcome"], "resolved", "{answer}");
    assert_eq!(answer["schema_version"], "1");
    assert_eq!(answer["total"], 6, "declaration + five calls: {answer}");
    assert_eq!(answer["total_is_lower_bound"], false);
    assert_eq!(answer["narrowing_comparable"], true);

    let mut kinds = kinds_of(&answer);
    kinds.sort();
    assert_eq!(kinds, ["call", "call", "call", "call", "call", "declaration"], "{answer}");

    // The histogram is what a caller walks when `limit` hides part of the list, so its
    // counts must add up to the total — over more than one file, or any summing mistake
    // would still add up.
    let buckets = answer["files"].as_array().expect("a per-file histogram");
    assert!(buckets.len() >= 4, "one bucket per file with a hit: {buckets:?}");
    let sum: u64 = buckets.iter().map(|b| b["count"].as_u64().expect("a count")).sum();
    assert_eq!(sum, answer["total"].as_u64().unwrap(), "the histogram must cover the total");
    assert_eq!(answer["histogram_truncated"], false);

    // The body is the resident's own walk, so it carries the resident's identity.
    let freshness = &answer["freshness"];
    assert_eq!(freshness["source"], "resident");
    assert!(!freshness["revision"].is_null(), "{freshness}");
    assert!(!freshness["topology_fingerprint"].is_null(), "{freshness}");
    assert_eq!(freshness["completeness"]["status"], "complete", "{freshness}");

    client.cancel().await.ok();
}

/// Gate L — every pair this tool hands out is one another tool accepts. An address a
/// consumer cannot use is decoration.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_published_pair_addresses_a_file_diagnostics_serves() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let answer = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    let entries = answer["references"].as_array().expect("the list").clone();
    assert!(!entries.is_empty(), "an empty list would make this check vacuous");

    for entry in entries {
        let location = entry.get("location").unwrap_or_else(|| panic!("a place: {entry}"));
        assert_eq!(location["position_encoding"], "utf-16");
        assert_eq!(location["schema_version"], "1");
        let diagnostics = poll(
            &client,
            "diagnostics",
            args(&[
                ("action", Value::from("file")),
                ("root_id", location["root_id"].clone()),
                ("path", location["path"].clone()),
            ]),
        )
        .await;
        assert!(
            diagnostics["result"].get("error").is_none(),
            "the pair {location} was refused: {}",
            diagnostics["result"],
        );
    }

    client.cancel().await.ok();
}

/// Gate F — the envelope names whoever COMPOSED the body, not whoever was asked along the
/// way. The pair is the point: one input whose body the resident walked, one whose body is
/// dictionary candidates. A single input would pass against an implementation that stamps
/// the same source on everything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn freshness_names_whoever_composed_the_body() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    // Composed by the resident, though the anchor came from the dictionary: a short unique
    // name, answered with the resident's own reference walk.
    let short = references(&client, &[("symbol", Value::from("ВызватьПервый"))]).await;
    assert_eq!(short["outcome"], "resolved", "{short}");
    assert_eq!(short["freshness"]["source"], "resident", "{short}");
    assert!(!short["freshness"]["topology_fingerprint"].is_null(), "{short}");

    // Composed by the dictionary: nothing anchored, and the body IS its candidate list.
    let missing = references(&client, &[("symbol", Value::from("СовершенноНеизвестноеИмя"))]).await;
    assert_eq!(missing["outcome"], "not_found", "{missing}");
    assert_eq!(missing["freshness"]["source"], "name-dictionary", "{missing}");
    assert!(missing["freshness"]["revision"].is_null(), "{missing}");
    assert!(missing["freshness"]["topology_fingerprint"].is_null(), "{missing}");

    client.cancel().await.ok();
}

/// Gate O — a name that resolves to something with no reference walk is `unsupported_symbol`,
/// which is not `not_found` and not an empty list. The third input is the control: an
/// implementation answering `unsupported_symbol` for everything would pass the first two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_symbol_without_a_reference_walk_says_so() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let object = references(&client, &[("symbol", Value::from("Справочник.Справочник1"))]).await;
    assert_eq!(object["outcome"], "unsupported_symbol", "{object}");
    assert!(object.get("references").is_none(), "no list at all, not an empty one: {object}");
    assert!(!object["unsupported"]["category"].as_str().unwrap().is_empty(), "{object}");

    let module = references(&client, &[("symbol", Value::from("ПервыйОбщийМодуль"))]).await;
    assert_eq!(module["outcome"], "unsupported_symbol", "a module as a whole: {module}");

    // The control: an exported method nobody calls is `resolved` with an empty list —
    // a proven zero, and the answer says so with a different word.
    let unused = references(
        &client,
        &[
            ("symbol", Value::from(STAND_METHOD)),
            ("include_declaration", Value::from(false)),
            ("kinds", Value::from(vec!["write"])),
        ],
    )
    .await;
    assert_eq!(unused["outcome"], "resolved", "{unused}");
    assert_eq!(unused["total"], 0, "nobody writes to a procedure: {unused}");
    assert_eq!(unused["references"].as_array().map(Vec::len), Some(0), "{unused}");

    client.cancel().await.ok();
}

/// Gate N — narrowing by area is a subset, and it selects by file identity rather than by
/// comparing path strings. The control is the wide answer: a filter that matched nothing
/// would leave both at zero and pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_area_narrows_the_answer_and_says_what_it_selected() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let wide = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    let wide_total = wide["total"].as_u64().expect("a total");

    let narrowed = references(
        &client,
        &[
            ("symbol", Value::from(STAND_METHOD)),
            ("area_path_prefix", Value::from("CommonModules/Первый")),
        ],
    )
    .await;

    assert_eq!(narrowed["outcome"], "resolved", "{narrowed}");
    // Exactly one, and the input is chosen so a plain `starts_with` would answer two:
    // `CommonModules/ПервыйОбщийМодуль` also begins with this prefix, and only a
    // segment-wise comparison keeps it out.
    assert_eq!(narrowed["total"], 1, "only the one caller: {narrowed}");
    assert!(narrowed["total"].as_u64().unwrap() < wide_total, "the filter must actually cut");
    // A prefix that names nothing would be indistinguishable from "no references" without
    // this count.
    assert!(narrowed["area"]["files_in_area"].as_u64().unwrap() >= 1, "{narrowed}");
    for entry in narrowed["references"].as_array().expect("the list") {
        let path = entry["location"]["path"].as_str().expect("a path");
        assert!(path.starts_with("CommonModules/Первый"), "{path} escaped the area");
    }

    client.cancel().await.ok();
}

/// Gate T — a display cap is declared as a reason, and the reason is not stuck on. The
/// control is the same query with room to spare.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_display_cap_is_declared_and_the_total_is_not_capped() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let capped =
        references(&client, &[("symbol", Value::from(STAND_METHOD)), ("limit", Value::from(1))])
            .await;
    assert_eq!(capped["references"].as_array().map(Vec::len), Some(1), "{capped}");
    assert_eq!(capped["total"], 6, "`total` is counted before `limit`: {capped}");
    let reasons = capped["freshness"]["completeness"]["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason["code"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(reasons.contains(&"result_cap".to_owned()), "{capped}");
    // The histogram still covers everything the limit hid — that is what makes it a
    // replacement for a cursor.
    let sum: u64 = capped["files"]
        .as_array()
        .expect("a histogram")
        .iter()
        .map(|b| b["count"].as_u64().unwrap())
        .sum();
    assert_eq!(sum, 6, "{capped}");

    let roomy =
        references(&client, &[("symbol", Value::from(STAND_METHOD)), ("limit", Value::from(50))])
            .await;
    assert_eq!(roomy["freshness"]["completeness"]["status"], "complete", "{roomy}");

    client.cancel().await.ok();
}

/// An unregistered root is refused. The control is the call just above it: the
/// configuration's own id is accepted, so the refusal is about the unknown root and not
/// about the parameter existing at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unknown_root_is_refused_rather_than_silently_ignored() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    // Reach a ready resident first: while it builds, every call is answered with the retry
    // envelope, and a refusal that never happened would look like one that did.
    let served = references(
        &client,
        &[("symbol", Value::from(STAND_METHOD)), ("area_root_id", Value::from(""))],
    )
    .await;
    assert_eq!(served["outcome"], "resolved", "{served}");

    let call = CallToolRequestParams::new(TOOL).with_arguments(args(&[
        ("symbol", Value::from(STAND_METHOD)),
        ("area_root_id", Value::from("no-such-root")),
    ]));
    let result = client.call_tool(call).await;
    assert!(
        result.is_err(),
        "an unregistered root must be refused, not answered with an empty list: {result:?}",
    );

    client.cancel().await.ok();
}

/// A short name two modules declare is `ambiguous`, and the answer carries the qualified
/// names that end the ambiguity. The control is feeding one of them back: it resolves, so
/// `ambiguous` is a step in a workflow and not a dead end.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ambiguous_short_name_offers_the_names_that_resolve_it() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let ambiguous = references(&client, &[("symbol", Value::from("НеУстаревшаяФункция"))]).await;

    assert_eq!(ambiguous["outcome"], "ambiguous", "{ambiguous}");
    assert!(ambiguous.get("references").is_none(), "an ambiguous anchor counts nothing");
    // The body is the dictionary's candidate list, and it says so rather than borrowing the
    // resident's revision.
    assert_eq!(ambiguous["freshness"]["source"], "name-dictionary", "{ambiguous}");
    // `total` at the top level counts occurrences and nothing else. The dictionary's own
    // count lives under its own key, so a consumer reading `total` after branching on
    // `outcome` cannot pick up a number that means something different.
    assert!(ambiguous.get("total").is_none(), "top-level total is occurrences-only: {ambiguous}");
    assert!(ambiguous["lookup"]["total"].as_u64().unwrap() >= 2, "{ambiguous}");

    let candidates = ambiguous["lookup"]["candidates"].as_array().expect("candidates");
    assert!(candidates.len() >= 2, "two modules declare it: {candidates:?}");
    let qualified = candidates
        .iter()
        .filter_map(|c| c["address"]["symbol"].as_str())
        .find(|symbol| symbol.contains('.'))
        .expect("a candidate carries a qualified name")
        .to_owned();

    let resolved = references(&client, &[("symbol", Value::from(qualified.clone()))]).await;
    assert_eq!(resolved["outcome"], "resolved", "{qualified} did not resolve: {resolved}");

    client.cancel().await.ok();
}

/// Gate H — the histogram is what replaces a cursor, so its own truncation is named apart
/// from the body's: a shortened histogram loses whole files, and a caller walking it would
/// never learn of them from the reference list. The control is the same query with the
/// default budget, where the sum covers the total exactly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_truncated_histogram_is_named_apart_from_a_truncated_list() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let squeezed = references(
        &client,
        &[("symbol", Value::from(STAND_METHOD)), ("max_output_tokens", Value::from(40))],
    )
    .await;

    assert_eq!(squeezed["outcome"], "resolved", "{squeezed}");
    assert_eq!(squeezed["total"], 6, "the count survives the budget: {squeezed}");
    assert_eq!(squeezed["histogram_truncated"], true, "{squeezed}");
    let sum: u64 = squeezed["files"]
        .as_array()
        .expect("a histogram")
        .iter()
        .map(|bucket| bucket["count"].as_u64().unwrap())
        .sum();
    assert!(sum < 6, "a cut histogram must not still add up to the total: {squeezed}");
    let reasons = reason_codes(&squeezed);
    assert!(reasons.contains(&"output_budget".to_owned()), "{squeezed}");

    let whole = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    assert_eq!(whole["histogram_truncated"], false, "control: {whole}");
    let whole_sum: u64 = whole["files"]
        .as_array()
        .expect("a histogram")
        .iter()
        .map(|bucket| bucket["count"].as_u64().unwrap())
        .sum();
    assert_eq!(whole_sum, 6, "control: an untruncated histogram covers the total: {whole}");

    client.cancel().await.ok();
}

/// Gate M — a walk stopped by `max_files` says so twice: `total` becomes a lower bound and
/// the answer declares itself incomparable with a narrower one. The control is the same
/// query with room to finish, where both flags clear.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_truncated_walk_declares_that_its_total_is_a_floor() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let capped = references(
        &client,
        &[("symbol", Value::from(STAND_METHOD)), ("max_files", Value::from(1))],
    )
    .await;

    assert_eq!(capped["outcome"], "resolved", "{capped}");
    assert_eq!(capped["files_scanned"], 1, "{capped}");
    assert_eq!(capped["total_is_lower_bound"], true, "{capped}");
    assert_eq!(capped["narrowing_comparable"], false, "{capped}");
    assert!(capped["total"].as_u64().unwrap() < 6, "a partial walk cannot see everything");
    assert!(reason_codes(&capped).contains(&"result_cap".to_owned()), "{capped}");

    let full = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    assert_eq!(full["total_is_lower_bound"], false, "control: {full}");
    assert_eq!(full["narrowing_comparable"], true, "control: {full}");
    assert_eq!(full["total"], 6, "control: {full}");

    client.cancel().await.ok();
}

fn reason_codes(answer: &Value) -> Vec<String> {
    answer["freshness"]["completeness"]["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason["code"].as_str().expect("a code").to_owned())
        .collect()
}

/// Gate D — narrowing survives a root whose DECLARED spelling differs from the one the
/// files are indexed under. A filter built by joining the declared root onto a prefix and
/// comparing strings matches nothing here, and answers `resolved` with an empty list: the
/// silent zero this whole tool exists to abolish, returned through its own filter.
///
/// The control is the second half: the same stand opened by its real path, where the two
/// spellings coincide and any implementation passes.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn narrowing_survives_a_root_declared_through_a_symlink() {
    let ws = stage_workspace();
    let link_home = TempDir::new().expect("scratch dir for the link");
    let link = link_home.path().join("проект");
    std::os::unix::fs::symlink(ws.path(), &link).expect("symlink the workspace");

    let through_link = client_with_references(&link).await;
    let narrowed = references(
        &through_link,
        &[
            ("symbol", Value::from(STAND_METHOD)),
            ("area_root_id", Value::from("")),
            ("area_path_prefix", Value::from("CommonModules")),
        ],
    )
    .await;
    assert_eq!(narrowed["outcome"], "resolved", "{narrowed}");
    assert_eq!(
        narrowed["total"], 6,
        "every hit lives under CommonModules, so the filter must keep them all: {narrowed}",
    );
    assert!(narrowed["area"]["files_in_area"].as_u64().unwrap() > 1, "{narrowed}");
    through_link.cancel().await.ok();

    let direct = client_with_references(ws.path()).await;
    let control = references(
        &direct,
        &[
            ("symbol", Value::from(STAND_METHOD)),
            ("area_root_id", Value::from("")),
            ("area_path_prefix", Value::from("CommonModules")),
        ],
    )
    .await;
    assert_eq!(control["total"], narrowed["total"], "the link must not change the answer");
    direct.cancel().await.ok();
}

/// Two declarations in ONE root cannot be told apart by a root filter, and the answer must
/// not send an agent down that road. The control is the two-root case, where the root
/// filter is exactly the right advice.
///
/// Staging it needs a case-sensitive filesystem: the two spellings are the same directory on
/// APFS or NTFS, and the stand would quietly become a single module answering `resolved` —
/// a gate that cannot fail. Declared with `cfg(unix)` rather than left to degrade, and the
/// stand asserts its own precondition before it asserts anything about the hint.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_resolution_hint_names_an_axis_that_can_separate_these_declarations() {
    let ws = stage_workspace();
    // One root, two directories whose names differ only in case: a module path is folded to
    // its key, so both files answer to `Стенд.ОбщийМетод`, and no root filter stands between
    // them. This is the spelling divergence a case-sensitive filesystem makes real, not an
    // invented stand.
    for spelling in ["Стенд", "СТЕНД"] {
        write_module(ws.path(), spelling, "Процедура ОбщийМетод() Экспорт\nКонецПроцедуры\n");
    }
    let staged = std::fs::read_dir(ws.path().join("CommonModules"))
        .expect("the stand's modules")
        .filter_map(Result::ok)
        // Cyrillic, so `eq_ignore_ascii_case` would fold nothing and count zero.
        .filter(|entry| entry.file_name().to_string_lossy().to_lowercase() == "стенд")
        .count();
    assert_eq!(
        staged, 2,
        "this filesystem folded the two spellings into one directory, so the ambiguity this \
         gate is about was never staged",
    );

    let client = client_with_references(ws.path()).await;
    let answer = references(&client, &[("symbol", Value::from("Стенд.ОбщийМетод"))]).await;

    assert_eq!(answer["outcome"], "ambiguous", "{answer}");
    let declarations = answer["declarations"].as_array().expect("declarations");
    assert_eq!(declarations.len(), 2, "{answer}");
    let roots: Vec<&str> =
        declarations.iter().filter_map(|d| d["location"]["root_id"].as_str()).collect();
    assert_eq!(roots, ["", ""], "the stand must put both declarations in one root: {answer}");
    let hint = answer["resolution_hint"].as_str().expect("a hint");
    // A published machine field, so its text is checked as a value: an alignment run left
    // inside the literal travels to the agent and into the tool's own documentation.
    assert!(
        !hint.contains("  "),
        "a run of spaces from a wrapped literal leaked into the hint: {hint:?}",
    );
    assert!(
        !hint.contains("anchor_root_id"),
        "a root filter cannot separate declarations sharing a root: {hint}",
    );
    assert!(hint.contains("path"), "the hint must name an axis that works: {hint}");

    client.cancel().await.ok();
}

/// Gate F, the pair the first edition of it lacked: an `unsupported_symbol` composed by the
/// dictionary carries the dictionary's own answer, and one composed by the resident does
/// not pretend to. An envelope that names `name-dictionary` and shows none of what it could
/// consult is an identity without its evidence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unsupported_answer_carries_the_sources_that_composed_it() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    // Composed by the dictionary: a short exact name of a platform member.
    let platform = references(&client, &[("symbol", Value::from("Добавить"))]).await;
    assert_eq!(platform["outcome"], "unsupported_symbol", "{platform}");
    assert_eq!(platform["freshness"]["source"], "name-dictionary", "{platform}");
    let providers = platform["lookup"]["providers"].as_array().unwrap_or_else(|| {
        panic!("a name-dictionary envelope must show its providers: {platform}")
    });
    assert!(!providers.is_empty(), "{platform}");

    // Composed by the resident: the category came from the resident's own resolution, and
    // there is no dictionary answer to show.
    let object = references(&client, &[("symbol", Value::from("Справочник.Справочник1"))]).await;
    assert_eq!(object["outcome"], "unsupported_symbol", "{object}");
    assert_eq!(object["freshness"]["source"], "resident", "{object}");
    assert!(object.get("lookup").is_none(), "nothing was asked of the dictionary: {object}");

    client.cancel().await.ok();
}

/// `histogram_truncated` says whole files are missing from the histogram. A single bucket
/// that is itself larger than what the budget left is not that: nothing was lost, and the
/// counts still add up. The control is the multi-file stand where buckets really are
/// dropped — there the flag must fire.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kept_oversized_bucket_is_not_a_truncated_histogram() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let one_file = references(
        &client,
        &[("symbol", Value::from("Первый.ВызватьПервый")), ("max_output_tokens", Value::from(1))],
    )
    .await;

    assert_eq!(one_file["outcome"], "resolved", "{one_file}");
    let buckets = one_file["files"].as_array().expect("a histogram");
    assert_eq!(buckets.len(), 1, "the stand must have exactly one file with hits: {one_file}");
    let sum: u64 = buckets.iter().map(|b| b["count"].as_u64().unwrap()).sum();
    assert_eq!(sum, one_file["total"].as_u64().unwrap(), "nothing was dropped: {one_file}");
    assert_eq!(
        one_file["histogram_truncated"], false,
        "no file is missing from this histogram: {one_file}",
    );
    // The overflow itself is still declared — as a budget reason, which is what it is.
    assert!(reason_codes(&one_file).contains(&"output_budget".to_owned()), "{one_file}");

    client.cancel().await.ok();
}

/// The output budget is a promise of the parameter, not a property of one branch: an answer
/// that resolved nothing still has to fit it, and to say when it did not. The control is the
/// same query with room to spare.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_budget_applies_to_an_answer_that_resolved_nothing() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let squeezed = references(
        &client,
        &[("symbol", Value::from("НеУстаревшаяФункция")), ("max_output_tokens", Value::from(20))],
    )
    .await;
    assert_eq!(squeezed["outcome"], "ambiguous", "{squeezed}");
    assert!(
        reason_codes(&squeezed).contains(&"output_budget".to_owned()),
        "an over-budget candidate list must say so: {squeezed}",
    );

    let roomy = references(&client, &[("symbol", Value::from("НеУстаревшаяФункция"))]).await;
    assert_eq!(roomy["outcome"], "ambiguous", "{roomy}");
    assert!(
        !reason_codes(&roomy).contains(&"output_budget".to_owned()),
        "control: with room, no budget reason: {roomy}",
    );

    client.cancel().await.ok();
}

/// A parameter that is validated and then ignored is worse than one that is refused: the
/// caller learns its root is unknown but never that its narrowing did nothing. The control
/// is the same positional call without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_anchor_root_with_a_positional_anchor_is_refused() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let path = "CommonModules/Первый/Ext/Module.bsl";
    let control = references(
        &client,
        &[
            ("root_id", Value::from("")),
            ("path", Value::from(path)),
            ("line", Value::from(0)),
            // Column 10: the declared name, past the `Процедура` keyword.
            ("column", Value::from(10)),
        ],
    )
    .await;
    assert_eq!(control["outcome"], "resolved", "control: the positional anchor works: {control}");

    let call = CallToolRequestParams::new(TOOL).with_arguments(args(&[
        ("root_id", Value::from("")),
        ("path", Value::from(path)),
        ("line", Value::from(0)),
        ("column", Value::from(10)),
        ("anchor_root_id", Value::from("")),
    ]));
    let refused = client.call_tool(call).await;
    assert!(
        refused.is_err(),
        "a position already names one file; a declaration root cannot also apply: {refused:?}",
    );

    client.cancel().await.ok();
}

/// A candidate list the budget shortened must say so where the promise lives. `truncated`
/// and `total` under `lookup` are the dictionary's own contract — "this list is complete
/// against that count" — and trimming the list from outside without touching the flag
/// leaves the two numbers to betray it. The control is the same query with room, where the
/// list is whole and the flag is false.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_budget_trimmed_candidate_list_declares_itself_incomplete() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let squeezed = references(
        &client,
        &[("symbol", Value::from("НеУстаревшаяФункция")), ("max_output_tokens", Value::from(20))],
    )
    .await;
    assert_eq!(squeezed["outcome"], "ambiguous", "{squeezed}");
    let shown = squeezed["lookup"]["candidates"].as_array().expect("candidates").len() as u64;
    let total = squeezed["lookup"]["total"].as_u64().expect("a dictionary total");
    assert!(
        shown < total,
        "the input must actually lose a candidate, or this gate passes on any code: {squeezed}",
    );
    assert_eq!(
        squeezed["lookup"]["truncated"], true,
        "a list shorter than its own total is truncated, whoever shortened it: {squeezed}",
    );

    let roomy = references(&client, &[("symbol", Value::from("НеУстаревшаяФункция"))]).await;
    let whole = roomy["lookup"]["candidates"].as_array().expect("candidates").len() as u64;
    assert_eq!(whole, roomy["lookup"]["total"].as_u64().unwrap(), "control: {roomy}");
    assert_eq!(roomy["lookup"]["truncated"], false, "control: nothing was cut: {roomy}");

    client.cancel().await.ok();
}

/// An ambiguity the name dictionary composed has an axis of its own — the qualified name of
/// a candidate, fed back as `symbol` — and it is not a root: the candidates are spread over
/// kinds, not over roots. Without the field the agent falls back to the tool description,
/// which cannot know which ambiguity it got. The control is the declaration-based ambiguity
/// next door, whose hint names a different axis for the same field.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dictionary_ambiguity_carries_its_own_resolution_hint() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let ambiguous = references(&client, &[("symbol", Value::from("НеУстаревшаяФункция"))]).await;
    assert_eq!(ambiguous["outcome"], "ambiguous", "{ambiguous}");
    assert_eq!(ambiguous["freshness"]["source"], "name-dictionary", "{ambiguous}");
    let hint = ambiguous["resolution_hint"].as_str().expect("a hint: {ambiguous}");
    assert!(
        hint.contains("symbol"),
        "the axis out of a dictionary ambiguity is a qualified name: {hint}",
    );
    assert!(
        !hint.contains("anchor_root_id"),
        "these candidates are not separated by a root: {hint}",
    );

    client.cancel().await.ok();
}

/// `root_id` spells out which root a `path` is relative to, and beside a `symbol` it has
/// nothing to qualify: a name is resolved across every root. Accepting it there would report
/// a narrowing that never happened — and `symbol_info` refuses the same pair for the same
/// reason. The controls are both halves apart: the symbol alone, and the root beside a path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_root_beside_a_symbol_is_refused_rather_than_dropped() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    let control = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    assert_eq!(control["outcome"], "resolved", "control: the symbol alone answers: {control}");

    let call = CallToolRequestParams::new(TOOL).with_arguments(args(&[
        ("symbol", Value::from(STAND_METHOD)),
        ("root_id", Value::from("")),
    ]));
    assert!(
        client.call_tool(call).await.is_err(),
        "a root that qualifies nothing must be refused, not silently dropped",
    );

    let positional = references(
        &client,
        &[
            ("root_id", Value::from("")),
            ("path", Value::from("CommonModules/Первый/Ext/Module.bsl")),
            ("line", Value::from(0)),
            ("column", Value::from(10)),
        ],
    )
    .await;
    assert!(
        positional.get("outcome").is_some(),
        "control: the same root beside a path is what the parameter is for: {positional}",
    );

    // The same rule for the other half of a positional anchor: `line` narrows where the
    // walk starts, and beside a name there is nothing for it to narrow.
    let with_line = CallToolRequestParams::new(TOOL)
        .with_arguments(args(&[("symbol", Value::from(STAND_METHOD)), ("line", Value::from(0))]));
    assert!(
        client.call_tool(with_line).await.is_err(),
        "a line that positions nothing must be refused, not silently dropped",
    );

    client.cancel().await.ok();
}

/// The stand the root filter needs: two roots, each holding a caller of ONE declaration.
/// A single-root stand cannot tell a working root filter from one that never runs, because
/// `""` selects everything either way. The extension lives in its own scratch dir — a
/// subdirectory of the configuration would be walked by the configuration root too, and
/// then no file would be exclusive to either root.
fn stage_two_roots() -> (TempDir, TempDir) {
    let ws = stage_workspace();
    let ext = TempDir::new().expect("scratch extension");
    // A declared extension is recognised by its `Configuration.xml`, and without one the
    // topology refuses the declaration outright.
    std::fs::copy(ws.path().join("Configuration.xml"), ext.path().join("Configuration.xml"))
        .expect("the extension needs a configuration file to be one");
    write_module(
        ext.path(),
        "Расширение",
        "Процедура ВызватьИзРасширения() Экспорт\n    \
         ПервыйОбщийМодуль.НеУстаревшаяФункция();\nКонецПроцедуры\n",
    );
    let path = ext.path();
    std::fs::write(
        ws.path().join("bsl-analyzer.toml"),
        format!("[source]\nroot = \".\"\nextensions = [{{ name = \"расш\", path = {path:?} }}]\n"),
    )
    .expect("declare the extension");
    (ws, ext)
}

/// Gate N — `area_root_id` names a root and the answer holds references from that root
/// alone. Every other root check in this file passes `""`, which selects the whole
/// workspace: a `select` that validated the id against the root table and then filtered
/// nothing would pass all of them. The control is the same query against the other root,
/// whose hits are disjoint from these.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_area_root_selects_one_root_and_excludes_the_other() {
    let (ws, _ext) = stage_two_roots();
    let client = client_with_references(ws.path()).await;

    let everywhere = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    assert_eq!(everywhere["outcome"], "resolved", "{everywhere}");
    let roots: Vec<String> = everywhere["files"]
        .as_array()
        .expect("a histogram")
        .iter()
        .map(|bucket| bucket["location"]["root_id"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        roots.iter().any(String::is_empty),
        "the configuration must contribute hits: {roots:?}",
    );
    // Taken from the answer, not spelled here: a `root_id` a location publishes is the one
    // the tools accept back, and that is the pair this gate is about.
    let extension_root = roots
        .iter()
        .find(|root| !root.is_empty())
        .expect("the stand must span both roots, or the filter has nothing to exclude")
        .to_owned();

    let extension = references(
        &client,
        &[
            ("symbol", Value::from(STAND_METHOD)),
            ("area_root_id", Value::from(extension_root.clone())),
        ],
    )
    .await;
    let configuration = references(
        &client,
        &[("symbol", Value::from(STAND_METHOD)), ("area_root_id", Value::from(""))],
    )
    .await;

    let files_of = |answer: &Value| -> Vec<String> {
        answer["files"]
            .as_array()
            .expect("a histogram")
            .iter()
            .map(|bucket| {
                format!(
                    "{}:{}",
                    bucket["location"]["root_id"].as_str().unwrap_or_default(),
                    bucket["location"]["path"].as_str().unwrap_or_default(),
                )
            })
            .collect()
    };
    let from_extension = files_of(&extension);
    let from_configuration = files_of(&configuration);

    assert!(!from_extension.is_empty(), "the extension calls it: {extension}");
    assert!(!from_configuration.is_empty(), "so does the configuration: {configuration}");
    assert!(
        from_extension.iter().all(|file| file.starts_with(&format!("{extension_root}:"))),
        "a root filter that let another root through selected nothing: {from_extension:?}",
    );
    assert!(
        from_configuration.iter().all(|file| file.starts_with(':')),
        "the control must be just as exclusive: {from_configuration:?}",
    );
    assert_eq!(
        extension["total"].as_u64().unwrap() + configuration["total"].as_u64().unwrap(),
        everywhere["total"].as_u64().unwrap(),
        "the two roots partition the answer: {extension} / {configuration}",
    );

    client.cancel().await.ok();
}

/// A name declared after the resident was built must not come back `not_found`: a caller
/// reads that outcome as final. Two mechanisms can keep the promise — the change hub, and
/// the forced re-scan this tool does on a miss the way `symbol_info` does — and this gate
/// holds whichever of them ran; it does not single out the second. The control is a name
/// nothing ever declared, which stays `not_found` however many re-scans it takes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_name_declared_after_the_last_scan_is_found_on_a_forced_retry() {
    let ws = stage_workspace();
    let client = client_with_references(ws.path()).await;

    // Reach a ready resident first, so what follows is a stale scan and not a cold start.
    let ready = references(&client, &[("symbol", Value::from(STAND_METHOD))]).await;
    assert_eq!(ready["outcome"], "resolved", "{ready}");

    let module = ws.path().join("CommonModules").join("Первый").join("Ext").join("Module.bsl");
    let text = std::fs::read_to_string(&module).expect("the stand module");
    std::fs::write(
        &module,
        format!("{text}\nПроцедура ДобавленнаяПозже() Экспорт\nКонецПроцедуры\n"),
    )
    .expect("declare a method after the resident was built");

    let fresh = references(&client, &[("symbol", Value::from("Первый.ДобавленнаяПозже"))]).await;
    assert_eq!(
        fresh["outcome"], "resolved",
        "a method that exists on disk must not be reported missing: {fresh}",
    );

    let never = references(&client, &[("symbol", Value::from("Первый.НикогдаНеБыло"))]).await;
    assert_eq!(
        never["outcome"], "not_found",
        "control: a re-scan finds nothing that is not there: {never}"
    );

    client.cancel().await.ok();
}
