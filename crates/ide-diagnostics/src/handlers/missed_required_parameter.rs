use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{MethodSymbol, ModuleId, Name};
use ide_db::TextRange;
use vfs::FileId;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    callee: &str,
    module: Option<&str>,
    mdo_type: Option<&str>,
    mdo_name: Option<&str>,
    args: &[bool],
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MissedRequiredParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let missing = if let (Some(mdo_type_kw), Some(mdo_obj_name)) = (mdo_type, mdo_name) {
        tracing::debug!(
            mdo_type = mdo_type_kw,
            mdo_name = mdo_obj_name,
            callee,
            "Processing three-level call in from_hir"
        );
        check_manager_module_call(ctx, mdo_type_kw, mdo_obj_name, callee, args)?
    } else if let Some(module_name) = module {
        check_qualified_call(ctx, module_name, callee, args)?
    } else {
        check_local_call(ctx, callee, args)?
    };

    if missing.is_empty() {
        return None;
    }

    let param_list =
        missing.iter().map(|name| format!("'{}'", name)).collect::<Vec<_>>().join(", ");
    let message = format!("Укажите обязательный параметр {}", param_list);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn check_local_call(
    ctx: &DiagnosticsContext,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    let symbol_tree = ctx.symbol_tree();
    let name = Name::new(method_name);

    let method = symbol_tree.find_method(&name)?;
    Some(check_missing_params(method, args))
}

fn check_qualified_call(
    ctx: &DiagnosticsContext,
    module_name: &str,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    let _span =
        tracing::debug_span!("check_qualified_call", module = module_name, method = method_name)
            .entered();

    let name = Name::new(method_name);
    let bodies = ctx.common_module_bodies(module_name);
    if bodies.is_empty() {
        tracing::debug!("bailout: no CommonModule found in any visible configuration");
        return None;
    }

    match bodies.search_merged_surface(|module_file_id| {
        let symbol_tree = ctx.symbol_tree_for(ModuleId::new(module_file_id));
        let method = symbol_tree.find_method(&name)?;
        method.is_export.then(|| check_missing_params(method, args))
    }) {
        hir::BodySearch::Found(missing) => {
            tracing::debug!(missing_count = missing.len(), "check_qualified_call success");
            Some(missing)
        }
        hir::BodySearch::Absent => {
            tracing::debug!("bailout: method not found (or not exported) in any defining module");
            None
        }
        // Part of the module's surface could not be read, so the signature that would
        // really answer this call is unknown: measuring the call against the readable
        // remainder would demand arguments nobody can show are required.
        hir::BodySearch::Unread => {
            tracing::debug!("bailout: a body of the module could not be read");
            None
        }
    }
}

fn check_manager_module_call(
    ctx: &DiagnosticsContext,
    mdo_type_keyword: &str,
    mdo_name: &str,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    let _span =
        tracing::debug_span!("check_manager_module_call", mdo_type_keyword, mdo_name, method_name)
            .entered();

    let mdo_type = match bsl_metadata::MdoType::from_plural(mdo_type_keyword) {
        Some(t) => t,
        None => {
            tracing::debug!(
                mdo_type_keyword,
                mdo_name,
                method_name,
                "Unknown MDO type keyword, cannot check manager module call"
            );
            return None;
        }
    };

    tracing::debug!(
        mdo_type = ?mdo_type,
        mdo_name,
        method_name,
        "Checking manager module method call"
    );

    let manager_file_id = find_manager_module_file(ctx, mdo_type, mdo_name)?;

    let module_id = ModuleId::new(manager_file_id);
    let manager_symbol_tree = ctx.symbol_tree_for(module_id);

    let method_name_obj = Name::new(method_name);
    let method = manager_symbol_tree.find_method(&method_name_obj)?;

    if !method.is_export {
        tracing::debug!(
            mdo_type = ?mdo_type,
            mdo_name,
            method_name,
            "Method is not exported, skipping manager module call validation"
        );
        return None;
    }

    Some(check_missing_params(method, args))
}

