use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::change_hub::{SinkCursor, WorkspaceChangeHub};

use super::lifecycle::{lock_recover, DiagnosticsState};
use super::types::{DiagnosticsStatus, ResidentOutcome};

pub(super) fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Write only a common module's descriptor XML (not its `Ext/Module.bsl`), so a test
/// can flip a metadata property as pure `.xml` drift without touching the body.
pub(super) fn write_common_module_xml(root: &Path, name: &str, server: bool) {
    let client = !server;
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="00000000-0000-0000-0000-0000000000{id:02}">
		<Properties>
			<Name>{name}</Name>
			<Global>false</Global>
			<ClientManagedApplication>{client}</ClientManagedApplication>
			<Server>{server}</Server>
			<ExternalConnection>false</ExternalConnection>
			<ClientOrdinaryApplication>{client}</ClientOrdinaryApplication>
			<ServerCall>false</ServerCall>
			<Privileged>false</Privileged>
			<ReturnValuesReuse>DontUse</ReturnValuesReuse>
		</Properties>
	</CommonModule>
</MetaDataObject>"#,
        id = name.len(),
    );
    write(root, &format!("CommonModules/{name}.xml"), &xml);
}

pub(super) fn write_common_module(root: &Path, name: &str, server: bool, body: &str) {
    write_common_module_xml(root, name, server);
    write(root, &format!("CommonModules/{name}/Ext/Module.bsl"), body);
}

pub(super) fn sample_workspace(root: &Path) {
    write_common_module(root, "Сервер", true, "&НаСервере\nФункция Считать() Экспорт КонецФункции");
}

pub(super) fn module_path(root: &Path, name: &str) -> PathBuf {
    root.join(format!("CommonModules/{name}/Ext/Module.bsl"))
}

/// The module both roots hold, spelled relative to whichever root owns it.
pub(crate) const SHARED_MODULE_REL: &str = "CommonModules/Общий/Ext/Module.bsl";

pub(crate) const CONFIGURATION_SYMBOL: &str = "ФункцияКонфигурации";
pub(crate) const EXTENSION_SYMBOL: &str = "ФункцияРасширения";

/// A workspace whose configuration IS the workspace directory and whose extension lives
/// outside it, both holding a module at [`SHARED_MODULE_REL`].
///
/// Every part of the shape is load-bearing:
///
/// - the extension is OUTSIDE the workspace because a root that canonically lies inside the
///   configuration is rejected rather than registered, and a stand built that way measures
///   the rejection instead of the subject;
/// - the configuration is the workspace directory itself, so a path read against the
///   workspace — today's reading — lands on a REAL file. A configuration in a subdirectory
///   would make the wrong reading miss instead, and a miss cannot tell "answered from the
///   namesake" from "answered from nothing";
/// - the two modules differ in TEXT. Identical bytes would make the two files
///   indistinguishable in any answer derived from content, and the whole point is telling
///   them apart.
///
/// Returns the temporary directory (kept alive by the caller), the workspace and the
/// extension root.
pub(crate) fn workspace_with_an_outside_extension() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("ws");
    let extension = dir.path().join("outside-ext");
    for (root, name) in [(&workspace, "Конфа"), (&extension, "Расш")] {
        fs::create_dir_all(root).unwrap();
        fs::write(
            root.join("Configuration.xml"),
            format!("<Configuration><Name>{name}</Name></Configuration>"),
        )
        .unwrap();
    }
    write_common_module_xml(&workspace, "Общий", true);
    write(
        &workspace,
        SHARED_MODULE_REL,
        &format!("&НаСервере\nФункция {CONFIGURATION_SYMBOL}() Экспорт Возврат 1; КонецФункции\n"),
    );
    write_common_module_xml(&extension, "Общий", true);
    write(
        &extension,
        SHARED_MODULE_REL,
        &format!("&НаСервере\nФункция {EXTENSION_SYMBOL}() Экспорт Возврат 2; КонецФункции\n"),
    );
    fs::write(
        workspace.join("bsl-analyzer.toml"),
        format!(
            "[source]\nroot = \".\"\nextensions = [{{ name = \"a\", path = {extension:?} }}]\n"
        ),
    )
    .unwrap();
    (dir, workspace, extension)
}

