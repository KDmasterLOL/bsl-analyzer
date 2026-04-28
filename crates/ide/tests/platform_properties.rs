//! End-to-end coverage for platform-property inference, completion, and
//! the `ReadOnlyPropertyAssignment` diagnostic.
//!
//! Pins the `Зап = Новый Запрос; Зап.Параметры.Вставить(...)` scenario from
//! the user's nvim session through four layers:
//!
//! 1. `field_lookup::lookup_field` resolves `Зап.Параметры` via
//!    `platform_property_lookup` (receiver is `Ty::PlatformObject("Запрос")`).
//! 2. The chained `.Вставить` call resolves because `Параметры` returns
//!    `Ty::Structure` and `method_lookup` knows `Structure.Вставить`.
//! 3. Completion on `Зап.` lists both methods and properties with the
//!    right `CompletionItemKind`.
//! 4. `Stmt::Assign` emits `ReadOnlyPropertyAssignment` when the LHS is
//!    a read-only platform property.
//!
//! Fixtures are minimal (no designer workspace needed): the platform
//! catalogue is loaded from `bsl-platform/data/platform_data.json` at
//! crate init, so property data is always available even without
//! metadata-object configs.

use hir::{HirDatabase, InferenceDiagnostic, Name, Ty};
use ide::{Analysis, CompletionItem, CompletionItemKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use vfs::{FileId, FileSet, VfsPath};

fn setup_inline(code: &str) -> (RootDatabaseImpl, FileId) {
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::default();
    file_set.insert(file_id, VfsPath::new("/test.bsl"));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, code);
    (db, file_id)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

fn readonly_diagnostics(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(Name, Ty)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::ReadOnlyPropertyAssignment { receiver_ty, field_name, .. } => {
                Some((field_name.clone(), receiver_ty.clone()))
            }
            _ => None,
        })
        .collect()
}

fn completions_at(code: &str) -> Vec<CompletionItem> {
    let cursor = code.find("$0").expect("fixture must mark cursor with $0");
    let without_cursor: String = format!("{}{}", &code[..cursor], &code[cursor + 2..]);
    let (db, file_id) = setup_inline(&without_cursor);
    Analysis::from_database(db).completions(file_id, cursor as u32, None)
}

#[test]
fn new_query_text_field_infers_to_string() {
    // `Зап = Новый Запрос; Т = Зап.Текст;` — `Запрос.Текст` is declared
    // as `Строка` in `platform_data.json`. The field-lookup adapter for
    // `Ty::PlatformObject("Запрос")` must route through
    // `platform_property_lookup` and propagate `Ty::String` to the
    // enclosing assignment.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Т = Зап.Текст;
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    assert_eq!(
        var_ty(&db, file_id, "т"),
        Some(Ty::String),
        "Т must carry Ty::String via property inference, not Ty::Unknown"
    );
}

#[test]
fn new_query_parameters_field_infers_to_structure() {
    // The headline scenario — `Запрос.Параметры` returns `Структура` per
    // HBK. Must flow end-to-end through the property lookup so the
    // chained `.Вставить` can later resolve via Structure.Вставить.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    П = Зап.Параметры;
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    assert_eq!(
        var_ty(&db, file_id, "п"),
        Some(Ty::Structure),
        "Зап.Параметры must carry Ty::Structure via property inference"
    );
}

#[test]
fn chained_property_method_resolves() {
    // Full user scenario: `Зап.Параметры.Вставить("K", Значение)`. The
    // chained method call must resolve — proves that
    // `Зап.Параметры → Ty::Structure` flows into `method_lookup`, which
    // then finds `Structure.Вставить` in the platform method table.
    // A regression here turns `Вставить` into `UnresolvedMethodCall`.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Параметры.Вставить(\"Ключ\", 1);
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    let unresolved: Vec<String> = db
        .infer(file_id)
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
        !unresolved.iter().any(|n| n.eq_ignore_ascii_case("Вставить")),
        "chained Зап.Параметры.Вставить must resolve, got UnresolvedMethodCall for: {unresolved:?}",
    );
}

#[test]
fn read_only_property_assignment_emits_diagnostic() {
    // `Запрос.Параметры` is read-only — assigning to it must trigger the
    // `ReadOnlyPropertyAssignment` diagnostic, carrying the property
    // name and receiver type in the payload.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Параметры = Новый Структура;
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    let diags = readonly_diagnostics(&db, file_id);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one ReadOnlyPropertyAssignment diagnostic, got: {diags:?}"
    );
    let (field, recv) = &diags[0];
    assert_eq!(field.as_str(), "Параметры");
    assert!(
        matches!(recv, Ty::PlatformObject(n) if n.as_str().eq_ignore_ascii_case("Запрос")),
        "receiver_ty must be Ty::PlatformObject(\"Запрос\"), got {recv:?}"
    );
}