fn find_manager_module_file(
    ctx: &DiagnosticsContext,
    mdo_type: bsl_metadata::MdoType,
    mdo_name: &str,
) -> Option<FileId> {
    let english_plural = match mdo_type {
        bsl_metadata::MdoType::Document => "Documents",
        bsl_metadata::MdoType::Catalog => "Catalogs",
        bsl_metadata::MdoType::InformationRegister => "InformationRegisters",
        bsl_metadata::MdoType::AccumulationRegister => "AccumulationRegisters",
        bsl_metadata::MdoType::AccountingRegister => "AccountingRegisters",
        bsl_metadata::MdoType::CalculationRegister => "CalculationRegisters",
        bsl_metadata::MdoType::ChartOfCharacteristicTypes => "ChartsOfCharacteristicTypes",
        bsl_metadata::MdoType::ChartOfAccounts => "ChartsOfAccounts",
        bsl_metadata::MdoType::ChartOfCalculationTypes => "ChartsOfCalculationTypes",
        bsl_metadata::MdoType::BusinessProcess => "BusinessProcesses",
        bsl_metadata::MdoType::Task => "Tasks",
        _ => {
            tracing::debug!(
                mdo_type = ?mdo_type,
                "MDO type does not have Manager Module"
            );
            return None;
        }
    };

    let manager_module_path = format!("{}/{}/Ext/ManagerModule.bsl", english_plural, mdo_name);
    // Конвенционные позиции кандидата регистронезависимы; `mdo_name` — имя
    // объекта, его позиция точная.
    use bsl_conventions::SegmentMatch as M;
    let modes = [M::Ci, M::Exact, M::Ci, M::Ci];

    for visible in ctx.visible_configurations() {
        if !visible.config.configuration.has_metadata_object(mdo_type, mdo_name) {
            continue;
        }
        let full_path = visible.root.join(&manager_module_path);
        if let Some(file_id) = ctx.resolve_vfs_path_ci(base_db::SourceRootId(0), &full_path, &modes)
        {
            return Some(file_id);
        }
        tracing::warn!(
            mdo_type = ?mdo_type,
            mdo_name,
            path = %full_path.display(),
            "Manager Module file not found in VFS - ensure file is loaded"
        );
    }

    tracing::debug!(
        mdo_type = ?mdo_type,
        mdo_name,
        "Metadata object not found in any visible configuration"
    );
    None
}