/// The identifier the root table gives the extension of
/// [`workspace_with_an_outside_extension`]. Read from the table rather than spelled out: a
/// root outside the workspace is identified by its absolute spelling, and hard-coding that
/// would pin the test to a rule it is not testing.
pub(crate) fn extension_root_id(workspace: &Path, extension: &Path) -> String {
    let project = crate::project::at(workspace).expect("the fixture is a valid project");
    let (roots, _rejected) = crate::project::workspace_roots(&project);
    let file = extension.join(SHARED_MODULE_REL);
    let canonical = file.canonicalize().expect("the extension's module exists");
    roots.root_of(&file, &canonical).expect("the extension's file has an owning root").root_id
}

/// A workspace whose configuration sits in a SUBDIRECTORY, with no extensions at all.
/// This is the shape where the configuration's own hits are already unreadable back: their
/// paths are spelled against the configuration root while the resident reads a bare relative
/// path against the project root, and the two differ only when the configuration is nested.
pub(crate) fn workspace_with_a_nested_configuration() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();
    let configuration = workspace.join("src").join("cf");
    fs::create_dir_all(&configuration).unwrap();
    fs::write(
        configuration.join("Configuration.xml"),
        "<Configuration><Name>Конфа</Name></Configuration>",
    )
    .unwrap();
    write_common_module_xml(&configuration, "Общий", true);
    write(
        &configuration,
        SHARED_MODULE_REL,
        &format!("&НаСервере\nФункция {CONFIGURATION_SYMBOL}() Экспорт Возврат 1; КонецФункции\n"),
    );
    fs::write(workspace.join("bsl-analyzer.toml"), "[source]\nroot = \"src/cf\"\n").unwrap();
    (dir, workspace)
}

