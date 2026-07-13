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

pub(super) fn wait_ready(state: &DiagnosticsState) {
    for _ in 0..300 {
        match state.status() {
            DiagnosticsStatus::Ready { .. } => return,
            DiagnosticsStatus::Failed(msg) => panic!("diagnostics load failed: {msg}"),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("diagnostics db did not become ready");
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
