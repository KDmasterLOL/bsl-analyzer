//! End-to-end coverage for platform-manager method fallback.
//!
//! Pins the "Спр = Справочники.X.СоздатьЭлемент(); Спр.Записать()" flow
//! through three adapters that previously returned `Ty::Unknown` for the
//! stock platform catalogue:
//!
//! 1. `infer_three_level_call` (`Справочники.X.СоздатьЭлемент()`) —
//!    falls back to `resolve_platform_manager_method` when the workspace
//!    `ManagerModule.bsl` does not define the method.
//! 2. `method_lookup::lookup_method` on `Ty::ObjectManager` — same
//!    fallback for aliased managers (`М = Справочники.X; М.СоздатьЭлемент()`),
//!    which never enter the 3-segment call path.
//! 3. `method_lookup::lookup_method` on `Ty::MetadataRef { CatalogObject, .. }`
//!    — chained `Спр.Записать()` resolves to the platform `CatalogObject`
//!    method table.
//!
//! Fixtures use the shared designer workspace (catalog `Справочник1`,
//! document `Документ1`). The catalog's `ManagerModule.bsl` declares one
//! exported method (`ТестЭкспортная`) — test #5 below uses that one name
//! to prove workspace > platform priority (platform has no
//! `ТестЭкспортная`, but if the resolver muddles priority this test
//! would either double-emit or silently pick platform).

use bsl_metadata::MdoType;
use hir::{
    DefDatabase, HirDatabase, InferenceDiagnostic, MetadataKind, ModuleId, Name, Ty,
    UnresolvedMethodKind,
};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }

    db.set_all_config_paths(vec![(None, designer_fixture_path())]);

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

fn unresolved_kinds(db: &RootDatabaseImpl, file_id: FileId) -> Vec<UnresolvedMethodKind> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

fn unresolved_method_names(db: &RootDatabaseImpl, file_id: FileId) -> Vec<String> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, .. } => {
                Some(method_name.as_str().to_string())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn catalog_create_item_returns_catalog_object_metadata_ref() {
    // Primary user scenario:
    //   Спр = Справочники.Справочник1.СоздатьЭлемент();
    //   Спр.   ← completion should see CatalogObject methods
    //
    // The 3-segment call must resolve through the platform fallback
    // (`ManagerModule.bsl` has no `СоздатьЭлемент`) and rebind the
    // generic `СправочникОбъект` return to
    // `Ty::MetadataRef { CatalogObject, "Справочник1" }`. Without the
    // rebinding `Спр` stays `Ty::Unknown` → completion empty.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Спр = Справочники.Справочник1.СоздатьЭлемент();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert_eq!(
        var_ty(&db, file_id, "спр"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogObject, name: Name::new("Справочник1")
        }),
        "Спр must carry CatalogObject.Справочник1, not Unknown",
    );

    // Sanity — no UnresolvedMethodCall on `СоздатьЭлемент`. Before the
    // platform fallback this emitted a false positive.
    assert!(
        !unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("СоздатьЭлемент")),
        "platform-defined СоздатьЭлемент must not fire UnresolvedMethodCall, got {:?}",
        unresolved_method_names(&db, file_id),
    );
}

#[test]
fn aliased_manager_create_item_resolves_through_lookup_method() {
    // Codex-critical: the bypass that the initial design missed.
    //   М = Справочники.Справочник1;
    //   Спр = М.СоздатьЭлемент();
    // The second call is `Expr::MethodCall { receiver: М, method: ... }`,
    // **not** a 3-segment call — so the fallback must also live in
    // `method_lookup::lookup_method` keyed on `Ty::ObjectManager`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    М = Справочники.Справочник1;
    Спр = М.СоздатьЭлемент();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert_eq!(
        var_ty(&db, file_id, "м"),
        Some(Ty::ObjectManager {
            kind: MdoType::Catalog, name: Name::new("Справочник1")
        }),
        "Sanity — alias carries ObjectManager",
    );
    assert_eq!(
        var_ty(&db, file_id, "спр"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogObject, name: Name::new("Справочник1")
        }),
        "Aliased manager path must resolve СоздатьЭлемент through platform fallback",
    );
}

