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

/// Variant of [`setup`] for tests that need the inference target to
/// be a file *other than* `test.bsl` — typically an inline
/// `ObjectModule.bsl` whose own body contains `ЭтотОбъект` calls.
///
/// `this_object::resolve_this_object_owner` reads `db.module_metadata(...)`,
/// which is keyed off the file's path: the file path must walk up to a
/// configuration root (designer fixture) for `ModuleType::ObjectModule`
/// + `ModuleMetadata.mdo` to be populated.
///
/// This helper rewrites each inline fixture path
/// (`//- /Catalogs/Справочник1/Ext/ObjectModule.bsl`) to its absolute
/// counterpart under the designer fixture root, so the path-based
/// metadata detection mirrors a real workspace. File *content* is the
/// inline test text — the on-disk file is overlaid via
/// `set_file_text`.
fn setup_with_target_path(
    fixture_text: &str,
    target_path_suffix: &str,
) -> (RootDatabaseImpl, FileId) {
    let designer = designer_fixture_path();
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    let mapped: Vec<(FileId, vfs::VfsPath)> = fixture
        .files
        .iter()
        .map(|(id, f)| {
            let virt = f.path.as_path().to_string_lossy();
            let suffix = virt.trim_start_matches('/');
            let abs = designer.join(suffix);
            (*id, vfs::VfsPath::new(abs.to_string_lossy().to_string()))
        })
        .collect();
    for (file_id, vfs_path) in &mapped {
        file_set.insert(*file_id, vfs_path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    db.set_all_config_paths(vec![(None, designer)]);
    // Normalize separators so the suffix match works on Windows too —
    // `VfsPath` keeps native separators on disk (`\\` on Windows), but
    // tests author the suffix with forward slashes (`/`).
    let needle = target_path_suffix.replace('\\', "/");
    let target = mapped
        .iter()
        .find(|(_, p)| p.as_path().to_string_lossy().replace('\\', "/").ends_with(&needle))
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("fixture must contain {target_path_suffix}"));
    let _ = db.module_bodies(ModuleId::new(target));
    (db, target)
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
fn catalog_find_by_code_string_arg_does_not_fire_type_mismatch() {
    // Regression: `НайтиПоКоду` declares `Код: Число, Строка` in
    // platform_data — the manager-method lowering path used to lift the
    // raw "Число, Строка" string into a single `Ty::PlatformObject`,
    // which made `String → expected` fail structural equality and
    // false-fired `TypeMismatch` for `НайтиПоКоду("796")`. The fix
    // routes manager params through `lower_param_type`, the same
    // comma-aware splitter used by tabular-section / fluent paths, so
    // the param lowers to `Ty::Union([Number, String])` and the union-
    // right rule accepts a `String` literal.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Ссылка = Справочники.Справочник1.НайтиПоКоду("796");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);

    let mismatches: Vec<_> = db
        .arg_diagnostics(file_id)
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
                Some((expected.clone(), actual.clone()))
            }
            _ => None,
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "String literal must be assignable to FindByCode's `Число, Строка` param — got {mismatches:?}",
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
    let items = analysis.completions(file_id, offset, None, ide::Locale::Ru);

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
fn aliased_manager_workspace_method_resolves() {
    // Phase A primary scenario:
    //   М = Справочники.Справочник1; М.ТестЭкспортная()
    // The 2-shape callee lowers to
    // `Expr::Call { callee: Expr::Field {..} }` with `Ty::ObjectManager`
    // receiver — `lookup_method`'s manager table is platform-only, so
    // without `resolve_aliased_manager_call` the call would surface a
    // false-positive `MethodNotFound`.
    //
    // The fixture inlines `ManagerModule.bsl` because the designer
    // fixture path supplies *configuration* data only (so
    // `Ty::ObjectManager` is produced); BSL source files from disk are
    // not loaded into the SourceRoot — `module_index.resolve_manager`
    // finds only the inline path.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ManagerModule.bsl
Процедура ТестЭкспортная() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = Справочники.Справочник1;
    М.ТестЭкспортная();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("ТестЭкспортная")),
        "exported workspace ManagerModule method must resolve through Phase A resolver; got {:?}",
        unresolved_method_names(&db, file_id),
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "method-call site must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn aliased_manager_non_exported_method_emits_method_not_export() {
    // `ТестНеЭкспортная` exists in the inline `ManagerModule.bsl` but
    // lacks the `Экспорт` keyword. Workspace resolver finds the
    // method; the call site must surface `MethodNotExport` (the user
    // forgot the keyword), distinct from `MethodNotFound` (typo'd or
    // missing method).
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ManagerModule.bsl
Процедура ТестНеЭкспортная()
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = Справочники.Справочник1;
    М.ТестНеЭкспортная();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let kinds: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, kind, .. }
                if method_name.as_str().eq_ignore_ascii_case("ТестНеЭкспортная") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotExport],
        "non-exported workspace ManagerModule method must surface MethodNotExport, got {kinds:?}",
    );
}

