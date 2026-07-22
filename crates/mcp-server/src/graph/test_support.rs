use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::cache::graph_db_path;
use crate::graph_db::build_graph_database;

use super::build::GRAPH_BUILD_BATCH;
use super::{GraphState, GraphStatus};

pub(super) fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Minimal common-module metadata descriptor so the module is declared in the
/// configuration (the resolver refuses qualified calls to undeclared modules)
/// and its client/server execution context is known.
pub(super) fn write_common_module(root: &Path, name: &str, server: bool, body: &str) {
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
    write(root, &format!("CommonModules/{name}/Ext/Module.bsl"), body);
}

pub(super) fn sample_workspace(root: &Path) {
    write_common_module(
        root,
        "Клиент",
        false,
        "&НаКлиенте\nПроцедура Главная() Экспорт\nСервер.Считать();\nКонецПроцедуры",
    );
    write_common_module(root, "Сервер", true, "&НаСервере\nФункция Считать() Экспорт КонецФункции");
}

pub(super) fn wait_ready(graph: &GraphState) {
    for _ in 0..200 {
        match graph.status() {
            GraphStatus::Ready { .. } => return,
            GraphStatus::Failed(msg) => panic!("graph load failed: {msg}"),
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    panic!("graph did not become ready");
}

/// A workspace with two declared extensions; `depends_on` toggles the one
/// dependency edge without touching any `.bsl`/`.xml` file.
pub(super) fn write_extension_workspace(root: &Path, depends_on: bool) {
    sample_workspace(root);
    // The base marker keeps the configured `root = "."` resolving to ROOT itself;
    // without it source discovery walks on and picks the first extension dir.
    write(root, "Configuration.xml", "<Configuration/>");
    write(root, "ext/a/Configuration.xml", "<Configuration/>");
    write(root, "ext/b/Configuration.xml", "<Configuration/>");
    write_extension_config(root, depends_on);
}

/// Rewrite ONLY the analyzer config — the drift under test must never come from
/// a scanned file's stat moving.
pub(super) fn write_extension_config(root: &Path, depends_on: bool) {
    let deps = if depends_on { ", dependsOn = [\"a\"]" } else { "" };
    fs::write(
        root.join("bsl-analyzer.toml"),
        format!(
            "[source]\nroot = \".\"\nextensions = [\n  \
             {{ name = \"a\", path = \"ext/a\" }},\n  \
             {{ name = \"b\", path = \"ext/b\"{deps} }},\n]\n"
        ),
    )
    .unwrap();
}

pub(super) fn seed_cache(root: &Path, fingerprint: crate::graph_db::GraphFp) {
    let out = graph_db_path(root);
    fs::create_dir_all(out.parent().unwrap()).unwrap();
    build_graph_database(
        &crate::graph::ProjectSnapshot::load(root),
        &out,
        GRAPH_BUILD_BATCH,
        &crate::graph_db::GraphMeta {
            revision: 7,
            fingerprint,
            files: 0,
            built_at: "cached-build-sentinel".to_string(),
        },
    )
    .expect("seed cache builds");
}

pub(super) fn meta_string(path: &Path, key: &str) -> String {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT value FROM meta WHERE key=?1", [key], |row| row.get(0))
        .unwrap()
}