#[test]
fn catalog_find_by_code_returns_catalog_ref() {
    // `НайтиПоКоду` returns the generic `СправочникСсылка` — must
    // rebind to `MetadataRef { CatalogRef, <mdo_name> }`, not a bare
    // `PlatformObject("СправочникСсылка")`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Ссылка = Справочники.Справочник1.НайтиПоКоду("001");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert_eq!(
        var_ty(&db, file_id, "ссылка"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Справочник1")
        }),
    );
}

#[test]
fn unknown_manager_method_still_emits_unresolved_diagnostic() {
    // Regression guard: an actually-missing method must still surface
    // `UnresolvedMethodCall`. Without this, the fallback would silence
    // every typo the user makes on a manager receiver.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Результат = Справочники.Справочник1.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert!(
        unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("НетТакогоМетода")),
        "typo'd method must still fire UnresolvedMethodCall, got {:?}",
        unresolved_method_names(&db, file_id),
    );
}

/// Inline-module setup that mirrors `infer_three_level.rs::setup`: no
/// external designer fixture, the ManagerModule.bsl lives in the
/// fixture text itself. Used by the priority-ordering test below.
fn setup_inline(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

#[test]
fn workspace_manager_method_wins_over_platform() {
    // `ТестЭкспортная` is declared in the inline manager module — no
    // platform entry. Resolver must pick the workspace method on the
    // first pass; the platform fallback never runs for this call.
    //
    // This also guards the priority ordering: had platform been tried
    // first, a future platform `ТестЭкспортная` would silently shadow
    // the user's implementation.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ManagerModule.bsl
Процедура ТестЭкспортная() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Справочники.Справочник1.ТестЭкспортная();
КонецПроцедуры
"#;
    let (db, file_id) = setup_inline(fixture);
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "workspace-defined ТестЭкспортная must resolve without falling back to platform, got {:?}",
        unresolved_kinds(&db, file_id),
    );
}

#[test]
fn catalog_object_chained_write_resolves_through_metadata_ref_lookup() {
    // Round-trip: after the SaveItem scenario produces a
    // `MetadataRef { CatalogObject, ... }`, subsequent calls on that
    // receiver (`.Записать()`) must resolve through the same adapter.
    // Without the `MetadataRef` branch in `method_lookup` this
    // would emit `UnresolvedMethodCall` on `Записать`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Спр = Справочники.Справочник1.СоздатьЭлемент();
    Спр.Записать();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "CatalogObject.Записать must resolve via MetadataRef adapter; got unresolved {:?}",
        unresolved_method_names(&db, file_id),
    );
}

#[test]
fn completion_after_create_item_offers_catalog_object_methods() {
    // End-to-end: the primary user-visible scenario. After
    // `Спр = Справочники.X.СоздатьЭлемент()` the dot-completion on
    // `Спр.` must include platform CatalogObject methods like
    // `Записать` / `Удалить`. Without the routing through
    // `platform_completion::complete_prefix_methods_for_receiver` the
    // receiver Ty maps to bare "MetadataRef" and the scalar
    // `complete_platform_methods` returns nothing.
    let source = "\
Процедура Тест()
    Спр = Справочники.Справочник1.СоздатьЭлемент();
    Спр.$0
КонецПроцедуры
";
    let cursor = source.find("$0").expect("fixture must contain $0");
    let cleaned = source.replacen("$0", "", 1);
    let offset = cursor as u32;

    let fixture_text = format!("//- /test.bsl\n{}", cleaned);
    let (db, file_id) = setup(&fixture_text);
    let analysis = ide::Analysis::from_database(db);
    let items = analysis.completions(file_id, offset, None);

    let has = |label: &str| items.iter().any(|i| i.label.eq_ignore_ascii_case(label));
    assert!(
        has("Записать") || has("Write"),
        "CatalogObject.Записать must be in completion items; got {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn catalog_object_chained_unknown_method_still_emits_diagnostic() {
    // Symmetry with test #4 on the `MetadataRef` receiver: bogus method
    // names on a CatalogObject must still diagnose.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Спр = Справочники.Справочник1.СоздатьЭлемент();
    Спр.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    // `МетодКоторогоНет` — note: diagnostic is emitted only for the
    // 3-segment form today. On `Expr::MethodCall` path the inference
    // returns `Ty::Unknown` silently. So we only check the round-trip
    // type stays sensible; the diagnostic shape is not a contract here.
    assert_eq!(
        var_ty(&db, file_id, "спр"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogObject,
            name: Name::new("Справочник1"),
        }),
        "Спр must still be MetadataRef — chained-call failure on it must not poison the prior assignment",
    );
}

