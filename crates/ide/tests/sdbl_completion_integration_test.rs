//! Integration tests for SDBL completion.
//!
//! These tests verify the full completion flow:
//! 1. Parse BSL code with SDBL query
//! 2. Lower SDBL AST to HIR
//! 3. Build Scope from HIR (via DbScopeProvider)
//! 4. Run completion
//!
//! Unlike unit tests which mock Scope, integration tests verify
//! the entire pipeline including query range calculation and scope building.

use ide::Analysis;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

/// Helper to create an Analysis instance from BSL code.
fn create_analysis(code: &str) -> (Analysis, FileId) {
    let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));

    let mut db = RootDatabaseImpl::default();
    let source_root_id = SourceRootId(0);

    // Create FileSet with all files
    let mut file_set = FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }

    // Create SourceRoot (local, not library)
    let source_root = SourceRoot::new_local(file_set);

    // Set source root in database (through SourceDatabase trait)
    db.set_source_root(source_root_id, source_root);

    // Set file contents and associate with source root
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, source_root_id);
        db.set_file_text(*file_id, &file.content);
    }

    let file_id = fixture.first_file().expect("No files in fixture");

    (Analysis::from_database(db), file_id)
}

#[test]
fn test_completion_for_nested_union_with_into_clause() {
    // ВАЖНО: Этот тест воспроизводит реальный сценарий из бага:
    // SELECT из alias подзапроса (с UNION) который создает временную таблицу.
    //
    // Проблема была в том что HIR lowering использовал main_query.text_range()
    // вместо select_query.text_range(), из-за чего cursor offset не попадал
    // в query range и DbScopeProvider возвращал None.

    let code = r#"Функция ТестЗапрос()
    Запрос = Новый Запрос;
    Запрос.Текст =
    "ВЫБРАТЬ
    |   Вложенный.Поле1 КАК Поле1,
    |   Вложенный.Поле2 КАК Поле2
    |ПОМЕСТИТЬ ВТ_Результат
    |ИЗ
    |   (ВЫБРАТЬ
    |       Т1.Наименование КАК Поле1,
    |       Т1.Код КАК Поле2
    |   ИЗ
    |       Справочник.Номенклатура КАК Т1
    |
    |   ОБЪЕДИНИТЬ ВСЕ
    |
    |   ВЫБРАТЬ
    |       Т2.Наименование,
    |       Т2.Код
    |   ИЗ
    |       Справочник.Контрагенты КАК Т2) КАК Вложенный
    |;
    |
    |////////////////////////////////////////////////////////////////////////////////
    |ВЫБРАТЬ
    |   ВТ_Результат.Поле1 КАК Поле1,
    |   ВТ_Результат.Поле2 КАК Поле2
    |ИЗ
    |   ВТ_Результат КАК ВТ_Результат";

    Возврат Запрос.Выполнить();
КонецФункции"#;

    let (analysis, file_id) = create_analysis(code);

    // Тест 1: Cursor в первом SELECT (строка "Вложенный.Поле1")
    // Должен показывать поля из вложенного подзапроса с UNION
    let cursor_offset_1 = code.find("Вложенный.Поле1").expect("Should find 'Вложенный.Поле1'");
    let cursor_offset_1 = cursor_offset_1 + "Вложенный.".len();

    let completions_1 =
        analysis.completions(file_id, cursor_offset_1.try_into().unwrap(), None, ide::Locale::Ru);

    // Проверяем что completion работает (не fallback на keywords)
    assert!(
        completions_1.iter().any(|c| c.label == "Поле1"),
        "Should complete 'Поле1' from subquery. Got completions: {:?}",
        completions_1.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
    assert!(
        completions_1.iter().any(|c| c.label == "Поле2"),
        "Should complete 'Поле2' from subquery. Got completions: {:?}",
        completions_1.iter().map(|c| &c.label).collect::<Vec<_>>()
    );

    // Тест 2: Cursor во втором SELECT (строка "ВТ_Результат.Поле1")
    // Должен показывать поля из временной таблицы
    let cursor_offset_2 =
        code.rfind("ВТ_Результат.Поле1").expect("Should find second 'ВТ_Результат.Поле1'");
    let cursor_offset_2 = cursor_offset_2 + "ВТ_Результат.".len();

    let completions_2 =
        analysis.completions(file_id, cursor_offset_2.try_into().unwrap(), None, ide::Locale::Ru);

    assert!(
        completions_2.iter().any(|c| c.label == "Поле1"),
        "Should complete 'Поле1' from temp table. Got completions: {:?}",
        completions_2.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
    assert!(
        completions_2.iter().any(|c| c.label == "Поле2"),
        "Should complete 'Поле2' from temp table. Got completions: {:?}",
        completions_2.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
}

// NOTE: Simple query without metadata test is skipped because
// completion requires metadata (Справочник.Номенклатура) which is not
// available in unit tests. Integration tests with real projects would test this.
