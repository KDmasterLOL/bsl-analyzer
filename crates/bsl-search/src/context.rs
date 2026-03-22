//! Context enrichment for code chunks.
//!
//! Converts file paths to 1C module paths and builds enriched
//! text representations for better embedding quality.

use crate::chunker::{Chunk, ChunkKind};

/// Convert a relative file path to a 1C metadata module path.
///
/// Examples:
///   `Documents/Реализация/Ext/ObjectModule.bsl`
///     → `Документ.Реализация.МодульОбъекта`
///   `CommonModules/ОбщийМодульСервер/Ext/Module.bsl`
///     → `ОбщийМодуль.ОбщийМодульСервер`
///   `Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl`
///     → `Справочник.Номенклатура.Форма.ФормаЭлемента`
pub fn file_path_to_module_path(rel_path: &str) -> String {
    let path = rel_path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').collect();

    if parts.is_empty() {
        return rel_path.to_owned();
    }

    // Find the metadata type directory and object name.
    let mut result_parts = Vec::new();

    let mut i = 0;
    while i < parts.len() {
        if let Some(ru_type) = metadata_type_ru(parts[i]) {
            result_parts.push(ru_type.to_owned());
            // Next part is the object name.
            if i + 1 < parts.len() && parts[i + 1] != "Ext" {
                result_parts.push(parts[i + 1].to_owned());
                i += 2;

                // Check for Forms subdirectory.
                if i < parts.len() && parts[i] == "Forms" && i + 1 < parts.len() {
                    result_parts.push("Форма".to_owned());
                    result_parts.push(parts[i + 1].to_owned());
                    i += 2;
                }
                // Check for Commands subdirectory.
                if i < parts.len() && parts[i] == "Commands" && i + 1 < parts.len() {
                    result_parts.push("Команда".to_owned());
                    result_parts.push(parts[i + 1].to_owned());
                    i += 2;
                }
            }
        }
        i += 1;
    }

    // Append module type from filename.
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

/// Build enriched text for embedding.
///
/// Prepends metadata context to the chunk text so the embedding model
/// understands where this code belongs in the configuration.
pub fn enrich_chunk_text(chunk: &Chunk, module_path: &str) -> String {
    let mut lines = Vec::new();

    // Module path context.
    if !module_path.is_empty() {
        lines.push(format!("// Модуль: {module_path}"));
    }

    // Symbol signature context.
    match chunk.kind {
        ChunkKind::Procedure | ChunkKind::Function => {
            let kind_ru = match chunk.kind {
                ChunkKind::Procedure => "Процедура",
                ChunkKind::Function => "Функция",
                _ => unreachable!(),
            };

            let mut sig_parts = Vec::new();
            if chunk.is_export {
                sig_parts.push("экспорт");
            }
            for ann in &chunk.annotations {
                sig_parts.push(ann);
            }

            if sig_parts.is_empty() {
                lines.push(format!("// {kind_ru} {}", chunk.name));
            } else {
                lines.push(format!("// {kind_ru} {} ({})", chunk.name, sig_parts.join(", ")));
            }
        }
        ChunkKind::ModuleHeader => {
            lines.push("// Заголовок модуля".to_owned());
        }
    }

    lines.push(chunk.text.clone());
    lines.join("\n")
}

/// Map EDT directory names to Russian metadata type names.
fn metadata_type_ru(dir_name: &str) -> Option<&'static str> {
    match dir_name {
        "Documents" => Some("Документ"),
        "Catalogs" => Some("Справочник"),
        "CommonModules" => Some("ОбщийМодуль"),
        "DataProcessors" => Some("Обработка"),
        "Reports" => Some("Отчет"),
        "InformationRegisters" => Some("РегистрСведений"),
        "AccumulationRegisters" => Some("РегистрНакопления"),
        "AccountingRegisters" => Some("РегистрБухгалтерии"),
        "CalculationRegisters" => Some("РегистрРасчета"),
        "Enums" => Some("Перечисление"),
        "Constants" => Some("Константа"),
        "ChartsOfCharacteristicTypes" => Some("ПланВидовХарактеристик"),
        "ChartsOfAccounts" => Some("ПланСчетов"),
        "ChartsOfCalculationTypes" => Some("ПланВидовРасчета"),
        "BusinessProcesses" => Some("БизнесПроцесс"),
        "Tasks" => Some("Задача"),
        "ExchangePlans" => Some("ПланОбмена"),
        "WebServices" => Some("WebСервис"),
        "HTTPServices" => Some("HTTPСервис"),
        "FilterCriteria" => Some("КритерийОтбора"),
        "SettingsStorages" => Some("ХранилищеНастроек"),
        "FunctionalOptions" => Some("ФункциональнаяОпция"),
        "CommonForms" => Some("ОбщаяФорма"),
        "CommonCommands" => Some("ОбщаяКоманда"),
        "SessionParameters" => Some("ПараметрСеанса"),
        "Sequences" => Some("Последовательность"),
        "DocumentJournals" => Some("ЖурналДокументов"),
        _ => None,
    }
}

/// Map BSL module file names to Russian module type names.
fn module_type_ru(file_name: &str) -> Option<&'static str> {
    match file_name {
        "ObjectModule.bsl" => Some("МодульОбъекта"),
        "ManagerModule.bsl" => Some("МодульМенеджера"),
        "FormModule.bsl" => Some("МодульФормы"),
        "Module.bsl" => Some("Модуль"),
        "CommandModule.bsl" => Some("МодульКоманды"),
        "RecordSetModule.bsl" => Some("МодульНабораЗаписей"),
        "ValueManagerModule.bsl" => Some("МодульМенеджераЗначения"),
        "SessionModule.bsl" => Some("МодульСеанса"),
        "ExternalConnectionModule.bsl" => Some("МодульВнешнегоСоединения"),
        "ManagedApplicationModule.bsl" => Some("МодульУправляемогоПриложения"),
        "OrdinaryApplicationModule.bsl" => Some("МодульОбычногоПриложения"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn enrich_procedure() {
        let chunk = Chunk {
            kind: ChunkKind::Procedure,
            name: "ОбработкаПроведения".to_owned(),
            is_export: true,
            annotations: vec!["&НаСервере".to_owned()],
            line_start: 0,
            line_end: 5,
            text: "Процедура ОбработкаПроведения(Отказ)\nКонецПроцедуры".to_owned(),
        };

        let enriched = enrich_chunk_text(&chunk, "Документ.Реализация.МодульОбъекта");
        assert!(enriched.contains("// Модуль: Документ.Реализация.МодульОбъекта"));
        assert!(enriched.contains("// Процедура ОбработкаПроведения (экспорт, &НаСервере)"));
        assert!(enriched.contains("Процедура ОбработкаПроведения(Отказ)"));
    }

    #[test]
    fn enrich_header() {
        let chunk = Chunk {
            kind: ChunkKind::ModuleHeader,
            name: String::new(),
            is_export: false,
            annotations: Vec::new(),
            line_start: 0,
            line_end: 2,
            text: "Перем А;".to_owned(),
        };

        let enriched = enrich_chunk_text(&chunk, "ОбщийМодуль.Сервер");
        assert!(enriched.contains("// Модуль: ОбщийМодуль.Сервер"));
        assert!(enriched.contains("// Заголовок модуля"));
    }
}