#[test]
fn aliased_manager_typo_emits_method_not_found() {
    // Inverse of the prior `aliased_object_manager_unknown_method_stays_silent`:
    // with the Phase A workspace resolver wired in, `ObjectManager` is
    // now authoritative. `М = Справочники.Справочник1; М.УсерМетод()`
    // misses both the workspace `ManagerModule.bsl` AND the platform
    // manager catalogue — the call surface for that `(MdoType, name)`
    // pair has been exhaustively consulted, so a miss conclusively
    // means "method does not exist".
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    М = Справочники.Справочник1;
    М.УсерМетод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let entries: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall {
                receiver_name, method_name, kind, ..
            } if method_name.as_str().eq_ignore_ascii_case("УсерМетод") => {
                Some((receiver_name.as_str().to_string(), *kind))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        entries,
        vec![("Справочники.Справочник1".to_string(), UnresolvedMethodKind::MethodNotFound)],
        "ObjectManager miss is now authoritative (workspace + platform exhausted); got {entries:?}",
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "method-call site must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn aliased_register_manager_workspace_method_resolves() {
    // Register-flavour parallel to
    // `aliased_manager_workspace_method_resolves`. Locks the Phase A.1
    // visibility-gate extension: without `mdo_visible_in_configs`
    // probing `Configuration::find_register_by_type_and_name`, the
    // register MDO would be falsely invisible (it lives in
    // `Configuration::registers`, not `metadata_objects`) and the
    // resolver would short-circuit to `NotVisibleInConfigs`.
    let fixture = r#"
//- /InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl
Процедура НеУстаревшаяПроцедура() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = РегистрыСведений.РегистрСведений1;
    М.НеУстаревшаяПроцедура();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("НеУстаревшаяПроцедура")),
        "exported register ManagerModule method must resolve through Phase A resolver; got {:?}",
        unresolved_method_names(&db, file_id),
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "register manager method-call site must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn register_recordset_typo_emits_method_not_found() {
    // Phase C: register-record `MetadataRef` receivers
    // (`InformationRegisterRecordManager`,
    // `AccumulationRegisterRecordSet`) now go through the workspace
    // `RecordSetModule.bsl` resolver before the platform layer. Both
    // miss `НесуществующийМетод` (no inline RecordSetModule.bsl, no
    // platform table) → authoritative `MethodNotFound`.
    //
    // `СоздатьМенеджерЗаписи` rebinds its return type to
    // `Ty::MetadataRef { InformationRegisterRecordManager, .. }` via
    // the Phase C extension to `map_generic_metadata_return_type`,
    // so the chained call enters the same workspace+platform path as
    // catalog `Об`/`Спр` receivers.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    МЗ = РегистрыСведений.РегистрСведений1.СоздатьМенеджерЗаписи();
    МЗ.НесуществующийМетод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let entries: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall {
                receiver_name, method_name, kind, ..
            } if method_name.as_str().eq_ignore_ascii_case("НесуществующийМетод") => {
                Some((receiver_name.as_str().to_string(), *kind))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        entries,
        vec![(
            "РегистрыСведений.РегистрСведений1".to_string(),
            UnresolvedMethodKind::MethodNotFound
        )],
        "register record-manager miss is now authoritative (workspace + platform exhausted); \
         got {entries:?}",
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "register recordmanager method-call must not emit UnresolvedField, got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn record_manager_platform_method_resolves() {
    // Phase C interaction: now that
    // `map_generic_metadata_return_type` rebinds
    // `СоздатьМенеджерЗаписи` to
    // `Ty::MetadataRef { InformationRegisterRecordManager, .. }`,
    // platform methods declared in the
    // `InformationRegisterRecordManager.<Имя>` composite typename
    // (`Записать`, `Прочитать`, `Удалить`, …) MUST stay resolvable —
    // restoring `mdo_kind_to_plural` in Phase C without also wiring
    // `platform_prefix()` for register-record kinds would
    // false-positive every legitimate platform call. Pinned
    // explicitly because the platform-side wiring is what makes
    // Phase C's authoritative diagnostic safe.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    МЗ = РегистрыСведений.РегистрСведений1.СоздатьМенеджерЗаписи();
    МЗ.Записать();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "platform Записать on InformationRegisterRecordManager must resolve through platform \
         layer; got {:?}",
        unresolved_method_names(&db, file_id),
    );
}

