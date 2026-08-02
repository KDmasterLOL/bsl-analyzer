pub fn file_path_to_module_path(rel_path: &str) -> String {
    let path = rel_path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();

    if parts.is_empty() {
        return rel_path.to_owned();
    }

    let mut result_parts = Vec::new();

    let mut i = 0;
    while i < parts.len() {
        if let Some(ru_type) = metadata_type_ru(parts[i]) {
            use bsl_conventions::{conventional_of, ConventionalName as Conv};
            result_parts.push(ru_type.to_owned());
            // `parts[i + 1]` — позиция ИМЕНИ объекта: сравнение с `Ext` здесь
            // точное сознательно, объект может называться `EXT` и остаётся собой.
            if i + 1 < parts.len() && parts[i + 1] != "Ext" {
                result_parts.push(parts[i + 1].to_owned());
                i += 2;

                if i < parts.len()
                    && conventional_of(parts[i]) == Some(Conv::Forms)
                    && i + 1 < parts.len()
                {
                    result_parts.push("Форма".to_owned());
                    result_parts.push(parts[i + 1].to_owned());
                    i += 2;
                }
                if i < parts.len()
                    && conventional_of(parts[i]) == Some(Conv::Commands)
                    && i + 1 < parts.len()
                {
                    result_parts.push("Команда".to_owned());
                    result_parts.push(parts[i + 1].to_owned());
                    i += 2;
                }
            }
        }
        i += 1;
    }

    if let Some(&last) = parts.last() {
        if let Some(module_type) = module_type_ru(last) {
            result_parts.push(module_type.to_owned());
        }
    }

    if result_parts.is_empty() {
        return rel_path.to_owned();
    }

    result_parts.join(".")
}

fn metadata_type_ru(dir_name: &str) -> Option<&'static str> {
    // Английские имена коллекций выгрузки; регистр не значим. Двуязычная
    // эквивалентность (русские коллекции) — политика спеки модульных путей,
    // сюда не тянется: таблица покрывает только английскую раскладку.
    const TABLE: &[(&str, &str)] = &[
        ("Documents", "Документ"),
        ("Catalogs", "Справочник"),
        ("CommonModules", "ОбщийМодуль"),
        ("DataProcessors", "Обработка"),
        ("Reports", "Отчет"),
        ("InformationRegisters", "РегистрСведений"),
        ("AccumulationRegisters", "РегистрНакопления"),
        ("AccountingRegisters", "РегистрБухгалтерии"),
        ("CalculationRegisters", "РегистрРасчета"),
        ("Enums", "Перечисление"),
        ("Constants", "Константа"),
        ("ChartsOfCharacteristicTypes", "ПланВидовХарактеристик"),
        ("ChartsOfAccounts", "ПланСчетов"),
        ("ChartsOfCalculationTypes", "ПланВидовРасчета"),
        ("BusinessProcesses", "БизнесПроцесс"),
        ("Tasks", "Задача"),
        ("ExchangePlans", "ПланОбмена"),
        ("WebServices", "WebСервис"),
        ("HTTPServices", "HTTPСервис"),
        ("FilterCriteria", "КритерийОтбора"),
        ("SettingsStorages", "ХранилищеНастроек"),
        ("FunctionalOptions", "ФункциональнаяОпция"),
        ("CommonForms", "ОбщаяФорма"),
        ("CommonCommands", "ОбщаяКоманда"),
        ("SessionParameters", "ПараметрСеанса"),
        ("Sequences", "Последовательность"),
        ("DocumentJournals", "ЖурналДокументов"),
    ];
    TABLE.iter().find(|(k, _)| k.eq_ignore_ascii_case(dir_name)).map(|(_, v)| *v)
}

