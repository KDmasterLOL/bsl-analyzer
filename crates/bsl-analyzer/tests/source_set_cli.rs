//! End-to-end checks that the source-set flags actually change the analysis,
//! driven through the real `analyze` binary and its `--format jsonl` output.
//!
//! Unit tests over `SourceSetArgs` only prove that argv turns into the right
//! project model. They cannot show that the model reaches the analyzer, which
//! is the whole point of the flags: an extension analyzed without its main
//! configuration reports valid calls into that configuration as unresolved.
//!
//! Every check here is an A/B differing by exactly one flag, and the run that
//! is expected to be clean is only trusted next to a run that is not — a
//! "diagnostic absent" result on its own is equally consistent with the file
//! never having been analyzed at all.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const MAIN_MODULE: &str = "БазовыйМодуль";
const DEP_MODULE: &str = "МодульЗависимости";
const EXT_MODULE: &str = "МодульРасширения";

/// A call nothing in any source set can resolve. Kept beside the call under
/// test so that "the diagnostic disappeared" can be told apart from "the
/// diagnostic stopped being produced at all in the flagged branch".
const MISSING_MODULE: &str = "ЗаведомоНетТакогоМодуля";
const MISSING_CALL: &str = "ЗаведомоНетТакогоМодуля.НетТакогоМетода();";

/// A configuration root deep enough that `Project`'s own two-level search for
/// `Configuration.xml` cannot find it, and not under `src/cf` or `Configuration`
/// either. Without this the "no main configuration" run would silently acquire
/// one by discovery, and the control it provides would be worthless.
const MAIN: &str = "a/b/main";
const EXT: &str = "a/b/ext";
const DEP: &str = "a/b/dep";

fn configuration_xml(name: &str, module: &str, extension: bool) -> String {
    let purpose = if extension {
        "<ConfigurationExtensionPurpose>Customization</ConfigurationExtensionPurpose>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<Configuration uuid="11111111-0000-0000-0000-000000000001">
		<Properties><Name>{name}</Name><Synonym/><Comment/><NamePrefix/>{purpose}<DefaultRunMode>ManagedApplication</DefaultRunMode></Properties>
		<ChildObjects><CommonModule>{module}</CommonModule></ChildObjects>
	</Configuration>
</MetaDataObject>"#
    )
}

fn common_module_xml(module: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="22222222-0000-0000-0000-000000000002">
		<Properties><Name>{module}</Name><Synonym/><Comment/><Global>false</Global><ClientManagedApplication>false</ClientManagedApplication><Server>true</Server><ExternalConnection>false</ExternalConnection><ClientOrdinaryApplication>false</ClientOrdinaryApplication><ServerCall>false</ServerCall><Privileged>false</Privileged><ReturnValuesReuse>DontUse</ReturnValuesReuse></Properties>
	</CommonModule>
</MetaDataObject>"#
    )
}

fn write_configuration(root: &Path, rel: &str, name: &str, module: &str, body: &str, ext: bool) {
    let dir = root.join(rel);
    let module_dir = dir.join("CommonModules").join(module).join("Ext");
    std::fs::create_dir_all(&module_dir).unwrap();
    std::fs::write(dir.join("Configuration.xml"), configuration_xml(name, module, ext)).unwrap();
    std::fs::write(
        dir.join("CommonModules").join(format!("{module}.xml")),
        common_module_xml(module),
    )
    .unwrap();
    std::fs::write(module_dir.join("Module.bsl"), body).unwrap();
}

/// Main configuration plus an extension whose module calls the main
/// configuration's exported common-module method.
fn workspace_calling_main_configuration(root: &Path) {
    write_configuration(
        root,
        MAIN,
        "ОсновнаяКонфигурация",
        MAIN_MODULE,
        "Функция Экспортируемая() Экспорт\n\tВозврат 1;\nКонецФункции\n",
        false,
    );
    write_configuration(
        root,
        EXT,
        "Расширение",
        EXT_MODULE,
        &format!(
            "Процедура Вызвать() Экспорт\n\t{MAIN_MODULE}.Экспортируемая();\n\t{MISSING_CALL}\nКонецПроцедуры\n"
        ),
        true,
    );
}

struct Run {
    files: Vec<Value>,
    done: Value,
    stderr: String,
}

