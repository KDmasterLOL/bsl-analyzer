//! IDE-surface invariants for projection-typed selections.
//!
//! Pins the three Phase E deliverables that turn the refined SDBL
//! projection into user-visible IDE output:
//!
//! - `enumerate_fields` surfaces the projection columns as
//!   `FieldInfo` entries so completion popups show them.
//! - `hir::Type::projection_fields` exposes the `(Name, Ty)` slice
//!   without forcing IDE consumers to import `hir_def::ty`.
//! - Hover on a projection-typed selection appends a `**Поля:**`
//!   block with SDBL display labels (precision / scale / length).

use expect_test::{expect, Expect};
use hir::{DefDatabase, HirDatabase, ModuleId, Ty, Type};
use ide::{Analysis, CompletionItem};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

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
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
}

#[test]
fn projection_fields_visible_via_hir_type_accessors() {
    // `Выборка` is the canonical projection-typed receiver. The
    // facade accessors must surface the SELECT alias column without
    // forcing the caller to pattern-match `Ty::QueryResultSelection`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать();
    Возврат Выборка;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "выборка").expect("выборка must be inferred");
    let type_facade = Type::from_id(&db, file_id, hir::ty_bridge::ty_to_typeid(&db, &ty));
    assert!(type_facade.is_query_projection(), "is_query_projection must return true");
    let fields =
        type_facade.projection_fields().expect("projection_fields must surface the column slice");
    assert_eq!(fields.len(), 1, "single-column SELECT yields one projection field");
    assert_eq!(fields[0].0.as_str(), "Имя");
    assert_eq!(hir::ty_bridge::typeid_to_ty(&db, fields[0].1), Ty::String);
}

#[test]
fn projection_fields_surface_in_enumerate_fields() {
    // `hir::Type::fields()` is what completion / hover walk to list
    // a receiver's accessible fields. The Phase E projection arm
    // makes the SDBL column visible there in addition to (not
    // instead of) the platform `ВыборкаИзРезультатаЗапроса`
    // properties — but the projection itself is what we pin here.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя, 42 КАК Цена").Выполнить().Выбрать();
    Возврат Выборка;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "выборка").expect("выборка must be inferred");
    let type_facade = Type::from_id(&db, file_id, hir::ty_bridge::ty_to_typeid(&db, &ty));
    let fields = type_facade.fields();
    let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
    assert!(
        names.contains(&"Имя"),
        "enumerate_fields must include projection column `Имя`, got {names:?}",
    );
    assert!(
        names.contains(&"Цена"),
        "enumerate_fields must include projection column `Цена`, got {names:?}",
    );
    // Projection columns are read-only — `Выборка.Имя = "..."` is a
    // runtime error.
    let projection_columns: Vec<_> =
        fields.iter().filter(|f| matches!(f.name.as_str(), "Имя" | "Цена")).collect();
    for col in &projection_columns {
        assert!(col.is_readonly, "projection column `{}` must be read-only", col.name.as_str());
    }
}

fn hover_baseline_setup(fixture_text: &str) -> (Analysis, FileId, u32) {
    let abs_idx = fixture_text.find("$0").expect("fixture must contain $0 cursor marker");
    let prefix = &fixture_text[..abs_idx];
    let last_header_start = prefix.rfind("//- ").expect("cursor must be inside a //- file");
    let header_end =
        prefix[last_header_start..].find('\n').expect("//- header must end with newline")
            + last_header_start;
    let path_line = &prefix[last_header_start + 4..header_end];
    let file_offset_in_prefix = header_end + 1;
    let cursor_in_file = (abs_idx - file_offset_in_prefix) as u32;
    let cleaned = fixture_text.replacen("$0", "", 1);

    let fixture = Fixture::parse(&cleaned);
    let mut db = RootDatabaseImpl::new();
    let source_root_id = SourceRootId(0);
    let mut file_set = FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(source_root_id, SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, source_root_id);
        db.set_file_text(*file_id, &file.content);
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with(path_line))
        .map(|(id, _)| *id)
        .expect("cursor-bearing file not found");
    (Analysis::from_database(db), test_file, cursor_in_file)
}