/// Ждёт готовности базы, а исчерпав ожидание — говорит, чего именно дождался.
///
/// Голое «не стало готовым» о превышении бюджета и о зависшей загрузке
/// сообщает одинаково, и падение под полным прогоном приходится
/// расследовать заново. Поэтому в сообщении стоит последний наблюдённый
/// статус и потраченное время: по ним видно, упёрлись ли мы в бюджет на
/// загруженной машине или загрузка не двигалась вовсе.
pub(crate) fn wait_ready(state: &DiagnosticsState) {
    let started = std::time::Instant::now();
    let mut last = state.status();

    for _ in 0..300 {
        match state.status() {
            DiagnosticsStatus::Ready { .. } => return,
            DiagnosticsStatus::Failed(msg) => panic!("diagnostics load failed: {msg}"),
            other => {
                last = other;
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    panic!("diagnostics db did not become ready in {:?}, last status {last:?}", started.elapsed());
}

pub(super) fn write_catalog(root: &Path, name: &str, code_length: u32) {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="00000000-0000-0000-0000-0000000000{id:02}">
        <Properties><Name>{name}</Name><CodeLength>{code_length}</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#,
        id = name.len(),
    );
    write(root, &format!("Catalogs/{name}.xml"), &xml);
}

pub(super) fn catalog_resolves(state: &DiagnosticsState, module: &Path) -> bool {
    let out = state.read(|resident, _| {
        let fid = resident.file_id_for(module).expect("module resolves to a FileId");
        resident
            .db()
            .resolve_metadata_object_for_file(fid, bsl_metadata::MdoType::Catalog, "Товары")
            .is_some()
    });
    match out {
        ResidentOutcome::Ready(v, _) => v,
        _ => panic!("expected Ready outcome"),
    }
}

pub(super) fn module_diag_fingerprint(state: &DiagnosticsState, module: &Path) -> Vec<String> {
    let out = state.read(|resident, _| {
        let fid = resident.file_id_for(module).expect("module resolves to a FileId");
        let analysis = resident.analysis();
        let mut lines: Vec<String> =
            analysis.diagnostics(fid, resident.config()).iter().map(|d| format!("{d:?}")).collect();
        lines.sort();
        lines
    });
    match out {
        ResidentOutcome::Ready(v, _) => v,
        _ => panic!("expected Ready outcome"),
    }
}

#[cfg(unix)]
pub(super) fn module_is_server(state: &DiagnosticsState, module: &Path) -> Option<bool> {
    let out = state.read(|resident, _| {
        let fid = resident.file_id_for(module)?;
        Some(resident.db().common_module_for_file_id(fid)?.is_server())
    });
    match out {
        ResidentOutcome::Ready(v, _) => v,
        _ => panic!("expected Ready outcome"),
    }
}

pub(super) fn state_with_hub(root: &Path) -> (DiagnosticsState, WorkspaceChangeHub) {
    let hub = WorkspaceChangeHub::start(vec![root.to_path_buf()]);
    assert!(hub.wait_until_watching(Duration::from_secs(5)), "the hub must arm");
    let mut state =
        DiagnosticsState::for_workspace(root.to_path_buf()).with_change_hub(hub.clone());
    state.drift_interval = Duration::from_millis(0);
    (state, hub)
}

/// A closure that keeps the diagnostics sink lossy: it drains whatever the hub
/// holds for the state's cursor and throws it away, advancing the cursor exactly
/// as a delivery would.
///
/// Needed where `&state` is out of reach, since a reconciler probe owns everything
/// it touches; and needed at all because one write is not one event: the watcher
/// may emit another for the same file at any moment, and the reconciler counts a
/// path delivered during its scan as merely late rather than missed. A test that
/// discards once and hopes no duplicate arrives is testing the machine's timing,
/// not its rule.
pub(super) fn cursor_discarder(state: &DiagnosticsState) -> impl FnOnce() + Send + 'static {
    let hub = state.change_hub.clone();
    let cursor = state.hub_cursor.clone();
    move || {
        let current = *lock_recover(&cursor);
        if let (Some(hub), Some(current)) = (hub, current) {
            let batch = hub.drain(current);
            *lock_recover(&cursor) = Some(batch.cursor);
        }
    }
}

/// Keep discarding until the hub has nothing left to say, and report whether it
/// went quiet within the budget.
///
/// A completed `fs::write` is one edit but several raw events, and the hub's
/// accumulator records them on its own schedule. Draining once leaves whatever
/// has not landed yet to arrive later — during a reconcile tick it would be read
/// as a delivery, and a test simulating a LOST delivery would observe the
/// opposite of what it set up. Consuming to quiet removes those events instead
/// of racing them.
pub(super) fn discard_until_quiet(
    state: &DiagnosticsState,
    hub: &WorkspaceChangeHub,
    quiet_rounds: usize,
) -> bool {
    let mut quiet = 0;
    for _ in 0..300 {
        let before = hub.events_seen();
        state.drain_and_discard_cursor();
        if hub.events_seen() == before {
            quiet += 1;
            if quiet == quiet_rounds {
                return true;
            }
        } else {
            quiet = 0;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub(super) fn raw_generation(state: &DiagnosticsState) -> u64 {
    lock_recover(&state.inner).generation
}

pub(super) fn wait_for_apply(state: &DiagnosticsState, generation: u64) -> bool {
    for _ in 0..300 {
        let _ = state.read(|_, _| ());
        if raw_generation(state) > generation {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

pub(super) fn wait_for_delivery(
    hub: &WorkspaceChangeHub,
    cursor: &mut SinkCursor,
    needle: &str,
) -> bool {
    for _ in 0..300 {
        let batch = hub.drain(*cursor);
        *cursor = batch.cursor;
        if batch.entries.iter().any(|entry| entry.raw.to_string_lossy().contains(needle)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}
