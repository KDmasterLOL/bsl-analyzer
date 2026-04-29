//! End-to-end regression tests for `MismatchedArgCount` on platform
//! constructor calls (`Новый <Type>(args)`).
//!
//! Pins the PR3 contract: `Expr::New` arity-checks against
//! [`bsl_platform::PlatformDataInner::get_constructors`] using the
//! same `signature_from_params` adapter and multi-overload "accept-if-
//! any" loop that PR1/PR2 already used for global functions. Variadic
//! tails are honoured through three sources, all transparent to the
//! caller:
//!
//! - **Syntax-line detector (PR2):** HBK `Синтаксис:` containing the
//!   `<X1>,...,<XN>` shape — `Array.По количеству элементов`,
//!   `COMSafeArray` ×2.
//! - **Docs-only overlay (PR3 Step 2):** HBK `Описание:` declares
//!   variadic but `Синтаксис:` is fixed — `Structure`, `FixedStructure`,
//!   `DynamicListRowKey` (each on the «По ключам и значениям» / «На
//!   основе путей и значений полей» variant).
//! - **Name idiom (PR3 Step 1):** rubric param NAME contains
//!   `<word>N,...,<word>N` with a letter suffix — `FormattedString.На
//!   основании строк`.

use hir::{HirDatabase, InferenceDiagnostic};
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
        db.set_file_text(*file_id, &file.content);
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn arg_count_diags(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(usize, usize, usize)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::MismatchedArgCount {
                required_count, total_count, found, ..
            } => Some((*required_count, *total_count, *found)),
            _ => None,
        })
        .collect()
}

#[test]
fn array_variadic_accepts_many_args() {
    // `Array.По количеству элементов` is is_variadic=true (PR2,
    // syntax-line detector). Multi-arg `Новый Массив(...)` must NOT
    // fire — variadic upper bound is None.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый Массив(10, 20, 30, 40, 50);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Новый Массив with variadic count list must accept any arity, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn array_one_arg_accepts() {
    // 1-arg form selects `Array.На основании фиксированного массива`
    // (single optional `Массив` param, max=1). Locks that the variadic
    // overload doesn't shadow the fixed one.
    let fixture = r#"
//- /test.bsl
Процедура Тест(ИсхМассив)
    Х = Новый Массив(ИсхМассив);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Новый Массив(x) must accept (1-arg fixed overload), got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn array_no_parens_accepts() {
    // `Новый Массив` (no parens) lowers to args=[]. Both Array
    // overloads are all-optional → required_count=0 for each → accept.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый Массив;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Новый Массив (no parens) must accept (zero-arity overload), got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn structure_keys_and_values_accepts_variadic() {
    // `Structure.По ключам и значениям` declares fixed [Ключи, Значения]
    // in HBK syntax, but the description allows additional values matching
    // keys. PR3 Step 2 overlay lifts is_variadic=true on the trailing
    // `Значения` param so this verbatim user-style call accepts.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Отбор = Новый Структура("Дата, Клиент", ТекущаяДата(), "Иванов");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Новый Структура('К1, К2', З1, З2) must accept via PR3 overlay, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn structure_no_parens_accepts() {
    // `Новый Структура` matches the `На основании фиксированной
    // структуры` overload (single optional param) at arity 0.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый Структура;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "Новый Структура (no parens) must accept, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn fixed_structure_keys_and_values_accepts_variadic() {
    // `FixedStructure.По ключам и значениям`: PR3 overlay marks `Значения`
    // as variadic. With keys + multiple values, the call accepts.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый ФиксированнаяСтруктура("К1, К2", "Знач1", "Знач2");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "ФиксированнаяСтруктура with keys+values must accept via PR3 overlay, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn dynamic_list_row_key_accepts_variadic() {
    // `DynamicListRowKey.На основе путей и значений полей`: PR3
    // overlay marks the trailing `Значения` as variadic. Description
    // says additional values follow paths in declaration order.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Ключ = Новый КлючСтрокиДинамическогоСписка("Поле1, Поле2", 1, "два");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "КлючСтрокиДинамическогоСписка with paths+values must accept, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn formatted_string_strings_accepts_variadic() {
    // `FormattedString.На основании строк` encodes its variadic shape
    // INSIDE one rubric name (`Содержимое1,...,СодержимоеN`). PR3
    // Step 1 (`name_implies_unbounded_variadic`) lifts max_args=None
    // for that overload.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Стр = Новый ФорматированнаяСтрока("a", "b", "c", "d");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "ФорматированнаяСтрока with multiple strings must accept via PR3 Step 1, got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn fixed_array_required_floor_fires() {
    // `FixedArray` has a single ctor `На основании обычного массива`
    // with one REQUIRED `Массив` param and NO zero-arity overload.
    // `Новый ФиксированныйМассив()` violates the required floor →
    // exactly one MismatchedArgCount(required=1, total=1, found=0).
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый ФиксированныйМассив();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let diags = arg_count_diags(&db, file_id);
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    assert_eq!(diags[0].2, 0, "found = 0");
    assert_eq!(diags[0].0, 1, "required = 1 (Массив is mandatory)");
}

#[test]
fn query_upper_bound_fires() {
    // `Query` has a single ctor with one OPTIONAL `ТекстЗапроса` param
    // (max=1, no variadic). `Новый Запрос("a", "b", "c")` exceeds the
    // upper bound → exactly one MismatchedArgCount.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый Запрос("текст", "extra1", "extra2");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let diags = arg_count_diags(&db, file_id);
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic, got {diags:?}");
    assert_eq!(diags[0].2, 3, "found = 3");
    assert_eq!(diags[0].1, 1, "total = 1 (single param)");
}

#[test]
fn unresolved_type_does_not_fire_arity() {
    // No `PlatformConstructor` registered for unknown types. The arity
    // check must SKIP (return early on empty `get_constructors`),
    // avoiding double-firing on top of any upstream "unresolved type"
    // diagnostic. Pin: zero MismatchedArgCount for an unknown type
    // even with arbitrary args.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Х = Новый НесуществующийТип(1, 2, 3, "anything");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "unknown ctor type must NOT fire MismatchedArgCount (skipped on empty ctor list), got {:?}",
        arg_count_diags(&db, file_id)
    );
}

#[test]
fn user_repro_structure_filter() {
    // The kind of call the user actually writes — a filter Structure
    // built from one keys-string and a matching number of values.
    // This is the constructor analogue of the PR2 `Мин(Макс(...))`
    // user-repro test in `infer_builtin_arity.rs`.
    let fixture = r#"
//- /test.bsl
Процедура ПрименитьОтбор(Дата, Клиент, Сумма)
    Отбор = Новый Структура("Дата, Клиент, Сумма", Дата, Клиент, Сумма);
    Сообщить(Отбор);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        arg_count_diags(&db, file_id).is_empty(),
        "user-style 4-arg Структура call must produce zero diagnostics, got {:?}",
        arg_count_diags(&db, file_id)
    );
}