impl Run {
    /// Matched on the module's own path tail rather than on the name appearing
    /// anywhere in the absolute path: a temp directory that happens to carry the
    /// module's name in an ancestor would otherwise pick the wrong file.
    fn file_event(&self, module: &str) -> Option<&Value> {
        let tail = format!("CommonModules/{module}/Ext/Module.bsl");
        self.files.iter().find(|e| e["path"].as_str().is_some_and(|p| p.ends_with(&tail)))
    }

    /// The module's file event, having established that it was actually
    /// analyzed. A `file` event is emitted for files whose analysis panicked or
    /// whose text could not be read, with the failure recorded in `error` and
    /// `done.failed_files` and the process still exiting zero — so the event's
    /// mere presence proves nothing.
    fn analyzed(&self, module: &str) -> &Value {
        let event = self
            .file_event(module)
            .unwrap_or_else(|| panic!("{module} was not analyzed at all; jsonl: {:?}", self.files));
        assert_eq!(event["error"], Value::Null, "{module} failed to analyze: {event}");
        assert_eq!(self.done["failed_files"], 0, "some file failed: {}", self.done);
        event
    }

    /// Messages of the given code reported for the module, in order.
    ///
    /// Compared as text rather than counted: the fixture keeps a deliberately
    /// unresolvable call beside the one under test, and a count alone cannot
    /// tell "the real call resolved" from "the two swapped places" or from the
    /// diagnostic being suppressed wholesale.
    fn messages(&self, module: &str, code: &str) -> Vec<String> {
        self.analyzed(module)["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"].as_str() == Some(code))
            .map(|d| d["message"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Which module names the given code complains about, sorted.
    fn unresolved_modules(&self, module: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .messages(module, "UnresolvedMethodCall")
            .iter()
            .filter_map(|m| m.split('\'').nth(1).map(str::to_owned))
            .collect();
        names.sort();
        names
    }
}

fn analyze(source_dir: &Path, flags: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .arg("analyze")
        .arg("-s")
        .arg(source_dir)
        .args(flags)
        .args(["--format", "jsonl"])
        .env_remove("ONEC_CONFIGURATIONS_ROOT")
        .output()
        .expect("failed to run the analyzer");
    // Checked for every run, including the ones inspected only through stderr:
    // the notice is printed before the walk, so a process that dies afterwards
    // still satisfies a bare `contains` and turns its paired run into noise.
    assert!(
        output.status.success(),
        "analyze {flags:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let events: Vec<Value> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    Run {
        files: events.iter().filter(|e| e["type"] == "file").cloned().collect(),
        done: events
            .iter()
            .find(|e| e["type"] == "done")
            .cloned()
            .unwrap_or_else(|| panic!("no done event; stdout: {stdout}")),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn workspace() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn path(root: &Path, rel: &str) -> PathBuf {
    root.join(rel)
}

#[test]
fn shared_configuration_dependency_resolves_from_project_dotenv() {
    let dir = workspace();
    let project = dir.path().join("project");
    let shared = dir.path().join("configurations");
    std::fs::create_dir_all(&project).unwrap();

    write_configuration(
        &shared,
        "UT11/11.5.22.129",
        "ОсновнаяКонфигурация",
        MAIN_MODULE,
        "Функция Экспортируемая() Экспорт\n\tВозврат 1;\nКонецФункции\n",
        false,
    );
    write_configuration(
        &project,
        "Расширения/EXT",
        "Расширение",
        EXT_MODULE,
        &format!(
            "Процедура Вызвать() Экспорт\n\t{MAIN_MODULE}.Экспортируемая();\n\t{MISSING_CALL}\nКонецПроцедуры\n"
        ),
        true,
    );
    std::fs::write(
        project.join("bsl-analyzer.toml"),
        r#"[source]
extensions = ["Расширения/EXT"]

[source.configuration]
id = "UT11"
version = "11.5.22.129"
"#,
    )
    .unwrap();
    std::fs::write(
        project.join(".env"),
        format!("ONEC_CONFIGURATIONS_ROOT={}\n", shared.display()),
    )
    .unwrap();

    let run = analyze(&project, &[]);
    assert_eq!(
        run.unresolved_modules(EXT_MODULE),
        vec![MISSING_MODULE.to_string()],
        "the shared configuration must provide the base module context"
    );
}

#[test]
fn binding_the_main_configuration_resolves_calls_into_it() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());

    // Positive control. Without a main configuration the call cannot resolve,
    // and this assertion is what makes the paired one below mean anything.
    let standalone = analyze(dir.path(), &["--extension", &format!("EXT={EXT}")]);
    assert_eq!(
        standalone.unresolved_modules(EXT_MODULE),
        vec![MAIN_MODULE.to_string(), MISSING_MODULE.to_string()],
        "alone, neither the main configuration's module nor the missing one resolves"
    );

    let bound =
        analyze(dir.path(), &["--configuration-root", MAIN, "--extension", &format!("EXT={EXT}")]);
    assert_eq!(
        bound.unresolved_modules(EXT_MODULE),
        vec![MISSING_MODULE.to_string()],
        "binding must resolve the main configuration's module and leave only the missing one"
    );
}

#[test]
fn a_declared_dependency_resolves_calls_between_extensions() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());
    write_configuration(
        dir.path(),
        DEP,
        "Зависимость",
        DEP_MODULE,
        "Функция ИзЗависимости() Экспорт\n\tВозврат 2;\nКонецФункции\n",
        true,
    );
    std::fs::write(
        path(dir.path(), EXT).join("CommonModules").join(EXT_MODULE).join("Ext/Module.bsl"),
        format!(
            "Процедура Вызвать() Экспорт\n\t{DEP_MODULE}.ИзЗависимости();\n\t{MISSING_CALL}\nКонецПроцедуры\n"
        ),
    )
    .unwrap();

