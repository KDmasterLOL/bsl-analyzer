use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Fix, TextEdit};
use ide_db::TextRange;
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub(crate) fn build_alias_fix(
    field_name: &Option<String>,
    raw_name: &Option<String>,
    bsl_range: TextRange,
) -> Vec<Fix> {
    match (field_name, raw_name) {
        (None, Some(name)) => {
            let insert = bsl_range.end();
            vec![Fix::safe(
                format!("Добавить псевдоним КАК {}", name),
                vec![TextEdit {
                    range: TextRange::new(insert, insert),
                    new_text: format!(" КАК {}", name),
                }],
            )]
        }
        (Some(name), _) => {
            let alias_byte_len = name.len() as u32;
            let insert = bsl_range.end() - line_index::TextSize::from(alias_byte_len);
            vec![Fix::safe(
                format!("Добавить ключевое слово КАК перед '{}'", name),
                vec![TextEdit {
                    range: TextRange::new(insert, insert),
                    new_text: "КАК ".to_string(),
                }],
            )]
        }
        _ => vec![],
    }
}

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::AliasWithoutAsKeyword { field_name, raw_name, range } = diag {
        let code = DiagnosticCode::AssignAliasFieldsInQuery;
        let bsl_range = mapper.map_range(*range, query_text);
        let message = if let Some(name) = field_name {
            format!("Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК", name)
        } else {
            "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string()
        };
        let fixes = build_alias_fix(field_name, raw_name, bsl_range);
        diagnostics.push(Diagnostic {
            code,
            message,
            severity: ctx.severity(code),
            range: bsl_range,
            tags: ctx.tags(code),
            fixes,
        });
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::AssignAliasFieldsInQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_diagnostics_snapshot_for, format_diags};
    use crate::{Diagnostic, DiagnosticCode, Severity};
    use expect_test::expect;
    use parser::parse_sdbl;

    fn check_standalone_query(query_text: &str) -> Vec<Diagnostic> {
        use sdbl_hir::lower_sdbl_to_hir;

        let parse = parse_sdbl(query_text);
        let package = lower_sdbl_to_hir(&parse, None);

        package
            .all_diagnostics()
            .filter_map(|d| {
                if let sdbl_hir::SdblDiagnostic::AliasWithoutAsKeyword {
                    field_name, range, ..
                } = d
                {
                    let message = if let Some(name) = field_name {
                        format!(
                            "Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК",
                            name
                        )
                    } else {
                        "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК"
                            .to_string()
                    };
                    Some(Diagnostic {
                        code: DiagnosticCode::AssignAliasFieldsInQuery,
                        message,
                        severity: Severity::Warning,
                        range: *range,
                        tags: vec![],
                        fixes: vec![],
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn check_standalone_query_snapshot(query_text: &str, expected: expect_test::Expect) {
        let diagnostics = check_standalone_query(query_text);
        expected.assert_eq(&format_diags(query_text, &diagnostics));
    }

    #[test]
    fn test_field_with_explicit_as() {
        let query = "SELECT Name AS ProductName FROM Products";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_field_without_as_keyword() {
        let query = "SELECT Name ProductName FROM Products";
        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 1:8..1:24
              message: Поле 'ProductName' должно иметь явный псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_field_without_alias() {
        let query = "SELECT Name FROM Products";
        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 1:8..1:12
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_asterisk_field() {
        let query = "SELECT * FROM Products";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_table_asterisk() {
        let query = "SELECT Products.* FROM Products";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_multiple_fields_mixed() {
        let query = "SELECT Name AS ProductName, Code ProductCode, Price FROM Products";
        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 1:29..1:45
              message: Поле 'ProductCode' должно иметь явный псевдоним с ключевым словом AS/КАК
              severity: Warning
            AssignAliasFieldsInQuery @ 1:47..1:52
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_russian_kak_keyword() {
        let query = "ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_union_query() {
        let query = "SELECT Name AS N FROM Products UNION SELECT Title FROM Services";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_sdbl_russian_query() {
        let query = "ВЫБРАТЬ Артикул, Наименование КАК ИмяТовара ИЗ Справочник.Номенклатура";

        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 1:9..1:16
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_query_with_comments() {
        let query = r#"ВЫБРАТЬ
	Товары.Артикул, // Неправильно
	Товары.Артикул КАК АртикулТовара, // Правильно
	Товары.Цена ЦенаПродажи // Неправильно
ИЗ
	Справочник.Номенклатура КАК Товары // Игнорируется

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Услуги.Артикул, // Игнорируется
	Услуги.Артикул, // Игнорируется
	Услуги.Тариф // Игнорируется
ИЗ
	Справочник.Услуги КАК Услуги"#;

        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 2:2..2:16
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning
            AssignAliasFieldsInQuery @ 4:2..4:25
              message: Поле 'ЦенаПродажи' должно иметь явный псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_simple_query_with_hir() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        let code = r#"Процедура Тест()
Запрос = "ВЫБРАТЬ Товары.Артикул, Товары.Цена ЦенаПродажи ИЗ Справочник.Номенклатура КАК Товары";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs = db.sdbl_hir_in_file(file_id);

        assert_eq!(sdbl_hirs.len(), 1);
        assert!(
            !sdbl_hirs[0].1.queries()[0].hir.diagnostics.is_empty(),
            "Expected diagnostics for fields without AS keyword"
        );
    }

    #[test]
    fn test_wrapped_vs_unwrapped_code() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        let code_wrapped = r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Товары.Артикул, Товары.Цена ЦенаПродажи ИЗ Справочник.Номенклатура КАК Товары";
КонецПроцедуры"#;

        let fixture_text = format!("//- /test.bsl\n{}", code_wrapped);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs_wrapped = db.sdbl_hir_in_file(file_id);

        let code_unwrapped = r#"Запрос = "ВЫБРАТЬ Товары.Артикул, Товары.Цена ЦенаПродажи ИЗ Справочник.Номенклатура КАК Товары";"#;

        let fixture_text = format!("//- /test.bsl\n{}", code_unwrapped);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let sdbl_hirs_unwrapped = db.sdbl_hir_in_file(file_id);

        assert!(!sdbl_hirs_wrapped.is_empty() || !sdbl_hirs_unwrapped.is_empty());
    }

    #[test]
    fn test_union_with_diagnostics() {
        let query = r#"ВЫБРАТЬ
	Товары.Артикул,
	Товары.Артикул КАК АртикулТовара,
	Товары.Цена ЦенаПродажи
ИЗ
	Справочник.Номенклатура КАК Товары

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Услуги.Артикул,
	Услуги.Артикул,
	Услуги.Тариф
ИЗ
	Справочник.Услуги КАК Услуги"#;

        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 2:2..2:16
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning
            AssignAliasFieldsInQuery @ 4:2..4:25
              message: Поле 'ЦенаПродажи' должно иметь явный псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_top_clause_with_explicit_alias() {
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_top_clause_parsing() {
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let parse = parser::parse_sdbl(query);
        assert!(!parse.has_errors(), "Parse should not have errors");

        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_top_clause_without_alias() {
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 2:1..2:17
              message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_top_clause_implicit_alias() {
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        check_standalone_query_snapshot(
            query,
            expect![[r#"
            AssignAliasFieldsInQuery @ 2:1..2:30
              message: Поле 'Номенклатура' должно иметь явный псевдоним с ключевым словом AS/КАК
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_distinct_clause() {
        let query = "SELECT DISTINCT Name AS ProductName FROM Products";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_distinct_top_combination() {
        let query = "ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Код КАК К ИЗ Товары";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_top_distinct_order() {
        let query = "SELECT TOP 50 DISTINCT Name AS N FROM Products";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_query_with_union_two_diagnostics() {
        let code = r#"Запрос = Новый Запрос;
Запрос.Текст =
	"ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка КАК ПсевдонимПоляСсылка,
	|	Валюты.Код Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты
	|
	|ОБЪЕДИНИТЬ ВСЕ
	|
	|ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка,
	|	Валюты.Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты";"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#"
                AssignAliasFieldsInQuery @ 4:4..4:17
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 6:4..6:18
                  message: Поле 'Код' должно иметь явный псевдоним с ключевым словом AS/КАК
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_second_query_with_union_two_diagnostics() {
        let code = r#"Запрос = Новый Запрос;
Запрос.Текст =
	"ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка КАК ПсевдонимПоляСсылка,
	|	Валюты.Код Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты
	|
	|ОБЪЕДИНИТЬ ВСЕ
	|
	|ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка,
	|	Валюты.Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты";

Запрос2 = Новый Запрос;
Запрос2.Текст =
	"ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка КАК ПсевдонимПоляСсылка,
	|	Валюты.Код Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты
	|
	|ОБЪЕДИНИТЬ ВСЕ
	|
	|ВЫБРАТЬ
	|	Валюты.Ссылка,
	|	Валюты.Ссылка,
	|	Валюты.Код
	|ИЗ
	|	Справочник.Валюты КАК Валюты";"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#"
                AssignAliasFieldsInQuery @ 4:4..4:17
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 6:4..6:18
                  message: Поле 'Код' должно иметь явный псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 22:4..22:17
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 24:4..24:18
                  message: Поле 'Код' должно иметь явный псевдоним с ключевым словом AS/КАК
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_nested_subquery_field_without_alias() {
        let code = r#"Запрос1 = Новый Запрос;
Запрос1.Текст =
	"ВЫБРАТЬ
	|	ВложенныйЗапрос.Ссылка КАК Ссылка
	|ИЗ
	|	(ВЫБРАТЬ
	|		Валюты.Ссылка
	|	ИЗ
	|		Справочник.Валюты КАК Валюты) КАК ВложенныйЗапрос";"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#"
                AssignAliasFieldsInQuery @ 7:5..7:18
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_union_part_does_not_emit_when_alias_missing() {
        let query = "SELECT Name AS Name FROM Products UNION ALL SELECT Title FROM Services";
        check_standalone_query_snapshot(query, expect![[r#""#]]);
    }

    #[test]
    fn test_union_part_uses_first_query_aliases_regression() {
        let code = r#"Запрос = Новый Запрос;
Запрос.Текст =
	"ВЫБРАТЬ
	|	ДополнительныеРеквизиты.Ссылка КАК Набор,
	|	ДополнительныеРеквизиты.Свойство КАК Свойство
	|ПОМЕСТИТЬ ВТ_ВсеНаборы
	|ИЗ
	|	Справочник.НаборыДополнительныхРеквизитовИСведений.ДополнительныеРеквизиты КАК ДополнительныеРеквизиты
	|
	|ОБЪЕДИНИТЬ ВСЕ
	|
	|ВЫБРАТЬ
	|	ДополнительныеСведения.Ссылка,
	|	ДополнительныеСведения.Свойство
	|ИЗ
	|	Справочник.НаборыДополнительныхРеквизитовИСведений.ДополнительныеСведения КАК ДополнительныеСведения";"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_query_with_leading_newline_field_without_alias() {
        let code = "ТекстЗапроса = \"\n\t|ВЫБРАТЬ\n\t|\tВТ_ТЧ.НомерСтроки\n\t|ИЗ\n\t|\t&ВТ_Цены КАК ВТ_Цены\n\t|;\n\t|\n\t|ВЫБРАТЬ\n\t|\t\" + ПоляТЧДокумента + \"\n\t|ИЗ\n\t|\t&ВТ_ТЧ КАК Товары\";";

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#"
                AssignAliasFieldsInQuery @ 3:4..3:21
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning"#]],
        );
    }

    #[test]
    fn track3_function_aggregate_and_case_fields_require_explicit_aliases_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   ЕСТЬNULL(Товары.Артикул, """"),
        |   СУММА(Товары.Количество),
        |   ВЫБОР
        |       КОГДА Товары.ПометкаУдаления ТОГДА 1
        |       ИНАЧЕ 0
        |   КОНЕЦ
        |ИЗ
        |   Справочник.Номенклатура КАК Товары";
КонецПроцедуры"#,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#"
                AssignAliasFieldsInQuery @ 5:13..5:43
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 6:13..6:37
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning
                AssignAliasFieldsInQuery @ 7:13..10:18
                  message: Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК
                  severity: Warning"#]],
        );
    }

    #[test]
    fn track3_split_concatenated_query_is_not_reconstructed_snapshot() {
        check_diagnostics_snapshot_for(
            r#"Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст =
        "ВЫБРАТЬ
        |   " + ИмяПоля + "
        |ИЗ
        |   Справочник.Номенклатура КАК Товары";
КонецПроцедуры"#,
            DiagnosticCode::AssignAliasFieldsInQuery,
            expect![[r#""#]],
        );
    }
}
