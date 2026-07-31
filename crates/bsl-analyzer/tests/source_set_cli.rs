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
            "Процедура Вызвать() Экспорт\n\t{MAIN_MODULE}.Экспортируемая();\nКонецПроцедуры\n"
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
    fn file_event(&self, module: &str) -> Option<&Value> {
        self.files.iter().find(|e| e["path"].as_str().is_some_and(|p| p.contains(module)))
    }

    /// Whether the module's own file carries the diagnostic — and, when it does
    /// not, that the file was analyzed at all and did not fail. Absence of a
    /// finding in a file nobody parsed is not a resolution.
    fn has_diagnostic(&self, module: &str, code: &str) -> bool {
        let event = self
            .file_event(module)
            .unwrap_or_else(|| panic!("{module} was not analyzed at all; jsonl: {:?}", self.files));
        assert_eq!(event["error"], Value::Null, "{module} failed to analyze: {event}");
        assert_eq!(self.done["failed_files"], 0, "some file failed: {}", self.done);
        event["diagnostics"].as_array().unwrap().iter().any(|d| d["code"].as_str() == Some(code))
    }
}

fn analyze(source_dir: &Path, flags: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_bsl-analyzer-app"))
        .arg("analyze")
        .arg("-s")
        .arg(source_dir)
        .args(flags)
        .args(["--format", "jsonl"])
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
fn binding_the_main_configuration_resolves_calls_into_it() {
    let dir = workspace();
    workspace_calling_main_configuration(dir.path());

    // Positive control. Without a main configuration the call cannot resolve,
    // and this assertion is what makes the paired one below mean anything.
    let standalone = analyze(dir.path(), &["--extension", &format!("EXT={EXT}")]);
    assert!(
        standalone.has_diagnostic(EXT_MODULE, "UnresolvedMethodCall"),
        "the extension analyzed alone must fail to resolve the main configuration's method"
    );

    let bound =
        analyze(dir.path(), &["--configuration-root", MAIN, "--extension", &format!("EXT={EXT}")]);
    assert!(
        !bound.has_diagnostic(EXT_MODULE, "UnresolvedMethodCall"),
        "binding the main configuration must resolve the call"
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
        format!("Процедура Вызвать() Экспорт\n\t{DEP_MODULE}.ИзЗависимости();\nКонецПроцедуры\n"),
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
    assert!(
        unrelated.has_diagnostic(EXT_MODULE, "UnresolvedMethodCall"),
        "without a declared edge the other extension's API must stay invisible"
    );

    let mut with_edge = refs.clone();
    with_edge.extend(["--extension-depends-on", "EXT=DEP"]);
    let dependent = analyze(dir.path(), &with_edge);
    assert!(
        !dependent.has_diagnostic(EXT_MODULE, "UnresolvedMethodCall"),
        "the declared edge must make the dependency's API visible"
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
    assert!(
        configured.file_event(EXT_MODULE).is_some(),
        "the configured extension must be analyzed when the flag is absent"
    );

    let opted_out = analyze(dir.path(), &["--no-extensions"]);
    assert!(
        opted_out.file_event(EXT_MODULE).is_none(),
        "--no-extensions must drop the list the config declared"
    );
    // The flag drops extensions, not the analysis. Without this, a wiring bug
    // that cleared every source root would satisfy the assertion above by
    // analyzing nothing at all.
    assert!(
        opted_out.file_event(MAIN_MODULE).is_some(),
        "--no-extensions must leave the main configuration in the analysis"
    );
}

/// How many times the notice appears — the invariant is exactly one message per
/// run, and a substring check would pass just as happily on a duplicate.
fn notices(run: &Run) -> usize {
    run.stderr
        .matches("is a configuration extension analyzed without its main configuration")
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