    let declared: Vec<String> = vec![
        "--configuration-root".into(),
        MAIN.into(),
        "--extension".into(),
        format!("DEP={DEP}"),
        "--extension".into(),
        format!("EXT={EXT}"),
    ];
    let refs: Vec<&str> = declared.iter().map(String::as_str).collect();

    // Independent extensions do not see each other, so this is the control.
    let unrelated = analyze(dir.path(), &refs);
    assert_eq!(
        unrelated.unresolved_modules(EXT_MODULE),
        vec![MISSING_MODULE.to_string(), DEP_MODULE.to_string()],
        "without a declared edge the other extension's module must stay invisible"
    );

    let mut with_edge = refs.clone();
    with_edge.extend(["--extension-depends-on", "EXT=DEP"]);
    let dependent = analyze(dir.path(), &with_edge);
    assert_eq!(
        dependent.unresolved_modules(EXT_MODULE),
        vec![MISSING_MODULE.to_string()],
        "the edge must resolve the dependency's module and leave only the missing one"
    );
}

#[test]
fn no_extensions_drops_the_configured_list() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());
    std::fs::write(
        dir.path().join("bsl-analyzer.toml"),
        format!("[source]\nroot = \"{MAIN}\"\nextensions = [\"{EXT}\"]\n"),
    )
    .unwrap();

    // Paired control: the flag is only shown to remove the extension if the same
    // workspace analyzes it without the flag. A wiring bug that always dropped
    // configured extensions would satisfy the one-sided check.
    let configured = analyze(dir.path(), &[]);
    configured.analyzed(EXT_MODULE);

    let opted_out = analyze(dir.path(), &["--no-extensions"]);
    assert!(
        opted_out.file_event(EXT_MODULE).is_none(),
        "--no-extensions must drop the list the config declared"
    );
    // The flag drops extensions, not the analysis. Without this, a wiring bug
    // that cleared every source root would satisfy the assertion above by
    // analyzing nothing at all.
    opted_out.analyzed(MAIN_MODULE);
}

/// How many times the notice appears — the invariant is exactly one message per
/// run, and a substring check would pass just as happily on a duplicate.
fn notices(run: &Run) -> usize {
    // The run's own health is asserted here as well: the notice is printed
    // before the walk, so a per-file failure afterwards still leaves the phrase
    // in stderr while the process exits zero — and an analysis regression for
    // exactly the extension under test would slip through.
    assert_eq!(run.done["failed_files"], 0, "some file failed: {}", run.done);
    // Counted per line, because the message embeds the source path: a workspace
    // path containing the phrase would otherwise inflate the count.
    run.stderr
        .lines()
        .filter(|line| {
            line.contains("is a configuration extension analyzed without its main configuration")
        })
        .count()
}