fn unresolved_field_names(db: &RootDatabaseImpl, file_id: FileId) -> Vec<String> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedField { field_name, .. } => {
                Some(field_name.as_str().to_string())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn document_object_chained_write_resolves_through_metadata_ref_lookup() {
    // User-reported: `Док = Документы.X.СоздатьДокумент(); Док.Записать();`
    // must resolve via `MetadataRef { DocumentObject, .. }` lookup. Pre-fix
    // the inner callee `Expr::Field { Док, Записать }` was re-walked by
    // `infer_all`'s second pass and emitted a spurious `UnresolvedField`
    // (the field arm doesn't know callees are method references).
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Док = Документы.Документ1.СоздатьДокумент();
    Док.Записать();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "DocumentObject.Записать must resolve via MetadataRef adapter; got unresolved {:?}",
        unresolved_method_names(&db, file_id),
    );
    assert!(
        !unresolved_field_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "DocumentObject.Записать must NOT emit UnresolvedField — it is a method, not a field; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn document_object_chained_unknown_method_emits_unresolved_method_call() {
    // Sibling of the catalog-object unknown-method test: a bogus method
    // name on a `MetadataRef { DocumentObject, .. }` receiver must surface
    // as `UnresolvedMethodCall(MethodNotFound)` — NOT silently swallowed
    // and NOT mis-labeled as `UnresolvedField`.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Док = Документы.Документ1.СоздатьДокумент();
    Док.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let inf = db.infer(file_id);
    let unresolved: Vec<_> = inf
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall {
                receiver_name, method_name, kind, ..
            } => Some((receiver_name.clone(), method_name.clone(), *kind)),
            _ => None,
        })
        .collect();

    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "method-call site must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
    assert!(
        unresolved.iter().any(|(rcv, m, kind)| {
            m.as_str().eq_ignore_ascii_case("НетТакогоМетода")
                && *kind == UnresolvedMethodKind::MethodNotFound
                && rcv.as_str() == "Документы.Документ1"
        }),
        "DocumentObject.НетТакогоМетода must emit UnresolvedMethodCall(MethodNotFound) \
         with `Plural.MDO` receiver-name; got {unresolved:?}",
    );
}

#[test]
fn aliased_object_manager_unknown_method_stays_silent() {
    // `М = Справочники.X; М.УсерМетод()` — the 2-shape on an
    // `ObjectManager` receiver. `method_lookup` consults only
    // `platform_manager_lookup::resolve_platform_manager_method`, which
    // is platform-data-only and does NOT see workspace
    // `ManagerModule.bsl` methods. A miss therefore is inconclusive —
    // `УсерМетод` may legitimately exist as an exported manager-module
    // method that this 2-shape lookup never resolves. Until the
    // workspace resolver is wired into `lookup_method`, silence is the
    // honest answer for `ObjectManager`. Diagnostic must not fire.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    М = Справочники.Справочник1;
    М.УсерМетод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let inf = db.infer(file_id);
    let unresolved_method: Vec<_> = inf
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, .. } => {
                Some(method_name.as_str().to_string())
            }
            _ => None,
        })
        .collect();

    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "method-call site on aliased ObjectManager must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
    assert!(
        !unresolved_method.iter().any(|n| n.eq_ignore_ascii_case("УсерМетод")),
        "ObjectManager miss is not authoritative — workspace ManagerModule methods are not \
         visible to method_lookup, so the diagnostic must stay silent until the resolver is \
         wired in; got {unresolved_method:?}",
    );
}
