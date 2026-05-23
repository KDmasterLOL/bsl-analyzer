//! Cross-procedure SDBL projection propagation.
//!
//! Pins the end-to-end invariant that a helper which builds and returns
//! a query
//!
//! ```bsl
//! Функция СоздатьЗапрос()
//!     Зап = Новый Запрос;
//!     Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
//!     Возврат Зап;
//! КонецФункции
//!
//! Результат = СоздатьЗапрос().Выполнить().Выбрать().Имя
//! ```
//!
//! propagates its refined projection through `method_return_type_query`
//! so the trailing `.Имя` on the caller side still types as
//! `Ty::String`. The helper body lives in a different `BodyInferenceResult`
//! than the caller, so the projection must survive the cross-method
//! Salsa boundary (Phase B synthesis + Phase D variable-state refinement
//! + Phase J method-graph cascade).

use hir::{DefDatabase, HirDatabase, ModuleId, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

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
fn helper_returning_refined_query_propagates_projection_to_caller() {
    // Phase F closes this gap: `infer_path_name` upgrades a
    // projection-less `Ty::Query{[None]}` (or legacy
    // `PlatformObject("Запрос")`) binding by running the same
    // reaching-defs walk Phase D applies at chain dispatch. The
    // helper's `Возврат Зап;` now resolves to `Ty::Query{[Some(p)]}`,
    // `method_return_type_query` caches it, and the caller's chain
    // surfaces `.Имя` as `Ty::String`.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
    Возврат Зап;
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "Phase F target: dataflow-refined query projection propagates through helper return",
    );
}

#[test]
fn helper_with_constructor_literal_propagates_projection() {
    // Companion to the variable-refinement test above — the helper
    // produces the projection at constructor time (Phase B), no
    // Phase D walk needed. Pins that Phase B synthesis survives the
    // same cross-method boundary.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Возврат Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя");
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "constructor-time projection must propagate through helper's return type",
    );
}

#[test]
fn divergent_text_writes_collapse_to_unknown() {
    // Phase F preserves Phase D's all-or-nothing rule: when reaching
    // defs disagree on the literal SDBL text, the projection collapses
    // and the caller's `.Имя` types as `Unknown`. Without this, two
    // divergent SELECTs would silently pick one — worse than no
    // refinement.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос(Флаг) Экспорт
    Зап = Новый Запрос;
    Если Флаг Тогда
        Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
    Иначе
        Зап.Текст = "ВЫБРАТЬ ""def"" КАК Цена";
    КонецЕсли;
    Возврат Зап;
КонецФункции

Функция Тест(Флаг)
    Х = СоздатьЗапрос(Флаг).Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "divergent reaching writes must collapse the chain so `Х` stays unrefined — \
         conservative var_types drops Unknown RHS, leaving the binding absent",
    );
}

#[test]
fn dynamic_text_write_collapses_to_unknown() {
    // RHS is a function call, not a string literal — the dataflow
    // walk rejects it (`projection_from_text_assignment` requires
    // `Expr::Literal(String)`). Caller surfaces Unknown.
    let fixture = r#"//- /test.bsl
Функция ПолучитьТекст()
    Возврат "ВЫБРАТЬ ""abc"" КАК Имя";
КонецФункции

Функция СоздатьЗапрос() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = ПолучитьТекст();
    Возврат Зап;
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "dynamic text RHS collapses the projection and the trailing `.Имя` resolves \
         to Unknown — conservative var_types drops Unknown so `Х` stays absent",
    );
}

#[test]
fn no_text_write_keeps_projection_none() {
    // Bare `Зап = Новый Запрос;` with no `.Текст` write — reaching
    // defs find nothing for `зап.текст`, refinement returns None, and
    // the caller's chain produces `Ty::Unknown` on the trailing
    // `.Имя`. Pins that Phase F doesn't fabricate a projection out of
    // an unrefined binding.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Зап = Новый Запрос;
    Возврат Зап;
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "no `.Текст` write keeps the projection unresolved and the trailing `.Имя` \
         types as Unknown — conservative var_types drops the Unknown so `Х` is absent",
    );
}

// ---------------------------------------------------------------------------
// Phase F follow-ups — pinned as `#[ignore]` so the fixtures exist and
// can be flipped to active tests when the underlying support lands. Each
// case names the missing primitive in the ignore reason. Per Codex
// round-1 review of the Phase F plan (REVISE on area E): keep fixtures
// in-repo to prevent silent regression.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Phase F follow-up: loop-carried `.Текст += ...` append idiom needs \
            string-concatenation reasoning in projection_from_text_assignment; \
            today the +=-style write fails the `Expr::Literal(String)` gate and \
            collapses to None (acceptable, but no caller-side wire-through yet)."]