#[test]
fn record_set_platform_method_resolves_on_information_register() {
    // After wiring `MetadataKind::InformationRegisterRecordSet`,
    // `СоздатьНаборЗаписей()` rebinds to a typed record-set receiver
    // and platform methods declared under
    // `InformationRegisterRecordSet.<Имя>` (`Записать`, `Загрузить`,
    // `Очистить`, …) MUST resolve through the metadata-ref path.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    НЗ = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    НЗ.Записать();
    НЗ.Загрузить(Новый ТаблицаЗначений);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let unresolved = unresolved_method_names(&db, file_id);
    assert!(
        !unresolved.iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "platform Записать on InformationRegisterRecordSet must resolve; got {unresolved:?}",
    );
    assert!(
        !unresolved.iter().any(|n| n.eq_ignore_ascii_case("Загрузить")),
        "platform Загрузить on InformationRegisterRecordSet must resolve; got {unresolved:?}",
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "record-set platform method calls must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn record_set_filter_dimension_set_method_resolves() {
    // Repro of the original user defect: the full chain
    // `НаборЗаписей.Отбор.<Изм>.Установить(...)` must fully resolve.
    // Step-by-step:
    //   1. `СоздатьНаборЗаписей()` → `MetadataRef{InformationRegisterRecordSet}`
    //   2. `.Отбор` → synthetic `MetadataRef{RegisterFilter{InformationRegister}}`
    //   3. `.Справочник1` (the fixture's only dimension) →
    //      `Ty::PlatformObject("ЭлементОтбора")`
    //   4. `.Установить(...)` → platform `FilterItem.Установить` method
    let fixture = r#"
//- /test.bsl
Процедура Тест(Знач Значение)
    НЗ = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    НЗ.Отбор.Справочник1.Установить(Значение);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Установить")),
        "FilterItem.Установить must resolve through scalar-key + platform path; got {:?}",
        unresolved_method_names(&db, file_id),
    );
    let unresolved_fields = unresolved_field_names(&db, file_id);
    assert!(
        !unresolved_fields.iter().any(|n| n.eq_ignore_ascii_case("Отбор")),
        "synthetic .Отбор must resolve as a field; got unresolved fields: {unresolved_fields:?}",
    );
    assert!(
        !unresolved_fields.iter().any(|n| n.eq_ignore_ascii_case("Справочник1")),
        "dimension Справочник1 must resolve through Filter member surface; \
         got unresolved fields: {unresolved_fields:?}",
    );
}