#[test]
fn writable_property_assignment_no_diagnostic() {
    // `Запрос.Текст` is read-write — assignment must stay silent. Guards
    // against over-triggering on every property assignment.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    assert!(
        readonly_diagnostics(&db, file_id).is_empty(),
        "writable property must not fire ReadOnlyPropertyAssignment"
    );
}

#[test]
fn completion_on_new_query_lists_text_and_parameters() {
    // After the dot on a `Новый Запрос` variable the completion list
    // must carry both methods and properties. We check the headline
    // property names (Текст, Параметры) and at least one canonical
    // method (Выполнить) to prove the dispatcher is mixing the two
    // queries.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.$0
КонецПроцедуры
";
    let items = completions_at(code);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(
        labels.contains(&"Текст"),
        "completion must list property Текст, got labels: {labels:?}"
    );
    assert!(
        labels.contains(&"Параметры"),
        "completion must list property Параметры, got labels: {labels:?}"
    );
    assert!(
        labels.contains(&"Выполнить"),
        "completion must keep method Выполнить alongside properties, got labels: {labels:?}"
    );

    // Property items must be classified as `Property`, not `Method`.
    let params = items.iter().find(|i| i.label == "Параметры").expect("Параметры item");
    assert_eq!(
        params.kind,
        CompletionItemKind::Property,
        "Параметры must surface as Property, not {:?}",
        params.kind
    );
    // Read-only properties annotate their detail with the marker.
    assert!(
        params.detail.as_deref().unwrap_or_default().contains("Только чтение"),
        "read-only property detail must include [Только чтение], got: {:?}",
        params.detail,
    );

    // Properties insert the bare name — no parens, no snippet cursor.
    assert_eq!(params.insert_text, "Параметры");
}

#[test]
fn hover_on_query_parameters_renders_readonly_structure_block() {
    // Hover on the property identifier `Параметры` inside
    // `Зап.Параметры` must surface the property card with:
    //   - bilingual title (`Параметры (Parameters)`),
    //   - `Только чтение` marker,
    //   - `Тип: Структура` line.
    // Regression guard: before the dedicated property-hover branch
    // landed, this hover returned `None` because
    // `hover_for_platform_method` treats `Зап` as a literal type.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    П = Зап.Параметры;
КонецПроцедуры
";
    let cursor = code.find("Параметры").expect("fixture must contain Параметры");
    let (db, file_id) = setup_inline(code);
    // Place the hover cursor on the first character of `Параметры`.
    let hover = Analysis::from_database(db)
        .hover(file_id, cursor as u32)
        .expect("hover must return a result for Зап.Параметры");
    let markup = &hover.markup;
    assert!(
        markup.contains("Параметры") && markup.contains("Parameters"),
        "hover markup must include bilingual name, got: {markup}"
    );
    assert!(
        markup.contains("Только чтение"),
        "hover markup must mark Параметры read-only, got: {markup}"
    );
    assert!(
        markup.contains("Структура"),
        "hover markup must include value type Структура, got: {markup}"
    );
}

#[test]
fn query_execute_unload_chain_infers_value_table() {
    // Regression for the user's chain
    //   `Таблица = Запрос.Выполнить().Выгрузить();`
    // Three layers must line up:
    //   1. Platform data says `Запрос.Выполнить` returns the comma-joined
    //      string `"РезультатЗапроса, Неопределено"`.
    //   2. `to_method_info` splits it into `Ty::Union([QueryResult,
    //      Undefined])`.
    //   3. `lookup_method` strips `Undefined` from the union receiver and
    //      dispatches `.Выгрузить` on `QueryResult` → `Ty::ValueTable`.
    // Before the fix step (2) produced
    // `Ty::PlatformObject("РезультатЗапроса, Неопределено")`, so step (3)
    // missed the bilingual index entirely and `Таблица` inferred as
    // `Ty::Unknown`.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
    Таблица = Зап.Выполнить().Выгрузить();
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    // HBK declares `Выгрузить()` as `"ТаблицаЗначений, ДеревоЗначений"`,
    // so `Таблица` arrives as a union. The contract for this regression
    // is "contains Ty::ValueTable so chained members resolve" — not a
    // strict `==` on ValueTable. A pre-fix run returned `Ty::Unknown`
    // (union receiver got lost) or a poisoned PlatformObject, both of
    // which would flunk the `contains_value_table` check below.
    let ty = var_ty(&db, file_id, "таблица");
    let contains_value_table = match &ty {
        Some(Ty::ValueTable) => true,
        Some(Ty::Union(members)) => members.iter().any(|m| matches!(m, Ty::ValueTable)),
        _ => false,
    };
    assert!(
        contains_value_table,
        "Запрос.Выполнить().Выгрузить() must carry Ty::ValueTable (optionally in a union) — got {ty:?}",
    );
}

