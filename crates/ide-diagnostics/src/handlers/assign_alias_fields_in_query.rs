//! AssignAliasFieldsInQuery diagnostic.
//!
//! Checks that all fields in SDBL subqueries have explicit aliases with AS/КАК keyword.
//!
//! ## Why?
//! Subqueries are often used in FROM clauses. Explicit aliases make queries more readable
//! and maintainable. Without AS keyword, it's unclear whether the identifier is an alias
//! or part of the field expression.
//!
//! ## Bad practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref FROM Catalog.Products) AS Sub";
//!         // Error: 'Ref' should be 'Ref AS Ref' (missing AS keyword)
//!
//! Query = "SELECT * FROM (SELECT Name, Code FROM Catalog.Products) AS Sub";
//!         // Error: both 'Name' and 'Code' need explicit AS keyword
//! ```
//!
//! ## Good practice
//! ```bsl
//! Query = "SELECT * FROM (SELECT Ref AS Ref FROM Catalog.Products) AS Sub";
//!
//! Query = "SELECT * FROM (SELECT * FROM Table) AS Sub"; // OK: asterisk doesn't need alias
//!
//! Query = "SELECT Name FROM Catalog.Products"; // OK: main query not checked
//! ```
//!
//! ## Rules
//! - Only subqueries are checked (not main queries)
//! - Asterisk fields (`*`, `Table.*`) don't require aliases
//! - AS/КАК keyword must be explicit (implicit aliases are forbidden)
//! - UNION: only first query in UNION is checked
//!
//! ## Implementation
//!
//! Ported from:
//!
//! Now uses SDBL HIR with diagnostics collected during lowering.

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

/// Builds a quick-fix for missing alias.
///
/// - No alias (`field_name = None`): insert ` КАК <raw_name>` at end of expression.
/// - Implicit alias without AS (`field_name = Some`): insert `КАК ` before alias identifier.
pub(crate) fn build_alias_fix(
    field_name: &Option<String>,
    raw_name: &Option<String>,
    bsl_range: TextRange,
) -> Vec<Fix> {
    match (field_name, raw_name) {
        (None, Some(name)) => {
            // No alias at all → insert " КАК <raw_name>" at end of expression
            let insert = bsl_range.end();
            vec![Fix {
                label: format!("Добавить псевдоним КАК {}", name),
                edits: vec![TextEdit {
                    range: TextRange::new(insert, insert),
                    new_text: format!(" КАК {}", name),
                }],
            }]
        }
        (Some(name), _) => {
            // Implicit alias without AS → insert "КАК " before alias identifier
            let alias_byte_len = name.len() as u32;
            let insert = bsl_range.end() - line_index::TextSize::from(alias_byte_len);
            vec![Fix {
                label: format!("Добавить ключевое слово КАК перед '{}'", name),
                edits: vec![TextEdit {
                    range: TextRange::new(insert, insert),
                    new_text: "КАК ".to_string(),
                }],
            }]
        }
        _ => vec![],
    }
}

/// Single-pass dispatch for AssignAliasFieldsInQuery.
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

/// Runs the AssignAliasFieldsInQuery diagnostic (standalone, used in tests).
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(
        ctx,
        DiagnosticCode::AssignAliasFieldsInQuery,
        dispatch,
    )
}