fn module_type_ru(file_name: &str) -> Option<&'static str> {
    use bsl_conventions::ConventionalName as Conv;
    match bsl_conventions::conventional_of(file_name)? {
        Conv::ObjectModule => Some("МодульОбъекта"),
        Conv::ManagerModule => Some("МодульМенеджера"),
        Conv::FormModule => Some("МодульФормы"),
        Conv::Module => Some("Модуль"),
        Conv::CommandModule => Some("МодульКоманды"),
        Conv::RecordSetModule => Some("МодульНабораЗаписей"),
        Conv::ValueManagerModule => Some("МодульМенеджераЗначения"),
        Conv::SessionModule => Some("МодульСеанса"),
        Conv::ExternalConnectionModule => Some("МодульВнешнегоСоединения"),
        Conv::ManagedApplicationModule => Some("МодульУправляемогоПриложения"),
        Conv::OrdinaryApplicationModule => Some("МодульОбычногоПриложения"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_case_variant_spelling_maps_like_its_canonical_twin() {
        for (variant, canonical) in [
            (
                "Documents/Реализация/EXT/OBJECTMODULE.BSL",
                "Documents/Реализация/Ext/ObjectModule.bsl",
            ),
            (
                "CATALOGS/Товары/FORMS/Ф/Ext/FormModule.bsl",
                "Catalogs/Товары/Forms/Ф/Ext/FormModule.bsl",
            ),
            ("CommonModules/Общий/EXT/MODULE.BSL", "CommonModules/Общий/Ext/Module.bsl"),
        ] {
            assert_eq!(
                file_path_to_module_path(variant),
                file_path_to_module_path(canonical),
                "{variant}"
            );
        }
    }

    /// Позиция после коллекции — ИМЯ объекта: объект, названный `EXT`, остаётся
    /// в semantic text под своим именем в любом регистре.
    #[test]
    fn an_object_named_ext_keeps_its_name() {
        assert_eq!(
            file_path_to_module_path("Catalogs/EXT/Ext/ObjectModule.bsl"),
            "Справочник.EXT.МодульОбъекта"
        );
    }

    /// Таблица по словарю: верхнерегистровое написание каждого конвенционного
    /// имени модуля даёт тот же ответ, что каноническое (включая None).
    #[test]
    fn every_dictionary_module_name_maps_case_insensitively() {
        for &name in bsl_conventions::ConventionalName::ALL {
            let canonical = name.canonical();
            if !canonical.ends_with(".bsl") {
                continue;
            }
            assert_eq!(
                module_type_ru(&canonical.to_ascii_uppercase()),
                module_type_ru(canonical),
                "{canonical}"
            );
        }
    }

    #[test]
    fn document_object_module() {
        assert_eq!(
            file_path_to_module_path("Documents/Реализация/Ext/ObjectModule.bsl"),
            "Документ.Реализация.МодульОбъекта"
        );
    }

    #[test]
    fn common_module() {
        assert_eq!(
            file_path_to_module_path("CommonModules/ОбщийМодульСервер/Ext/Module.bsl"),
            "ОбщийМодуль.ОбщийМодульСервер.Модуль"
        );
    }

    #[test]
    fn catalog_manager() {
        assert_eq!(
            file_path_to_module_path("Catalogs/Номенклатура/Ext/ManagerModule.bsl"),
            "Справочник.Номенклатура.МодульМенеджера"
        );
    }

    #[test]
    fn form_module() {
        assert_eq!(
            file_path_to_module_path(
                "Documents/Реализация/Forms/ФормаДокумента/Ext/Form/Module.bsl"
            ),
            "Документ.Реализация.Форма.ФормаДокумента.Модуль"
        );
    }

    #[test]
    fn register_module() {
        assert_eq!(
            file_path_to_module_path("InformationRegisters/ОстаткиТоваров/Ext/RecordSetModule.bsl"),
            "РегистрСведений.ОстаткиТоваров.МодульНабораЗаписей"
        );
    }

    #[test]
    fn unknown_path_returned_as_is() {
        assert_eq!(file_path_to_module_path("some/random/path.bsl"), "some/random/path.bsl");
    }

    #[test]
    fn windows_path_separators() {
        assert_eq!(
            file_path_to_module_path("Documents\\Реализация\\Ext\\ObjectModule.bsl"),
            "Документ.Реализация.МодульОбъекта"
        );
    }
}