fn check_hover_contains(fixture: &str, expected_substring: Expect) {
    let (analysis, file_id, offset) = hover_baseline_setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover must produce a result for projection-typed receivers");
    expected_substring.assert_eq(extract_fields_line(&hover.markup).as_str());
}

/// Extract the `**Поля:** ...` line from the hover markup; returns
/// the empty string when absent so the snapshot can lock either
/// presence or absence without depending on the surrounding platform
/// docs (which evolve with `platform_data.json` regenerations).
fn extract_fields_line(markup: &str) -> String {
    markup
        .lines()
        .find(|line| line.starts_with("**Поля:**"))
        .map(|line| line.to_string())
        .unwrap_or_default()
}

fn complete(fixture: &str) -> Vec<CompletionItem> {
    let (analysis, file_id, offset) = hover_baseline_setup(fixture);
    analysis.completions(file_id, offset, None, ide::Locale::Ru)
}

#[test]
fn completion_on_projection_selection_lists_columns_and_platform_members() {
    // Dot-completion on a projection-typed selection must surface
    // both the SDBL column aliases (`Имя`, `Цена`) AND the platform
    // `ВыборкаИзРезультатаЗапроса` members (`Получить`,
    // `Следующий`, `Сбросить`, …). Phase E hooks the projection
    // columns into the same `platform_type_name()` branch that
    // already returns platform methods/properties, so the popup
    // shows the union.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя, 42 КАК Цена").Выполнить().Выбрать();
    Возврат Выборка.$0;
КонецФункции
"#,
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"Имя"),
        "projection column `Имя` must appear in completion, got {labels:?}",
    );
    assert!(
        labels.contains(&"Цена"),
        "projection column `Цена` must appear in completion, got {labels:?}",
    );
    assert!(
        labels.contains(&"Следующий"),
        "platform method `Следующий` must still appear in completion, got {labels:?}",
    );
}

#[test]
fn completion_on_inline_query_union_receiver_lists_projection_columns() {
    // Phase 3 §4.D.4b regression: `Выполнить()` returns a union that
    // includes `Undefined`; after the walk moved to kernel TypeId,
    // union canonicalisation happens immediately. The following
    // `.Выбрать().<dot>` access is in the same expression, so it
    // observes that canonicalized receiver directly. Completion must
    // still see the concrete query-result arm and list projection
    // fields plus platform members.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Возврат Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать().$0;
КонецФункции
"#,
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"Имя"),
        "inline union receiver must preserve projection column completion, got {labels:?}",
    );
    assert!(
        labels.contains(&"Следующий"),
        "inline union receiver must preserve selection platform methods, got {labels:?}",
    );
}

#[test]
fn hover_on_inline_query_union_receiver_field_renders_type() {
    // Same timing hazard as the completion test, but through field
    // lookup: `Выполнить()` produces a union receiver, `.Выбрать()`
    // resolves against the query-result arm, and `.Имя` must still
    // resolve in the same body without relying on a later output
    // bridge to defer union canonicalisation.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Возврат Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать().Им$0я;
КонецФункции
"#;
    let (analysis, file_id, offset) = hover_baseline_setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover must resolve the inline projection field");
    assert!(
        hover.markup.contains("Строка") || hover.markup.contains("String"),
        "hover on inline union receiver field must render string type, got: {}",
        hover.markup,
    );
}

#[test]
fn hover_on_projection_none_omits_fields_block() {
    // Pin the negative: when refinement fails (no projection
    // captured at the constructor, no successful Phase D walk), the
    // hover output must NOT carry the `**Поля:**` line. Otherwise
    // the user sees an empty / misleading "fields list" on a
    // selection whose columns are unknowable from static analysis.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Текст = ПолучитьТекстЗапроса();
    Выборка = Новый Запрос(Текст).Выполнить().Выбрать();
    Возврат Выб$0орка;
КонецФункции
"#;
    let (analysis, file_id, offset) = hover_baseline_setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover must produce a result for projection-less selections");
    assert!(
        !hover.markup.contains("**Поля:**"),
        "projection-less selection must not surface a fields block — got: {markup}",
        markup = hover.markup,
    );
}

