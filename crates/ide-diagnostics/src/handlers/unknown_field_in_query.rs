use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use sdbl_hir;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious, MetadataTag::Sql],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub(crate) fn dispatch(
    ctx: &DiagnosticsContext,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &crate::sdbl_utils::SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let sdbl_hir::SdblDiagnostic::UnknownField { table_name, field_name, range } = diag {
        let code = DiagnosticCode::UnknownFieldInQuery;
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Поле \"{}\" не найдено в таблице \"{}\" запроса",
                field_name, table_name
            ),
            severity: ctx.severity(code),
            range: mapper.map_range(*range, query_text),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    crate::sdbl_utils::collect_sdbl_via_dispatch(ctx, DiagnosticCode::UnknownFieldInQuery, dispatch)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::format_diags;
    use crate::{DiagnosticsConfig, DiagnosticsContext};
    use expect_test::{expect, Expect};
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use std::path::PathBuf;
    use vfs::{FileId, FileSet, VfsPath};

    /// Runs the diagnostic against the shared designer config fixture, which
    /// supplies a fully-modeled catalog (`Справочник1` with attributes and a
    /// tabular section) and an information register (`РегистрСведений1`).
    fn check_with_designer(query_body: &str, expected: Expect) {
        let fixtures_dir =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let code = format!(
            "Процедура Тест()\n    Запрос = Новый Запрос;\n    Запрос.Текст = \"{}\";\nКонецПроцедуры",
            query_body
        );

        let mut db = RootDatabaseImpl::new();
        let workspace_root = PathBuf::from(fixtures_dir);

        let mut file_set = FileSet::default();
        let file_id = FileId(0);
        let module_path = VfsPath::new(format!("{}/Ext/SessionModule.bsl", fixtures_dir));
        file_set.insert(file_id, module_path);

        let source_root_id = SourceRootId(0);
        db.set_source_root(source_root_id, SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, source_root_id);
        db.set_file_text(file_id, &code);

        let configuration_path_input = ide_db::metadata::ConfigurationPathInput::new(
            &db,
            workspace_root.to_string_lossy().to_string(),
            0,
        );

        let provider = ide_db::SalsaProvider::new(&db, Some(configuration_path_input));
        let config = DiagnosticsConfig::all_enabled();
        let ctx = DiagnosticsContext::new(&config, file_id, &provider);

        let diagnostics = check(&ctx);
        expected.assert_eq(&format_diags(&code, &diagnostics));
    }

    #[test]
    fn positive_unknown_field_on_register() {
        check_with_designer(
            "ВЫБРАТЬ Т.НетТакогоПоля ИЗ РегистрСведений.РегистрСведений1 КАК Т",
            expect![[r#"
                UnknownFieldInQuery @ 3:31..3:44
                  message: Поле "НетТакогоПоля" не найдено в таблице "РегистрСведений.РегистрСведений1" запроса
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn negative_valid_dimension() {
        check_with_designer(
            "ВЫБРАТЬ Т.Справочник1 ИЗ РегистрСведений.РегистрСведений1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_standard_register_fields() {
        // Information-register standard fields (ВидДвижения is accumulation-only
        // and is intentionally absent here — see the positive test below).
        check_with_designer(
            "ВЫБРАТЬ Т.Период, Т.Регистратор, Т.Активность, Т.НомерСтроки, Т.МоментВремени ИЗ РегистрСведений.РегистрСведений1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn positive_movement_type_not_on_information_register() {
        // `ВидДвижения` exists only on accumulation registers; on an information
        // register it is a genuine unknown field.
        check_with_designer(
            "ВЫБРАТЬ Т.ВидДвижения ИЗ РегистрСведений.РегистрСведений1 КАК Т",
            expect![[r#"
                UnknownFieldInQuery @ 3:31..3:42
                  message: Поле "ВидДвижения" не найдено в таблице "РегистрСведений.РегистрСведений1" запроса
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn negative_valid_catalog_attribute() {
        check_with_designer(
            "ВЫБРАТЬ Т.Реквизит1 ИЗ Справочник.Справочник1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_virtual_presentation_and_data_version() {
        check_with_designer(
            "ВЫБРАТЬ Т.Представление, Т.ВерсияДанных ИЗ Справочник.Справочник1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_tabular_section_name() {
        check_with_designer(
            "ВЫБРАТЬ Т.ТабличнаяЧасть1 ИЗ Справочник.Справочник1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_select_star() {
        check_with_designer("ВЫБРАТЬ * ИЗ Справочник.Справочник1 КАК Т", expect![[r#""#]]);
    }

    #[test]
    fn negative_extension_object_gate_off() {
        // Resolves as a missing object (QueryToMissingMetadata path), so the
        // field model is incomplete and the unknown-field gate stays off.
        check_with_designer(
            "ВЫБРАТЬ Т.НетТакогоПоля ИЗ Справочник.НесуществующийСправочник КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_register_virtual_table_gate_off() {
        check_with_designer(
            "ВЫБРАТЬ Т.НетТакогоПоля ИЗ РегистрСведений.РегистрСведений1.СрезПоследних КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_temp_table_gate_off() {
        check_with_designer("ВЫБРАТЬ Т.НетТакогоПоля ИЗ ВременнаяТаблица КАК Т", expect![[r#""#]]);
    }

    #[test]
    fn negative_document_point_in_time() {
        // МоментВремени is a query-only virtual field of document tables.
        check_with_designer(
            "ВЫБРАТЬ Т.МоментВремени ИЗ Документ.Документ1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_chart_of_accounts_standard_fields() {
        // Родитель is unconditional (no Hierarchical property in the XML);
        // Вид/Забалансовый/accounting flags/ВидыСубконто are account-row fields.
        check_with_designer(
            "ВЫБРАТЬ Т.Родитель, Т.Порядок, Т.Вид, Т.Забалансовый, Т.Валютный, Т.ВидыСубконто ИЗ ПланСчетов.ПланСчетов1 КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn negative_chart_of_accounts_ext_dimension_table() {
        check_with_designer(
            "ВЫБРАТЬ Т.НомерСтроки, Т.ВидСубконто, Т.Предопределенное, Т.ТолькоОбороты, Т.Суммовой ИЗ ПланСчетов.ПланСчетов1.ВидыСубконто КАК Т",
            expect![[r#""#]],
        );
    }

    #[test]
    fn positive_unknown_field_on_chart_of_accounts() {
        // The chart-of-accounts model is exhaustive, so the gate stays on.
        check_with_designer(
            "ВЫБРАТЬ Т.НетТакогоПоля ИЗ ПланСчетов.ПланСчетов1 КАК Т",
            expect![[r#"
                UnknownFieldInQuery @ 3:31..3:44
                  message: Поле "НетТакогоПоля" не найдено в таблице "ПланСчетов.ПланСчетов1" запроса
                  severity: Blocker"#]],
        );
    }

    #[test]
    fn bilingual_unknown_field_english_table() {
        check_with_designer(
            "SELECT T.NoSuchField FROM Catalog.Справочник1 AS T",
            expect![[r#"
                UnknownFieldInQuery @ 3:30..3:41
                  message: Поле "NoSuchField" не найдено в таблице "Catalog.Справочник1" запроса
                  severity: Blocker"#]],
        );
    }
}