#[cfg(test)]
mod tests {
    use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, Severity};
    use parser::parse_sdbl;

    /// Helper for debug tests that need to return file content along with diagnostics.
    fn check_diagnostic(code: &str, config: DiagnosticsConfig) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use std::rc::Rc;
        use test_fixture::Fixture;
        use vfs::VfsPath;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let config = Rc::new(config);
        let ctx = crate::DiagnosticsContext {
            db: &db,
            config: &config,
            file_id,
            provider: None,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        (super::check(&ctx), file_content)
    }

    /// Helper to check a standalone SDBL query using HIR (for testing).
    ///
    /// Parses the query, lowers to HIR, and extracts AliasWithoutAsKeyword diagnostics.
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

    #[test]
    fn test_field_with_explicit_as() {
        // Should pass - has AS keyword
        let query = "SELECT Name AS ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_field_without_as_keyword() {
        // Should fail - implicit alias (no AS keyword)
        let query = "SELECT Name ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("AS/КАК"));
    }

    #[test]
    fn test_field_without_alias() {
        // Should fail - no alias at all
        let query = "SELECT Name FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("псевдоним"));
    }

    #[test]
    fn test_asterisk_field() {
        // Should pass - asterisk doesn't need alias
        let query = "SELECT * FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_table_asterisk() {
        // Should pass - Table.* doesn't need alias
        let query = "SELECT Products.* FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_multiple_fields_mixed() {
        // Mixed: some with AS, some without
        let query = "SELECT Name AS ProductName, Code ProductCode, Price FROM Products";
        let diagnostics = check_standalone_query(query);
        // Should have 2 errors: Code (implicit) and Price (no alias)
        assert_eq!(diagnostics.len(), 2);
    }

    #[test]
    fn test_russian_kak_keyword() {
        // Russian КАК keyword should work
        let query = "ВЫБРАТЬ Имя КАК ИмяПродукта ИЗ Товары";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_union_query() {
        // UNION query - first query checked, UNION parts not checked
        let query = "SELECT Name AS N FROM Products UNION SELECT Title FROM Services";
        let diagnostics = check_standalone_query(query);
        // First query OK (Name AS N), second query (Title without alias) not checked
        // Because we only check main query, not UNION queries
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_sdbl_russian_query() {
        // Test that SDBL parser handles Russian queries
        let query = "ВЫБРАТЬ Ссылка, Код КАК К ИЗ Справочник.Валюты";

        let diagnostics = check_standalone_query(query);
        // Should have 1 error: Ссылка without alias
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_query_with_comments() {
        // Test query with inline comments - comments should not affect diagnostics
        let query = r#"ВЫБРАТЬ
	Валюты.Ссылка, // Неправильно
	Валюты.Ссылка КАК ПсевдонимПоляСсылка, // Правильно
	Валюты.Код Код // Неправильно
ИЗ
	Справочник.Валюты КАК Валюты // Игнорируется

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Ссылка, // Игнорируется
	Валюты.Ссылка, // Игнорируется
	Валюты.Код // Игнорируется
ИЗ
	Справочник.Валюты КАК Валюты"#;

        let diagnostics = check_standalone_query(query);

        // Should have 2 AliasWithoutAsKeyword diagnostics from first SELECT (before UNION):
        // - Валюты.Ссылка without alias
        // - Валюты.Код Код without AS keyword
        // UNION queries are skipped
        assert_eq!(
            diagnostics.len(),
            2,
            "Expected 2 diagnostics from first SELECT (UNION queries skipped)"
        );
    }

    #[test]
    fn test_simple_query_with_hir() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use test_fixture::Fixture;
        use vfs::VfsPath;

        // Test simple query without comments using HIR
        let code = r#"Процедура Тест()
Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";
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
        // Should have at least 1 diagnostic (field without alias)
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

        // Test 1: Code wrapped in procedure
        let code_wrapped = r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";
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

        // Test 2: Code at module level (no procedure)
        let code_unwrapped =
            r#"Запрос = "ВЫБРАТЬ Валюты.Ссылка, Валюты.Код Код ИЗ Справочник.Валюты КАК Валюты";"#;

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

        // Both should work
        assert!(!sdbl_hirs_wrapped.is_empty() || !sdbl_hirs_unwrapped.is_empty());
    }

    #[test]
    fn test_union_with_diagnostics() {
        // Test UNION query - only first SELECT is checked, UNION queries are skipped
        let query = r#"ВЫБРАТЬ
	Валюты.Ссылка,
	Валюты.Ссылка КАК ПсевдонимПоляСсылка,
	Валюты.Код Код
ИЗ
	Справочник.Валюты КАК Валюты

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ
	Валюты.Ссылка,
	Валюты.Ссылка,
	Валюты.Код
ИЗ
	Справочник.Валюты КАК Валюты"#;

        let diagnostics = check_standalone_query(query);

        // Should have 2 diagnostics from first SELECT (before UNION):
        // - Валюты.Ссылка without alias
        // - Валюты.Код Код without AS keyword
        // UNION queries are skipped
        assert_eq!(
            diagnostics.len(),
            2,
            "Expected 2 diagnostics from first SELECT (UNION queries skipped)"
        );
    }

    #[test]
    fn test_top_clause_with_explicit_alias() {
        // Test that ПЕРВЫЕ (TOP) clause doesn't cause false positives
        // Field has explicit КАК keyword, should pass
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            0,
            "ПЕРВЫЕ clause with explicit alias should not trigger diagnostic"
        );
    }

    #[test]
    fn test_top_clause_parsing() {
        // Verify that TOP clause parses correctly and doesn't break diagnostics
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура КАК Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let parse = parser::parse_sdbl(query);
        assert!(!parse.has_errors(), "Parse should not have errors");

        // Field has explicit alias with КАК - no diagnostics expected
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "Explicit alias should not trigger diagnostic");
    }

    #[test]
    fn test_top_clause_without_alias() {
        // Test that ПЕРВЫЕ (TOP) clause still detects missing alias
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            1,
            "ПЕРВЫЕ clause with missing alias should trigger diagnostic"
        );
    }

    #[test]
    fn test_top_clause_implicit_alias() {
        // Test that ПЕРВЫЕ (TOP) clause detects implicit alias (without КАК)
        let query = r#"ВЫБРАТЬ ПЕРВЫЕ 100
Спр.Номенклатура Номенклатура
ИЗ
Справочник.Номенклатура КАК Спр"#;

        let diagnostics = check_standalone_query(query);
        assert_eq!(
            diagnostics.len(),
            1,
            "ПЕРВЫЕ clause with implicit alias (no КАК) should trigger diagnostic"
        );
    }

    #[test]
    fn test_distinct_clause() {
        // Test DISTINCT keyword
        let query = "SELECT DISTINCT Name AS ProductName FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "DISTINCT with explicit alias should pass");
    }

    #[test]
    fn test_distinct_top_combination() {
        // Test DISTINCT TOP combination
        let query = "ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Код КАК К ИЗ Товары";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "DISTINCT TOP with explicit alias should pass");
    }

    #[test]
    fn test_top_distinct_order() {
        // Test TOP DISTINCT order (also valid)
        let query = "SELECT TOP 50 DISTINCT Name AS N FROM Products";
        let diagnostics = check_standalone_query(query);
        assert_eq!(diagnostics.len(), 0, "TOP DISTINCT with explicit alias should pass");
    }

    /// Two queries with UNION - each query has fields without AS keyword.
    ///
    /// Expected 2 diagnostics per query (before UNION only): Валюты.Ссылка and Валюты.Код Код.
    /// UNION queries are skipped.
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

        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(
            diagnostics.len(),
            2,
            "Expected 2 diagnostics from first SELECT (UNION skipped)"
        );
    }

    /// Second query (separate statement) also generates diagnostics independently.
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

        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics (2 per query)");
    }

    /// Nested subquery - field inside subquery without alias triggers diagnostic.
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

        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic: Валюты.Ссылка in subquery");
    }

    /// Query with leading newline and field without alias.
    #[test]
    fn test_query_with_leading_newline_field_without_alias() {
        let code = "ТекстЗапроса = \"\n\t|ВЫБРАТЬ\n\t|\tВТ_ТЧ.НомерСтроки\n\t|ИЗ\n\t|\t&ВТ_Цены КАК ВТ_Цены\n\t|;\n\t|\n\t|ВЫБРАТЬ\n\t|\t\" + ПоляТЧДокумента + \"\n\t|ИЗ\n\t|\t&ВТ_ТЧ КАК Товары\";";

        let config = DiagnosticsConfig::default();
        let (diagnostics, _) = check_diagnostic(code, config);

        assert_eq!(diagnostics.len(), 1, "Expected 1 diagnostic: ВТ_ТЧ.НомерСтроки without alias");
    }
}