fn loop_carried_text_append_recovers_to_projection() {
    // Iterative builder idiom — projection only knowable if the
    // dataflow walk concatenates literal fragments across the loop
    // back-edge. Out of Phase F scope; Phase D's append-rejection
    // policy applies.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос(Условия) Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя ИЗ Справочник.Товары ГДЕ ";
    Для Каждого У Из Условия Цикл
        Зап.Текст = Зап.Текст + " И " + У;
    КонецЦикла;
    Возврат Зап;
КонецФункции

Функция Тест(Условия)
    Х = СоздатьЗапрос(Условия).Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "Phase F follow-up target: loop-append builder recovers projection",
    );
}

#[test]
#[ignore = "Phase F follow-up: parameter-as-Query has no module-local reaching \
            def for the param binding's `.Текст`, so refinement returns None. \
            Closing this gap needs callee/caller cross-method dataflow."]
fn parameter_query_with_text_write_in_caller_propagates() {
    // The helper receives a `Запрос` argument and the caller assigns
    // its text before passing it in. Phase F's module-local
    // reaching-defs don't cross the parameter boundary. Cross-method
    // dataflow would be a separate phase.
    let fixture = r#"//- /test.bsl
Функция Выполнить(Зап) Экспорт
    Возврат Зап.Выполнить().Выбрать().Имя;
КонецФункции

Функция Тест()
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
    Х = Выполнить(Зап);
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "Phase F follow-up target: param-as-Query refinement across call boundary",
    );
}

#[test]
#[ignore = "Phase F follow-up: a binding name that collides with a CFE-visible \
            symbol needs scope resolution before refinement; today the gate \
            only checks var_types and may miss shadowing. Out of Phase F scope."]
fn cfe_shadowed_binding_refines_through_local_only() {
    // `Запрос` is also a platform constructor name; the local
    // `Запрос = Новый Запрос;` shadows it. Phase F currently relies
    // on var_types having already captured the local — but a CFE
    // extension that publishes a `Запрос` global could intervene.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
    Возврат Запрос;
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "Phase F follow-up: refinement under name shadowing CFE-visible globals",
    );
}

// ---------------------------------------------------------------------------
// Batched-package support (Phase D follow-up shipped 2026-05-23 alongside
// Phase F). 1С `Запрос.Выполнить()` returns the result of the *last* query
// in a batched SDBL package; `ПОМЕСТИТЬ`/staging SELECTs produce no rows.
// The refinement and chain-rewrite layers now agree on that semantics.
// ---------------------------------------------------------------------------

#[test]
fn batched_text_assignment_picks_last_query_projection() {
    // ERP-style: a temp-table `ПОМЕСТИТЬ` stages a sub-query, then a
    // final SELECT joins against it. Runtime returns only the final
    // SELECT's rows. The dataflow walk produces a per-sub-query
    // projection vector; `.Выполнить()` reads the last entry; the
    // trailing `.Выгрузить()` carries the projection into the
    // returned ValueTable and the caller's iteration row sees the
    // final query's fields.
    let fixture = r#"//- /test.bsl
Функция ПолучитьТЗ() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ 1 КАК Игнор ПОМЕСТИТЬ ВТ; ВЫБРАТЬ ""abc"" КАК Имя ИЗ ВТ КАК ВТ";
    Возврат Зап.Выполнить().Выгрузить();
КонецФункции

Функция Тест()
    Для Каждого Стр Из ПолучитьТЗ() Цикл
        Х = Стр.Имя;
        Возврат Х;
    КонецЦикла;
    Возврат Неопределено;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "batched query: last-SELECT's `КАК Имя` projection must reach the iteration row",
    );
}

#[test]
fn batched_text_assignment_drops_staging_columns() {
    // Negative: a column that exists only in a `ПОМЕСТИТЬ` staging
    // sub-query must NOT leak into the final selection's projection.
    // Reading it on the iteration row types as Unknown (conservative
    // var_types track drops Unknown, so the binding stays absent).
    let fixture = r#"//- /test.bsl
Функция ПолучитьТЗ() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ 1 КАК Игнор ПОМЕСТИТЬ ВТ; ВЫБРАТЬ ""abc"" КАК Имя ИЗ ВТ КАК ВТ";
    Возврат Зап.Выполнить().Выгрузить();
КонецФункции

Функция Тест()
    Для Каждого Стр Из ПолучитьТЗ() Цикл
        Х = Стр.Игнор;
        Возврат Х;
    КонецЦикла;
    Возврат Неопределено;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "staging-only column `Игнор` must not leak into the final SELECT's projection",
    );
}