#[test]
fn record_set_filter_method_resolves_through_scalar_key() {
    // Filter's own scalar methods (`Сбросить`, `Получить`, `Найти`,
    // …) must be reachable on a `RegisterFilter` receiver via the
    // `metadata_ref_scalar_key` side channel. Their HBK rows live
    // under `type_name = "Filter"`, not a composite prefix.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    НЗ = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    НЗ.Отбор.Сбросить();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Сбросить")),
        "Filter.Сбросить must resolve through scalar-key path; got {:?}",
        unresolved_method_names(&db, file_id),
    );
}

#[test]
fn aliased_record_set_workspace_method_unresolved_keeps_strict_diagnostic() {
    // `RecordSetModule.bsl` for InformationRegister is now in scope
    // (after the new `record_set_kind_to_mdo` arm). The fixture's
    // RecordSetModule.bsl has no exported `НесуществующийМетод`; the
    // chain below must surface an authoritative `MethodNotFound`,
    // not silently fall through to "type unknown".
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    НЗ = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    НЗ.НесуществующийМетод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let entries: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, kind, .. }
                if method_name.as_str().eq_ignore_ascii_case("НесуществующийМетод") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        entries,
        vec![UnresolvedMethodKind::MethodNotFound],
        "record-set miss is authoritative now that workspace+platform paths are wired; \
         got {entries:?}",
    );
}

#[test]
fn metadata_ref_object_module_workspace_method_resolves() {
    // Phase B primary scenario:
    //   Об = Справочники.Справочник1.СоздатьЭлемент();
    //   Об.МойОбъектныйМетод()
    // `СоздатьЭлемент` returns `Ty::MetadataRef { CatalogObject, .. }`
    // (platform fallback). The chained call's receiver enters the
    // workspace `ObjectModule.bsl` resolver before `lookup_method`
    // (which is platform-only); without the Phase B wiring this would
    // emit a false-positive `MethodNotFound`.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура МойОбъектныйМетод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Об = Справочники.Справочник1.СоздатьЭлемент();
    Об.МойОбъектныйМетод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("МойОбъектныйМетод")),
        "exported workspace ObjectModule method must resolve through Phase B resolver; got {:?}",
        unresolved_method_names(&db, file_id),
    );
    assert!(
        unresolved_field_names(&db, file_id).is_empty(),
        "method-call site must not emit UnresolvedField; got {:?}",
        unresolved_field_names(&db, file_id),
    );
}

#[test]
fn metadata_ref_object_module_non_exported_emits_method_not_export() {
    // `НеЭкспортный` is declared in the inline `ObjectModule.bsl` but
    // lacks `Экспорт`. BSL semantics: `Об.НеЭкспортный()` is external
    // access through an object reference, which only sees `Экспорт`
    // methods. So the workspace resolver finds the method but the
    // call site must surface `MethodNotExport`.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура НеЭкспортный()
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Об = Справочники.Справочник1.СоздатьЭлемент();
    Об.НеЭкспортный();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let kinds: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, kind, .. }
                if method_name.as_str().eq_ignore_ascii_case("НеЭкспортный") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotExport],
        "non-exported workspace ObjectModule method must surface MethodNotExport, got {kinds:?}",
    );
}

#[test]
fn metadata_ref_workspace_then_platform_fallback() {
    // Workspace resolver misses (the inline `ObjectModule.bsl` does
    // not define `Записать`) but `Записать` is a platform method on
    // `CatalogObject`. The platform fallback must run after the
    // workspace miss — this pins the precedence so a future
    // workspace-only treatment doesn't break stock platform
    // resolution.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура НикакЗаписатьНеНазывается() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Об = Справочники.Справочник1.СоздатьЭлемент();
    Об.Записать();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        !unresolved_method_names(&db, file_id).iter().any(|n| n.eq_ignore_ascii_case("Записать")),
        "platform `Записать` must resolve after workspace miss; got {:?}",
        unresolved_method_names(&db, file_id),
    );
}

