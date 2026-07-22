//! Dependency-aware CFE visibility: a file sees the base, its extension's
//! declared transitive dependencies and its own extension — never an
//! unrelated sibling. Exercises the acceptance matrix on the
//! `cfe_dependencies` fixture: base + yaxunit + tests (dependsOn yaxunit)
//! + independent.

use hir::ConfigsDatabase;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::metadata::WorkspaceConfigsSnapshot;
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, FileSet, VfsPath};

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/cfe_dependencies"))
        .join(name)
}

struct Files {
    base: FileId,
    yaxunit: FileId,
    tests: FileId,
    independent: FileId,
}

fn setup() -> (RootDatabaseImpl, Files) {
    let mut db = RootDatabaseImpl::new();

    let files = Files {
        base: FileId::from_raw(1),
        yaxunit: FileId::from_raw(2),
        tests: FileId::from_raw(3),
        independent: FileId::from_raw(4),
    };
    let mut file_set = FileSet::default();
    let mut add = |file_id: FileId, root: &str, module: &str| {
        let path = fixture_root(root).join(format!("CommonModules/{module}/Ext/Module.bsl"));
        file_set.insert(file_id, VfsPath::new(path.to_string_lossy().to_string()));
    };
    add(files.base, "base", "БазовыйМодуль");
    add(files.yaxunit, "yaxunit", "МодульЮнит");
    add(files.tests, "tests_ext", "МодульТестов");
    add(files.independent, "independent", "МодульНезависимый");
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for file_id in [files.base, files.yaxunit, files.tests, files.independent] {
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, "Процедура Тест() КонецПроцедуры");
    }

    let paths = vec![
        (None, fixture_root("base")),
        (Some("yaxunit".to_string()), fixture_root("yaxunit")),
        (Some("tests".to_string()), fixture_root("tests_ext")),
        (Some("independent".to_string()), fixture_root("independent")),
    ];
    let canonical_paths =
        paths.iter().map(|(_, p)| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
    // `tests` (slot 2) depends on `yaxunit` (slot 1); everything else is independent.
    let snapshot = WorkspaceConfigsSnapshot {
        paths,
        canonical_paths,
        closures: vec![Vec::new(), Vec::new(), vec![1], Vec::new()],
        fingerprint: None,
    };
    db.set_workspace_configs_snapshot(snapshot);
    (db, files)
}

fn config_names(db: &RootDatabaseImpl, file_id: FileId) -> Vec<Option<String>> {
    db.configurations(file_id).into_iter().map(|c| c.name).collect()
}

#[test]
fn visible_configurations_follow_the_dependency_matrix() {
    let (db, files) = setup();

    assert_eq!(config_names(&db, files.base), [None], "a base file sees only the base");
    assert_eq!(
        config_names(&db, files.yaxunit),
        [None, Some("yaxunit".to_string())],
        "a dependency does not see its dependents",
    );
    assert_eq!(
        config_names(&db, files.tests),
        [None, Some("yaxunit".to_string()), Some("tests".to_string())],
        "a dependent sees base, its declared dependency, then itself",
    );
    assert_eq!(
        config_names(&db, files.independent),
        [None, Some("independent".to_string())],
        "an independent extension sees only base + itself",
    );
}

#[test]
fn common_module_resolution_follows_the_dependency_matrix() {
    let (db, files) = setup();
    let resolve = |file_id: FileId, name: &str| db.resolve_common_module(file_id, name).is_some();

    assert!(resolve(files.tests, "БазовыйМодуль"));
    assert!(resolve(files.tests, "МодульЮнит"), "TESTS must see its dependency's module");
    assert!(resolve(files.tests, "МодульТестов"));
    assert!(!resolve(files.tests, "МодульНезависимый"), "TESTS must not see unrelated CFE");

    assert!(!resolve(files.yaxunit, "МодульТестов"), "a dependency must not see its dependent");
    assert!(resolve(files.yaxunit, "МодульЮнит"));

    assert!(!resolve(files.independent, "МодульЮнит"));
    assert!(!resolve(files.independent, "МодульТестов"));
    assert!(resolve(files.independent, "МодульНезависимый"));

    assert!(!resolve(files.base, "МодульЮнит"), "a base file sees no CFE modules");
    assert!(resolve(files.base, "БазовыйМодуль"));
}

fn catalog_attribute_names(db: &RootDatabaseImpl, file_id: FileId, catalog: &str) -> Vec<String> {
    db.merged_visible_configuration(file_id)
        .and_then(|config| {
            config
                .find_metadata_object(bsl_metadata::MdoType::Catalog, catalog)
                .map(|mdo| mdo.attributes.iter().map(|a| a.name.clone()).collect())
        })
        .unwrap_or_default()
}

#[test]
fn chain_merge_composes_forward_and_preserves_inherited_fields() {
    let (db, files) = setup();

    let tests_attrs = catalog_attribute_names(&db, files.tests, "Товары");
    for attr in ["Цвет", "Вес", "Габарит"] {
        assert!(
            tests_attrs.iter().any(|a| a == attr),
            "TESTS must see base + dependency + own overlay attributes; missing {attr} in {tests_attrs:?}",
        );
    }

    let yaxunit_attrs = catalog_attribute_names(&db, files.yaxunit, "Товары");
    assert!(yaxunit_attrs.iter().any(|a| a == "Цвет"), "base attribute must survive the overlay");
    assert!(yaxunit_attrs.iter().any(|a| a == "Вес"));
    assert!(
        !yaxunit_attrs.iter().any(|a| a == "Габарит"),
        "a dependent's overlay must not leak into its dependency",
    );

    let independent_attrs = catalog_attribute_names(&db, files.independent, "Товары");
    assert!(
        !independent_attrs.iter().any(|a| a == "Вес"),
        "an unrelated extension's overlay must not leak",
    );
    assert!(
        db.merged_visible_configuration(files.independent)
            .and_then(|c| c
                .find_metadata_object(bsl_metadata::MdoType::Catalog, "СвойОбъект")
                .map(|_| ()))
            .is_some(),
        "the independent extension still sees its own objects",
    );

    let base_attrs = catalog_attribute_names(&db, files.base, "Товары");
    assert!(base_attrs.iter().any(|a| a == "Цвет"));
    assert!(
        !base_attrs.iter().any(|a| a == "Вес" || a == "Габарит"),
        "a base file sees the un-overlaid base object: {base_attrs:?}",
    );
}

#[test]
fn editing_an_unrelated_extension_does_not_invalidate_the_chain() {
    let (mut db, files) = setup();

    let before = db.merged_visible_configuration(files.tests).expect("merged config");
    db.bump_config_for_path(&fixture_root("independent"));
    let after_unrelated = db.merged_visible_configuration(files.tests).expect("merged config");
    assert!(
        std::sync::Arc::ptr_eq(&before, &after_unrelated),
        "an unrelated extension's change must not recompute TESTS' merged configuration",
    );

    db.bump_config_for_path(&fixture_root("yaxunit"));
    let after_dependency = db.merged_visible_configuration(files.tests).expect("merged config");
    assert!(
        !std::sync::Arc::ptr_eq(&before, &after_dependency),
        "a dependency's change must invalidate its dependents",
    );
}