#[test]
fn completion_after_query_execute_unload_lists_value_table_members() {
    // Same chain as above, but checked via the completion pipeline —
    // proves that `complete_platform_members` sees `Ty::ValueTable` on
    // the receiver and surfaces the standard members (`Добавить`,
    // `Колонки`, `НайтиСтроки`, …). A regression here means the user's
    // `.Выгрузить().` completion stays empty even though inference is
    // fixed.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Выполнить().Выгрузить().$0
КонецПроцедуры
";
    let items = completions_at(code);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for expected in ["Добавить", "Колонки", "Количество"] {
        assert!(
            labels.contains(&expected),
            "completion on ValueTable must include {expected}, got: {labels:?}",
        );
    }
}

#[test]
fn completion_on_query_parameters_lists_structure_methods() {
    // Chained completion: `Зап.Параметры.|` — `Параметры` is
    // `Ty::Structure`, so the list should include `Вставить`, `Удалить`,
    // `Свойство`, etc. from the `Structure` method table. A regression
    // in `field_lookup` → property propagation would collapse the
    // receiver to `Ty::Unknown` and return nothing.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Параметры.$0
КонецПроцедуры
";
    let items = completions_at(code);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    for expected in ["Вставить", "Количество", "Очистить"] {
        assert!(
            labels.contains(&expected),
            "completion on Зап.Параметры must include {expected}, got: {labels:?}"
        );
    }
}