#[test]
fn an_extension_taken_as_the_main_root_is_reported() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());
    write_configuration(
        dir.path(),
        DEP,
        "Зависимость",
        DEP_MODULE,
        "Функция ИзЗависимости() Экспорт\n\tВозврат 2;\nКонецФункции\n",
        true,
    );

    // The case that needs no flag at all, and the one an integrator actually
    // hits: point `-s` straight at an extension. The notice is tied to the
    // resolved root, not to any flag, so narrowing it to the override path
    // would leave exactly this run silent in front of its false findings.
    assert_eq!(
        notices(&analyze(&path(dir.path(), EXT), &[])),
        1,
        "an extension given directly as the source dir must be called out once"
    );
    assert_eq!(
        notices(&analyze(&path(dir.path(), MAIN), &[])),
        0,
        "a main configuration given directly must stay silent"
    );

    // Only `--configuration-root` moves between the runs of each pair. Varying
    // the extension flags at the same time would let an implementation keyed on
    // `--no-extensions`, or on the list being empty, pass without ever asking
    // what the resolved root actually is.
    for extensions in [vec!["--no-extensions"], vec!["--extension", "DEP=a/b/dep"]] {
        let with_root = |root: &str| {
            let mut flags = vec!["--configuration-root", root];
            flags.extend(extensions.iter().copied());
            notices(&analyze(dir.path(), &flags))
        };

        assert_eq!(
            with_root(EXT),
            1,
            "an extension used as the main root must be called out once (extensions: {extensions:?})"
        );
        assert_eq!(
            with_root(MAIN),
            0,
            "a real main configuration must stay silent (extensions: {extensions:?})"
        );
    }
}

/// Drives `mcp serve` over stdio: handshake, wait for the resident database to
/// be ready, then one `diagnostics file` call. Returns the modules that stayed
/// unresolved, plus the `status` body that reported readiness.
///
/// The MCP path re-derives the project from a bare workspace path in a dozen
/// places, none of which can see argv. Nothing in the `analyze` checks above
/// would notice one of them left on the old source, so this asks the question
/// again on the channel an embedding host actually uses.
///
/// Readiness is polled rather than assumed: a freshly started server answers a
/// data action with a "still building" envelope that carries no diagnostics at
/// all, which would read exactly like "everything resolved".
fn mcp_probe(workspace: &Path, module_file: &Path, flags: &[&str]) -> (Vec<String>, Value) {
    use std::io::{BufRead, BufReader, Write as _};
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["mcp", "serve", "--profile", "workspace", "--mode", "stdio", "-s"])
        .arg(workspace)
        .args(flags)
        // `--mode stdio` is also the *unset* value, and the workspace profile
        // resolves that to the broker — which detaches a daemon that outlives
        // the test and writes a cache into the temp workspace.
        .env("BSL_MCP_BROKER", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start the MCP server");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |value: Value| writeln!(stdin, "{value}").unwrap();
    let mut recv = |id: i64| -> Value {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "the server closed stdout");
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                if message["id"] == id {
                    return message;
                }
            }
        }
    };

    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "t", "version": "1"}}
    }));
    recv(1);
    send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let call = |id: i64, arguments: Value| {
        serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "diagnostics", "arguments": arguments}
        })
    };

    let mut status = Value::Null;
    for id in 10..200 {
        send(call(id, serde_json::json!({"action": "status"})));
        status = recv(id)["result"]["structuredContent"].clone();
        if status["state"] == "ready" || status["state"] == "failed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(status["state"], "ready", "the resident database never became ready: {status}");

    let reply = {
        let arguments =
            serde_json::json!({"action": "file", "path": module_file.display().to_string()});
        send(call(2, arguments));
        recv(2)
    };
    let body = reply.to_string();
    assert!(!body.contains("\"isError\":true"), "the diagnostics call failed: {body}");

    drop(stdin);
    let _ = child.wait();

    let mut names: Vec<String> = body
        .match_indices("разрешить получателя вызова '")
        .map(|(at, needle)| {
            let rest = &body[at + needle.len()..];
            rest[..rest.find('\'').unwrap_or(0)].to_owned()
        })
        .collect();
    names.sort();
    names.dedup();
    (names, status)
}