#[test]
fn hover_on_projection_selection_lists_field_names() {
    // Hover on a `Выборка` USE (the `Возврат Выборка;` reference)
    // shows the platform's `ВыборкаИзРезультатаЗапроса` docs PLUS
    // the projection columns appended as a `**Поля:**` line. The
    // hover target is the path-expression on the RHS so the receiver
    // Ty is `Ty::QueryResultSelection { projection: Some(_) }` —
    // hovering the assignment LHS would route through a different
    // dispatch (binding-definition hover) that doesn't reach
    // `ty_info_markup`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя").Выполнить().Выбрать();
    Возврат Выб$0орка;
КонецФункции
"#;
    check_hover_contains(fixture, expect!["**Поля:** Имя: Строка"]);
}

#[test]
fn hover_on_projection_selection_renders_cast_precision_and_scale() {
    // Phase G end-to-end: `ВЫРАЗИТЬ(0 КАК Число(15, 2))` lowers to
    // `SdblType::Number { Some(15), Some(2) }` inside the SDBL HIR; the
    // bridge surfaces it as `SdblTypeShadow { display: "Число(15, 2)" }`;
    // hover concatenates the column into the `**Поля:**` block with the
    // precision-bearing display verbatim. Before Phase G the CAST fell
    // through to `SdblType::Unknown` so the projection field was either
    // absent or rendered as bare `Неизвестно`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ВЫРАЗИТЬ(0 КАК Число(15, 2)) КАК Цена").Выполнить().Выбрать();
    Возврат Выб$0орка;
КонецФункции
"#;
    check_hover_contains(fixture, expect!["**Поля:** Цена: Число(15, 2)"]);
}

#[test]
fn hover_on_projection_selection_renders_cast_string_length() {
    // Length-only CAST surfaces `Строка(50)` (Phase E Display already
    // covered the String shape; Phase G adds the lowering path that
    // makes the precise type reach the bridge in the first place).
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ВЫРАЗИТЬ("""" КАК Строка(50)) КАК Имя").Выполнить().Выбрать();
    Возврат Выб$0орка;
КонецФункции
"#;
    check_hover_contains(fixture, expect!["**Поля:** Имя: Строка(50)"]);
}

#[test]
fn hover_on_iteration_row_from_batched_helper_renders_cast_precision() {
    // Cross-phase pin: batched SDBL package (Phase D + Phase B "pick
    // last query" semantics) carrying a `ВЫРАЗИТЬ(0 КАК Число(15, 2))`
    // projection (Phase G) flows through a helper return (Phase F /
    // Phase J cascade), then through `Для Каждого … Из ТЗ` (Phase H
    // iteration short-circuit). Hovering an iteration-row receiver
    // must surface `Цена: Число(15, 2)` in the `**Поля:**` block —
    // exercising the entire pipeline end-to-end.
    let fixture = r#"//- /test.bsl
Функция ПолучитьТЗ() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ 1 КАК X ПОМЕСТИТЬ ВТ; ВЫБРАТЬ ВЫРАЗИТЬ(0 КАК Число(15, 2)) КАК Цена ИЗ ВТ КАК ВТ";
    Возврат Зап.Выполнить().Выгрузить();
КонецФункции

Функция Тест()
    Для Каждого Стр Из ПолучитьТЗ() Цикл
        Возврат Ст$0р;
    КонецЦикла;
КонецФункции
"#;
    check_hover_contains(fixture, expect!["**Поля:** Цена: Число(15, 2)"]);
}

#[test]
fn hover_on_projection_selection_renders_cast_precision_only_number() {
    // Phase G Slice 2 — precision-only Number gets the `(P)` suffix in
    // Display, so the hover output renders `Число(15)` instead of
    // collapsing to a bare `Число`.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Выборка = Новый Запрос("ВЫБРАТЬ ВЫРАЗИТЬ(0 КАК Число(15)) КАК Сумма").Выполнить().Выбрать();
    Возврат Выб$0орка;
КонецФункции
"#;
    check_hover_contains(fixture, expect!["**Поля:** Сумма: Число(15)"]);
}