#[test]
fn hover_on_chained_method_resolves_platform_method() {
    // Regression for the user-reported hover bug:
    //   `Запрос.Выполнить().Выгрузить().ВыгрузитьКолонку("Ссылка")`
    // — the field-name `ВыгрузитьКолонку` previously fell through
    // `Semantics::resolve_name_to_definition` into module-method
    // resolution and matched a workspace free function (e.g. БСП's
    // `ОбщегоНазначения.ВыгрузитьКолонку`), so hover showed
    // `Функция ВыгрузитьКолонку() Экспортная …` from foreign code
    // instead of the platform method's signature.
    //
    // The fix is two-pronged:
    //   1. `Semantics::resolve_method_call_to_definition` (type-aware)
    //      infers the receiver Ty (`Ty::Union([ValueTable, ValueTree])`
    //      after `.Выгрузить()`) and resolves through
    //      `lookup_method_with_key` → returns
    //      `Definition::BuiltinMethod`.
    //   2. `Semantics::resolve_name_to_definition` adds a guard that
    //      refuses to fall through to `resolve_module_method` when the
    //      token is the field-name in a `FIELD_EXPR`.
    //
    // Hover emits the platform method markdown via
    // `hover_for_platform_method`, so the assertion checks for a
    // distinctive header substring.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
    Результат = Зап.Выполнить().Выгрузить().ВыгрузитьКолонку(\"Ссылка\");
КонецПроцедуры
";
    let cursor = code.rfind("ВыгрузитьКолонку").expect("fixture must contain ВыгрузитьКолонку");
    let (db, file_id) = setup_inline(code);
    let hover = Analysis::from_database(db)
        .hover(file_id, cursor as u32)
        .expect("hover must produce a result on chained ВыгрузитьКолонку");
    let markup = &hover.markup;
    assert!(
        markup.contains("ВыгрузитьКолонку") && markup.contains("UnloadColumn"),
        "hover must show bilingual platform-method header, got: {markup}"
    );
    // The bug-state markup contained these substrings — guard against
    // a regression that resolves to the workspace free function again.
    assert!(
        !markup.contains("КоллекцияСтрок"),
        "hover must NOT include workspace free-function param list, got: {markup}"
    );
}

#[test]
fn hover_on_chained_method_does_not_match_workspace_free_function() {
    // Same chain as above, but with a same-named module-method
    // definition in scope. The resolver guard must short-circuit
    // before module-level lookup; even if the platform-method hover
    // were unavailable, hover must NOT surface the workspace function
    // for a token that sits to the right of `.` in a FIELD_EXPR.
    //
    // The cursor lands on the call inside the function; the
    // workspace `ВыгрузитьКолонку` defined above is shadowed for
    // hover purposes by the type-aware platform path.
    let code = "\
Функция ВыгрузитьКолонку(КоллекцияСтрок, ИмяКолонки) Экспорт
    Возврат Новый Массив;
КонецФункции

Процедура Тест()
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
    Результат = Зап.Выполнить().Выгрузить().ВыгрузитьКолонку(\"Ссылка\");
КонецПроцедуры
";
    let cursor = code
        .rfind("ВыгрузитьКолонку(\"Ссылка")
        .expect("fixture must contain the chained call site");
    let (db, file_id) = setup_inline(code);
    let result = Analysis::from_database(db).hover(file_id, cursor as u32);
    match result {
        Some(hover) => {
            assert!(
                !hover.markup.contains("Функция ВыгрузитьКолонку()"),
                "hover must NOT render the workspace free-function header, got: {markup}",
                markup = hover.markup
            );
            assert!(
                !hover.markup.contains("КоллекцияСтрок"),
                "hover must NOT mention the workspace function's param, got: {markup}",
                markup = hover.markup
            );
        }
        None => {
            // Acceptable: type-aware lookup may degrade to None on this
            // exact fixture (no platform_data hit). The guard already did
            // its job by preventing the workspace match.
        }
    }
}

#[test]
fn hover_on_chained_method_ignores_local_var_with_same_name() {
    // Regression for the field-name-guard placement.
    //
    // The chained `…Выгрузить().ВыгрузитьКолонку("Ссылка")` token must
    // resolve through the type-aware path even when a same-named local
    // variable (`ВыгрузитьКолонку = "..."`) exists in the same scope.
    // Before the guard moved above the local-symbol fallback, the
    // resolver matched the field-name token against the local first
    // and `hover_user_defined` rendered a `Локальная переменная …`
    // hover for the method-name token — a real shadowing collision
    // that masked the platform method.
    //
    // The fix: `field_name_receiver` guard now fires after qualified
    // resolution but BEFORE builtin / local / MDO / module-method
    // fallbacks. The token then takes the type-aware platform-method
    // hover branch, not the shadowed-name branch.
    let code = "\
Процедура Тест()
    ВыгрузитьКолонку = \"shadow-name\";
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
    Результат = Зап.Выполнить().Выгрузить().ВыгрузитьКолонку(\"Ссылка\");
КонецПроцедуры
";
    // The cursor lands on the chained-call site, NOT the assignment.
    let cursor = code
        .rfind("ВыгрузитьКолонку(\"Ссылка")
        .expect("fixture must contain the chained call site");
    let (db, file_id) = setup_inline(code);
    let hover = Analysis::from_database(db)
        .hover(file_id, cursor as u32)
        .expect("hover must produce a result");
    assert!(
        hover.markup.contains("ВыгрузитьКолонку") && hover.markup.contains("UnloadColumn"),
        "hover must show platform-method header, got: {markup}",
        markup = hover.markup
    );
    assert!(
        !hover.markup.contains("Локальная переменная"),
        "hover must NOT show the shadowing local-var hover, got: {markup}",
        markup = hover.markup
    );
}

#[test]
fn unload_column_string_arg_does_not_emit_type_mismatch() {
    // Regression for the user-reported diagnostic bug:
    //   `Зап.Выполнить().Выгрузить().ВыгрузитьКолонку("Ссылка")`
    // — `<Колонка>` accepts `Число | Строка | КолонкаТаблицыЗначений`
    // per HBK, but `platform_data.json` had truncated `param_type` to
    // `"Число"` (HTML scraper bug). After fixing the scraper AND
    // routing param-type lowering through `resolve_platform_type_union`
    // in `to_method_info`, the diagnostic's `is_assignable` sees a
    // `Ty::Union` on the right and silently accepts `Ty::String`.
    let code = "\
Процедура Тест()
    Зап = Новый Запрос;
    Зап.Текст = \"ВЫБРАТЬ 1\";
    Результат = Зап.Выполнить().Выгрузить().ВыгрузитьКолонку(\"Ссылка\");
КонецПроцедуры
";
    let (db, file_id) = setup_inline(code);
    let infer = db.infer(file_id);
    let mismatches: Vec<&InferenceDiagnostic> = infer
        .diagnostics
        .iter()
        .map(|(_, d)| d)
        .filter(|d| matches!(d, InferenceDiagnostic::TypeMismatch { .. }))
        .collect();
    assert!(
        mismatches.is_empty(),
        "ВыгрузитьКолонку(\"Ссылка\") must not emit TypeMismatch — \
         param type after fix is Ty::Union([Number, String, ValueTableColumn]). \
         Got: {mismatches:#?}",
    );
}