/// The graph's own view of the source set: node and edge counts once its build
/// settles.
///
/// Graph passes re-derive the project from a bare workspace path, separately
/// from the resident diagnostics host. A regression that left that path on the
/// on-disk config would keep every diagnostics check green while the graph was
/// built over a different set of roots.
fn mcp_graph_overview(workspace: &Path, flags: &[&str]) -> Value {
    use std::io::{BufRead, BufReader, Write as _};
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .args(["mcp", "serve", "--profile", "workspace", "--mode", "stdio", "-s"])
        .arg(workspace)
        .args(flags)
        .env("BSL_MCP_BROKER", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start the MCP server");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |value: Value| writeln!(stdin, "{value}").unwrap();
    let mut recv = |id: i64| -> Value {
        loop {
            let mut line = String::new();
            assert_ne!(stdout.read_line(&mut line).unwrap(), 0, "the server closed stdout");
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                if message["id"] == id {
                    return message;
                }
            }
        }
    };

    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "t", "version": "1"}}
    }));
    recv(1);
    send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));

    let call = |id: i64, action: &str| {
        serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "graph", "arguments": {"action": action}}
        })
    };

    let mut status = Value::Null;
    for id in 10..300 {
        send(call(id, "status"));
        status = recv(id)["result"]["structuredContent"].clone();
        if status["state"] == "ready" || status["state"] == "failed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // `failed` is a real outcome here, not a flake: the builder reports it when
    // it panicked, and an overview read past it would compare empty to empty.
    assert_eq!(status["state"], "ready", "the graph never became ready: {status}");

    send(call(2, "overview"));
    let overview = recv(2)["result"]["structuredContent"]["result"].clone();

    drop(stdin);
    let _ = child.wait();
    overview
}

#[test]
fn the_source_set_reaches_the_graph() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());

    // Both runs bind the main configuration, so the only thing moving is whether
    // the extension is part of the set — which is exactly what the graph passes
    // re-derive for themselves.
    let base_only =
        mcp_graph_overview(dir.path(), &["--configuration-root", MAIN, "--no-extensions"]);
    assert_eq!(base_only["nodes"], 1, "only the main configuration's method: {base_only}");
    assert_eq!(base_only["edges"], 0, "nothing calls it: {base_only}");

    let with_extension = mcp_graph_overview(
        dir.path(),
        &["--configuration-root", MAIN, "--extension", "EXT=a/b/ext"],
    );
    assert_eq!(with_extension["nodes"], 2, "both methods: {with_extension}");
    assert_eq!(
        with_extension["edge_provenance"]["resolved"], 1,
        "the extension's call into the main configuration must resolve into an edge: \
         {with_extension}"
    );
}

/// Two configuration directories deep enough that discovery finds neither, and
/// no flags binding them: the workspace itself becomes the only declared root,
/// while each module still attributes to the nested directory that holds its
/// metadata. The build's pre-pool warm-up covers declared roots, so a second
/// attributed root reaches the whole-config loader lazily — from inside the
/// worker pool, where its fan-out may deadlock the build.
///
/// One such configuration is not enough to show this: the single module doubles
/// as the batch representative the warm-up already touches.
#[test]
fn nested_configurations_under_a_bare_workspace_still_build_a_graph() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());

    let overview = mcp_graph_overview(dir.path(), &[]);
    assert_eq!(overview["nodes"], 2, "both nested configurations' methods: {overview}");
}

#[test]
fn the_mcp_status_reports_a_standalone_extension() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());
    let module =
        path(dir.path(), EXT).join("CommonModules").join(EXT_MODULE).join("Ext").join("Module.bsl");

    // Pointed straight at the extension, with no main configuration behind it —
    // the state in which this backend's findings are wrong and nothing else in
    // the protocol says why.
    let (_, standalone) = mcp_probe(&path(dir.path(), EXT), &module, &[]);
    assert!(
        standalone["standalone_extension"]
            .as_str()
            .is_some_and(|s| s.contains("configuration extension analyzed without")),
        "status must carry the notice: {standalone}"
    );

    let (_, bound) = mcp_probe(
        dir.path(),
        &module,
        &["--configuration-root", MAIN, "--extension", "EXT=a/b/ext"],
    );
    assert!(
        bound.get("standalone_extension").is_none(),
        "a bound main configuration must leave the field out: {bound}"
    );
}

#[test]
fn the_source_set_reaches_the_mcp_server() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());
    let module =
        path(dir.path(), EXT).join("CommonModules").join(EXT_MODULE).join("Ext").join("Module.bsl");

    assert_eq!(
        mcp_probe(dir.path(), &module, &[]).0,
        vec![MAIN_MODULE.to_string(), MISSING_MODULE.to_string()],
        "without a source set the main configuration's module cannot resolve"
    );
    assert_eq!(
        mcp_probe(
            dir.path(),
            &module,
            &["--configuration-root", MAIN, "--extension", "EXT=a/b/ext"]
        )
        .0,
        vec![MISSING_MODULE.to_string()],
        "the source set must resolve the main configuration's module here too"
    );
}