#[test]
fn metadata_ref_total_miss_emits_method_not_found() {
    // Both workspace `ObjectModule.bsl` AND platform
    // `CatalogObject` table miss `НетТакогоМетода`. With Phase B the
    // workspace+platform pair is fully consulted, so the diagnostic
    // is now authoritative.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура ЕстьВсего() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Об = Справочники.Справочник1.СоздатьЭлемент();
    Об.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let entries: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall {
                receiver_name, method_name, kind, ..
            } if method_name.as_str().eq_ignore_ascii_case("НетТакогоМетода") => {
                Some((receiver_name.as_str().to_string(), *kind))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        entries,
        vec![("Справочники.Справочник1".to_string(), UnresolvedMethodKind::MethodNotFound)],
        "MetadataRef.*Object miss is now authoritative; got {entries:?}",
    );
}

#[test]
fn catalog_ref_does_not_consult_object_module() {
    // `Сс` carries `Ty::MetadataRef { CatalogRef, .. }`. The strict
    // `*Object` filter inside `resolve_object_module_call` must
    // reject `*Ref` kinds — a reference value's surface is
    // attributes / predefined items, NOT exported `ObjectModule.bsl`
    // methods. The inline ObjectModule.bsl defines `НеЭкспортный`,
    // but a `*Ref` receiver must not see it (workspace skipped) AND
    // must not surface a workspace-flavour `MethodNotExport` — the
    // diagnostic, if any, comes from the platform `CatalogRef` table
    // (which doesn't have `НеЭкспортный` either, so authoritative
    // miss).
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура НеЭкспортный()
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Сс = Справочники.Справочник1.НайтиПоКоду("001");
    Сс.НеЭкспортный();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let kinds: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, kind, .. }
                if method_name.as_str().eq_ignore_ascii_case("НеЭкспортный") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotFound],
        "*Ref must NOT consult ObjectModule.bsl — diagnostic, if any, is the platform-side \
         MethodNotFound, never a workspace-flavour MethodNotExport; got {kinds:?}",
    );
}

#[test]
fn this_object_non_exported_method_emits_method_not_export() {
    // BSL semantics pinned by the user: `ЭтотОбъект.НеЭкспортный()`
    // inside the same `ObjectModule.bsl` is **not** legal — even
    // though the method lives in the same file, going through the
    // `ЭтотОбъект` reference is an external-style access that
    // requires `Экспорт`. `Ty::ThisObject` is coerced to
    // `Ty::MetadataRef { CatalogObject, .. }` at the workspace
    // branch entry, finds the method without `Экспорт`, and surfaces
    // `MethodNotExport`.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура НеЭкспортный()
КонецПроцедуры
Процедура Тест()
    ЭтотОбъект.НеЭкспортный();
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_with_target_path(fixture, "/Catalogs/Справочник1/Ext/ObjectModule.bsl");
    let kinds: Vec<_> = db
        .infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { method_name, kind, .. }
                if method_name.as_str().eq_ignore_ascii_case("НеЭкспортный") =>
            {
                Some(*kind)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotExport],
        "ЭтотОбъект.НеЭкспортный() is external-style access requiring Экспорт; got {kinds:?}",
    );
}

#[test]
fn this_object_direct_non_exported_call_resolves() {
    // Sanity check / counter-example for the previous test. A
    // *direct* `НеЭкспортный()` call without `ЭтотОбъект.` prefix is
    // a local-scope call (`Expr::Path`, not `Expr::Field`), so it is
    // legal even on a non-exported method in the same module. The
    // workspace ObjectModule branch must NOT misfire here — local
    // calls don't go through the `Expr::Field` callee path.
    let fixture = r#"
//- /Catalogs/Справочник1/Ext/ObjectModule.bsl
Процедура НеЭкспортный()
КонецПроцедуры
Процедура Тест()
    НеЭкспортный();
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_with_target_path(fixture, "/Catalogs/Справочник1/Ext/ObjectModule.bsl");
    assert!(
        !unresolved_method_names(&db, file_id)
            .iter()
            .any(|n| n.eq_ignore_ascii_case("НеЭкспортный")),
        "direct `НеЭкспортный()` is a local-scope call, must resolve cleanly; got {:?}",
        unresolved_method_names(&db, file_id),
    );
}