fn check_missing_params(method: &MethodSymbol, provided_args: &[bool]) -> Vec<String> {
    let mut missing = Vec::new();

    for (i, param) in method.params.iter().enumerate() {
        if param.has_default {
            continue;
        }

        let is_missing = i >= provided_args.len() || !provided_args[i];

        if is_missing {
            missing.push(param.name.as_str().to_string());
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_diagnostics_snapshot_for, check_snapshot_with_cfe};
    use crate::DiagnosticCode;
    use expect_test::expect;
    use test_fixture::CfeFixtureBuilder;

    #[test]
    fn test_missed_required_parameter_simple() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(, 2);
КонецПроцедуры

Функция Сложение(Левый, Правый)
    Возврат Левый + Правый;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissedRequiredParameter,
            expect![[r#"
                MissedRequiredParameter @ 3:17..3:30
                  message: Укажите обязательный параметр 'Левый'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_cfe_exported_common_module_signature_is_visible() {
        let code = r#"
#Область ПрограммныйИнтерфейс
// Описание
Процедура Тест() Экспорт
    РасширениеApi.Требует();
КонецПроцедуры
#КонецОбласти
"#;
        let mut builder = CfeFixtureBuilder::new("");
        builder.add_extension("ApiExt", "").add_extension_module(
            "ApiExt",
            "РасширениеApi",
            r#"
Процедура Требует(Значение) Экспорт
КонецПроцедуры
"#,
        );

        check_snapshot_with_cfe(
            code,
            builder.build(),
            expect![[r#"
                MissedRequiredParameter @ 5:5..5:26
                  message: Укажите обязательный параметр 'Значение'
                  severity: Major
                MismatchedArgCount @ 5:5..5:26
                  message: Неверное количество аргументов: ожидалось 1, передано 0
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"Процедура Рассчет()

    Результат = Сложение(, 2); // Range(2, 16, 2, 29)
    Сообщить(Результат);

    Инкремент(Результат);
    Сообщить(Результат);

    Результат = Сложение(5); // Range(8, 16, 8, 27)
    Сообщить(Результат);

    Результат = Сложение(5, 4, 3);
    Сообщить(Результат);

    Результат = Сложение(); // 2xRange(14, 16, 14, 26)
    Сообщить(Результат);

    Сообщить(Сложение(,)); // 2хRange(17, 13, 17, 24)
    Сообщить(Менеджер("Справочник")); // Range(18, 13, 18, 35)
КонецПроцедуры

Процедура Версионирование()
    ВерсионированиеПриЗаписи(1);
    Документы.ПКО.ВерсионированиеПриЗаписи(1);
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1); // Range(24, 22, 24, 49)
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(2,); // Range(25, 22, 25, 50)
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(); // 2xRange(26, 22, 26, 48)
    Сообщить(ПервыйОбщийМодуль.ВерсионированиеПриЗаписи()); // 2xRange(27, 31, 27, 57)
    Справочники.Справочник1.Тест(); // Range(28, 28, 28, 34);
    Результат = ЭтотОбъект.Сложение(, 2);

КонецПроцедуры

Функция Сложение(Левый, Правый) Экспорт
    Возврат Левый + Правый;
КонецФункции

Функция Инкремент(Значение, Приращение = 1)
    Значение = Значение + Приращение;
    Возврат Значение;
КонецФункции

Функция Менеджер(Тип = "Справочник", Вид)
    ИмяТипа = СтрШаблон("%1Менеджер.%2", Тип, Вид);
    Возврат Новый(Тип(ИмяТипа));
КонецФункции"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissedRequiredParameter,
            expect![[r#"
                MissedRequiredParameter @ 3:17..3:30
                  message: Укажите обязательный параметр 'Левый'
                  severity: Major
                MissedRequiredParameter @ 9:17..9:28
                  message: Укажите обязательный параметр 'Правый'
                  severity: Major
                MissedRequiredParameter @ 15:17..15:27
                  message: Укажите обязательный параметр 'Левый', 'Правый'
                  severity: Major
                MissedRequiredParameter @ 18:14..18:25
                  message: Укажите обязательный параметр 'Левый', 'Правый'
                  severity: Major
                MissedRequiredParameter @ 19:14..19:36
                  message: Укажите обязательный параметр 'Вид'
                  severity: Major
                MissedRequiredParameter @ 30:17..30:41
                  message: Укажите обязательный параметр 'Левый'
                  severity: Major"#]],
        );
    }

    #[test]
    fn test_optional_parameters_not_required() {
        let code = r#"
Процедура Тест()
    Инкремент(5);
КонецПроцедуры

Функция Инкремент(Значение, Приращение = 1)
    Возврат Значение + Приращение;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissedRequiredParameter,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_extra_parameters_allowed() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(1, 2, 3, 4);
КонецПроцедуры

Функция Сложение(A, B)
    Возврат A + B;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissedRequiredParameter,
            expect![[r#""#]],
        );
    }

    /// The parameter list a call is measured against must come from the body that would
    /// really answer it. This route is handed the bodies as a merged surface, so an
    /// unread one anywhere in it leaves the effective signature unknown — the other body
    /// may declare the method with entirely different parameters, and measuring against
    /// it accuses the caller of missing an argument nobody can show is required.
    #[test]
    fn an_unread_body_bars_the_signature_verdict_for_the_whole_module() {
        use crate::test_utils::check_with_cfe_unreadable;

        let code = "Процедура Тест() Экспорт\nСервер.П();\nКонецПроцедуры";
        let fixture = || {
            let mut builder = CfeFixtureBuilder::new("");
            builder
                .add_base_module("Сервер", "Процедура П(Обязательный) Экспорт КонецПроцедуры")
                .add_extension("Расш", "")
                .add_extension_module(
                    "Расш",
                    "Сервер",
                    "Процедура П(Обязательный) Экспорт КонецПроцедуры",
                );
            builder.build()
        };

        // Control: with every body readable the base signature is measured and the call
        // IS accused. Without it, the silence below would prove nothing.
        let control = check_with_cfe_unreadable(code, fixture(), &[]);
        assert!(
            control.iter().any(|d| d.code == DiagnosticCode::MissedRequiredParameter),
            "control: a readable base signature must produce the verdict, got {control:?}"
        );

        let unread =
            check_with_cfe_unreadable(code, fixture(), &["CommonModules/Сервер/Ext/Module.bsl"]);
        assert!(
            !unread.iter().any(|d| d.code == DiagnosticCode::MissedRequiredParameter),
            "an unread base body leaves the effective signature unknown, got {unread:?}"
        );
    }

    #[test]
    fn test_qualified_calls_skipped_without_metadata() {
        let code = r#"
Процедура Тест()
    ОбщийМодуль.Метод();
    Объект.Метод(1);
КонецПроцедуры

Функция Метод(A, B)
    Возврат A + B;
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissedRequiredParameter,
            expect![[r#""#]],
        );
    }
}
