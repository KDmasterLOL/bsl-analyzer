//! Metadata tools: configuration tree, object structure, forms.

use bsl_metadata::metadata_object::MdoType;
use bsl_metadata::traits::MdObject;
use bsl_metadata::Configuration;
use rmcp::model::{CallToolResult, Content};
use rmcp::ErrorData as McpError;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Returns configuration metadata tree — categories and object names.
pub fn get_metadata_tree(
    config: &Configuration,
    extensions: &[(String, Configuration)],
    filter: Option<String>,
) -> Result<CallToolResult, McpError> {
    if let Some(ref category) = filter {
        format_filtered_tree(config, category)
    } else {
        let mut result = format_summary_tree(config)?;
        // Append extension summaries
        if !extensions.is_empty() {
            let text = result.content[0].raw.as_text().expect("text").text.clone();
            let mut out = text;
            for (name, ext_config) in extensions {
                out.push_str(&format_extension_summary(name, ext_config));
            }
            result = CallToolResult::success(vec![Content::text(out)]);
        }
        Ok(result)
    }
}

fn format_summary_tree(config: &Configuration) -> Result<CallToolResult, McpError> {
    let mut categories: BTreeMap<&str, usize> = BTreeMap::new();

    for obj in config.metadata_objects() {
        let key = obj.mdo_type.russian_name();
        *categories.entry(key).or_default() += 1;
    }

    for reg in config.registers() {
        let key = reg.mdo_type().russian_name();
        *categories.entry(key).or_default() += 1;
    }

    let common_modules = config.common_modules().len();
    if common_modules > 0 {
        categories.insert("ОбщийМодуль", common_modules);
    }

    let event_subs = config.event_subscriptions().len();
    if event_subs > 0 {
        categories.insert("ПодпискаНаСобытие", event_subs);
    }

    let defined_types = config.defined_types().len();
    if defined_types > 0 {
        categories.insert("ОпределяемыйТип", defined_types);
    }

    let scheduled_jobs = config.scheduled_jobs().len();
    if scheduled_jobs > 0 {
        categories.insert("РегламентноеЗадание", scheduled_jobs);
    }

    let roles = config.roles().len();
    if roles > 0 {
        categories.insert("Роль", roles);
    }

    let http_services = config.http_services().len();
    if http_services > 0 {
        categories.insert("HTTPСервис", http_services);
    }

    let web_services = config.web_services().len();
    if web_services > 0 {
        categories.insert("WebСервис", web_services);
    }

    let total: usize = categories.values().sum();
    let mut out = format!("# {} — дерево метаданных\n\n", config.name());
    let _ = writeln!(out, "Всего объектов: {total}\n");
    let _ = writeln!(out, "| Категория | Количество |");
    let _ = writeln!(out, "|-----------|------------|");
    for (category, count) in &categories {
        let _ = writeln!(out, "| {category} | {count} |");
    }
    let _ = writeln!(out, "\nИспользуйте `filter` для получения списка объектов категории.");

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn format_filtered_tree(
    config: &Configuration,
    category: &str,
) -> Result<CallToolResult, McpError> {
    let mut out = String::new();
    let mut found = false;

    // Try to match as MdoType (both Russian and English)
    if let Ok(mdo_type) = category.parse::<MdoType>() {
        let objects: Vec<_> =
            config.metadata_objects().iter().filter(|o| o.mdo_type == mdo_type).collect();

        if !objects.is_empty() {
            found = true;
            let _ = writeln!(out, "# {} ({})\n", mdo_type.russian_name(), objects.len());
            for obj in &objects {
                if let Some(ref name_en) = obj.name_en {
                    let _ = writeln!(out, "- {} ({name_en})", obj.name);
                } else {
                    let _ = writeln!(out, "- {}", obj.name);
                }
            }
        }

        let registers: Vec<_> =
            config.registers().iter().filter(|r| r.mdo_type() == mdo_type).collect();

        if !registers.is_empty() {
            found = true;
            let _ = writeln!(out, "# {} ({})\n", mdo_type.russian_name(), registers.len());
            for reg in &registers {
                let _ = writeln!(out, "- {}", reg.name());
            }
        }
    }

    // Special categories not in MdoType
    if !found {
        match category.to_lowercase().as_str() {
            "общиемодули" | "общиймодуль" | "commonmodule" | "commonmodules" =>
            {
                let modules = config.common_modules();
                if !modules.is_empty() {
                    found = true;
                    let _ = writeln!(out, "# ОбщиеМодули ({})\n", modules.len());
                    for m in modules {
                        let flags = format_common_module_flags(m);
                        let _ = writeln!(out, "- {} {flags}", m.name());
                    }
                }
            }
            "подписканасобытие" | "eventsubscription" | "eventsubscriptions" => {
                let subs = config.event_subscriptions();
                if !subs.is_empty() {
                    found = true;
                    let _ = writeln!(out, "# ПодпискиНаСобытия ({})\n", subs.len());
                    for s in subs {
                        let _ = writeln!(out, "- {}", s.name());
                    }
                }
            }
            "определяемыйтип" | "definedtype" | "definedtypes" => {
                let types = config.defined_types();
                if !types.is_empty() {
                    found = true;
                    let _ = writeln!(out, "# ОпределяемыеТипы ({})\n", types.len());
                    for t in types {
                        let _ = writeln!(out, "- {}", t.name());
                    }
                }
            }
            "роль" | "role" | "roles" => {
                let roles = config.roles();
                if !roles.is_empty() {
                    found = true;
                    let _ = writeln!(out, "# Роли ({})\n", roles.len());
                    for r in roles {
                        let _ = writeln!(out, "- {}", r.name());
                    }
                }
            }
            _ => {}
        }
    }

    if !found {
        return Err(McpError::invalid_params(
            format!("Категория '{category}' не найдена. Вызовите get_metadata_tree без фильтра для списка категорий."),
            None,
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

fn format_common_module_flags(m: &bsl_metadata::CommonModule) -> String {
    let mut flags = Vec::new();
    if m.is_server() {
        flags.push("Сервер");
    }
    if m.is_client_managed_application() {
        flags.push("Клиент");
    }
    if m.is_global() {
        flags.push("Глобальный");
    }
    if m.is_privileged() {
        flags.push("Привилегированный");
    }
    if m.is_server_call() {
        flags.push("ВызовСервера");
    }
    if flags.is_empty() {
        String::new()
    } else {
        format!("[{}]", flags.join(", "))
    }
}

/// Returns detailed structure of a metadata object.
pub fn get_object_structure(
    config: &Configuration,
    object_type: &str,
    object_name: &str,
) -> Result<CallToolResult, McpError> {
    let mdo_type: MdoType =
        object_type.parse().map_err(|e: String| McpError::invalid_params(e, None))?;

    if let Some(obj) = config.find_metadata_object(mdo_type, object_name) {
        return Ok(CallToolResult::success(vec![Content::text(format_metadata_object_structure(
            obj, mdo_type,
        ))]));
    }

    if let Some(reg) = config.find_register_by_type_and_name(mdo_type, object_name) {
        return Ok(CallToolResult::success(vec![Content::text(format_register_structure(reg))]));
    }

    Err(McpError::invalid_params(
        format!("Объект {}.{} не найден в конфигурации", mdo_type.russian_name(), object_name),
        None,
    ))
}

fn format_metadata_object_structure(
    obj: &bsl_metadata::MetadataObject,
    mdo_type: MdoType,
) -> String {
    let mut out = format!("# {}.{}\n\n", mdo_type.russian_name(), obj.name);

    if let Some(ref name_en) = obj.name_en {
        let _ = writeln!(out, "Английское имя: {name_en}\n");
    }

    if !obj.attributes.is_empty() {
        let _ = writeln!(out, "## Реквизиты ({})\n", obj.attributes.len());
        let _ = writeln!(out, "| Имя | Тип |");
        let _ = writeln!(out, "|-----|-----|");
        for attr in &obj.attributes {
            let _ = writeln!(out, "| {} | {} |", attr.name, attr.attr_type);
        }
        out.push('\n');
    }

    if !obj.tabular_sections.is_empty() {
        let _ = writeln!(out, "## Табличные части ({})\n", obj.tabular_sections.len());
        for ts in &obj.tabular_sections {
            let _ = writeln!(out, "### {}\n", ts.name());
            if !ts.attributes().is_empty() {
                let _ = writeln!(out, "| Имя | Тип |");
                let _ = writeln!(out, "|-----|-----|");
                for attr in ts.attributes() {
                    let _ = writeln!(out, "| {} | {} |", attr.name(), attr.attr_type());
                }
                out.push('\n');
            }
        }
    }

    if !obj.enum_values.is_empty() {
        let _ = writeln!(out, "## Значения перечисления ({})\n", obj.enum_values.len());
        for val in &obj.enum_values {
            if let Some(ref name_en) = val.name_en {
                let _ = writeln!(out, "- {} ({name_en})", val.name);
            } else {
                let _ = writeln!(out, "- {}", val.name);
            }
        }
        out.push('\n');
    }

    if !obj.predefined_items.is_empty() {
        let _ = writeln!(out, "## Предопределённые элементы ({})\n", obj.predefined_items.len());
        for item in &obj.predefined_items {
            let _ = writeln!(out, "- {}", item.name);
        }
    }

    out
}

fn format_register_structure(reg: &bsl_metadata::Register) -> String {
    let mut out = format!("# {}.{}\n\n", reg.mdo_type().russian_name(), reg.name());

    if !reg.dimensions().is_empty() {
        let _ = writeln!(out, "## Измерения ({})\n", reg.dimensions().len());
        let _ = writeln!(out, "| Имя | Тип |");
        let _ = writeln!(out, "|-----|-----|");
        for dim in reg.dimensions() {
            let type_str = dim
                .attr_type()
                .map(|t| t.to_string())
                .unwrap_or_else(|| dim.type_str().to_string());
            let _ = writeln!(out, "| {} | {type_str} |", dim.name());
        }
        out.push('\n');
    }

    if !reg.resources().is_empty() {
        let _ = writeln!(out, "## Ресурсы ({})\n", reg.resources().len());
        let _ = writeln!(out, "| Имя | Тип |");
        let _ = writeln!(out, "|-----|-----|");
        for res in reg.resources() {
            let type_str = res
                .attr_type()
                .map(|t| t.to_string())
                .unwrap_or_else(|| res.type_str().to_string());
            let _ = writeln!(out, "| {} | {type_str} |", res.name());
        }
        out.push('\n');
    }

    if !reg.attributes().is_empty() {
        let _ = writeln!(out, "## Реквизиты ({})\n", reg.attributes().len());
        let _ = writeln!(out, "| Имя | Тип |");
        let _ = writeln!(out, "|-----|-----|");
        for attr in reg.attributes() {
            let type_str = attr
                .attr_type()
                .map(|t| t.to_string())
                .unwrap_or_else(|| attr.type_str().to_string());
            let _ = writeln!(out, "| {} | {type_str} |", attr.name());
        }
        out.push('\n');
    }

    if let Some(periodicity) = reg.periodicity() {
        let _ = writeln!(out, "Периодичность: {periodicity:?}");
    }
    if let Some(register_type) = reg.register_type() {
        let _ = writeln!(out, "Вид регистра: {register_type:?}");
    }

    out
}

/// Returns general configuration info.
pub fn get_configuration_info(
    config: &Configuration,
    extensions: &[(String, Configuration)],
) -> Result<CallToolResult, McpError> {
    let total_objects = config.metadata_objects().len()
        + config.registers().len()
        + config.common_modules().len()
        + config.event_subscriptions().len()
        + config.defined_types().len()
        + config.scheduled_jobs().len()
        + config.roles().len()
        + config.http_services().len()
        + config.web_services().len();

    let mut out = format!("# Конфигурация: {}\n\n", config.name());
    let _ = writeln!(out, "- UUID: {}", config.uuid());
    let _ = writeln!(out, "- Всего объектов метаданных: {total_objects}");
    let _ = writeln!(out, "- Общие модули: {}", config.common_modules().len());
    let _ = writeln!(out, "- Объекты метаданных: {}", config.metadata_objects().len());
    let _ = writeln!(out, "- Регистры: {}", config.registers().len());
    let _ = writeln!(out, "- Подписки на события: {}", config.event_subscriptions().len());
    let _ = writeln!(out, "- Определяемые типы: {}", config.defined_types().len());
    let _ = writeln!(out, "- Регламентные задания: {}", config.scheduled_jobs().len());
    let _ = writeln!(out, "- Роли: {}", config.roles().len());
    let _ = writeln!(out, "- HTTP-сервисы: {}", config.http_services().len());
    let _ = writeln!(out, "- Web-сервисы: {}", config.web_services().len());

    if !extensions.is_empty() {
        let _ = writeln!(out, "\n## Расширения ({})\n", extensions.len());
        for (name, ext_config) in extensions {
            let ext_objects = ext_config.metadata_objects().len()
                + ext_config.registers().len()
                + ext_config.common_modules().len();
            let _ = writeln!(out, "- **{name}**: {ext_objects} объектов");
        }
    }

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Returns form structure for a metadata object.
///
/// Loads forms on-demand from the configuration directory.
/// Path convention: `{TypeDir}/{ObjectName}/Forms/{FormName}/Ext/Form.xml`
pub fn get_form_structure(
    workspace_root: Option<&std::path::Path>,
    object_type: &str,
    object_name: &str,
    form_name: Option<&str>,
) -> Result<CallToolResult, McpError> {
    let root = workspace_root.ok_or_else(|| {
        McpError::invalid_params("Workspace root не задан, формы недоступны", None)
    })?;

    let type_dir = mdo_type_to_dir(object_type).ok_or_else(|| {
        McpError::invalid_params(format!("Неизвестный тип объекта: {object_type}"), None)
    })?;

    let forms_dir = root.join(type_dir).join(object_name).join("Forms");
    if !forms_dir.exists() {
        return Err(McpError::invalid_params(
            format!("Каталог форм не найден: {}", forms_dir.display()),
            None,
        ));
    }

    if let Some(fname) = form_name {
        // Load specific form
        let form_xml_path = forms_dir.join(fname).join("Ext").join("Form.xml");
        if !form_xml_path.exists() {
            return Err(McpError::invalid_params(
                format!("Форма не найдена: {}", form_xml_path.display()),
                None,
            ));
        }
        let xml = std::fs::read_to_string(&form_xml_path)
            .map_err(|e| McpError::internal_error(format!("Ошибка чтения формы: {e}"), None))?;
        let form = bsl_metadata::xml_parser::parse_form_xml(&xml)
            .map_err(|e| McpError::internal_error(format!("Ошибка разбора формы: {e}"), None))?;
        Ok(CallToolResult::success(vec![Content::text(format_form(&form))]))
    } else {
        // List available forms
        let mut form_names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&forms_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        form_names.push(name.to_string());
                    }
                }
            }
        }
        form_names.sort();

        if form_names.is_empty() {
            return Err(McpError::invalid_params(
                format!("Формы не найдены для {object_type}.{object_name}"),
                None,
            ));
        }

        let mut out = format!("# Формы {object_type}.{object_name}\n\n");
        for name in &form_names {
            let _ = writeln!(out, "- {name}");
        }
        let _ =
            writeln!(out, "\nИспользуйте `form_name` для получения структуры конкретной формы.");
        Ok(CallToolResult::success(vec![Content::text(out)]))
    }
}

fn format_form(form: &bsl_metadata::Form) -> String {
    let mut out = format!("# Форма: {}\n\n", form.name());
    let _ = writeln!(out, "- Тип: {:?}", form.form_type());
    let _ = writeln!(out, "- UUID: {}", form.uuid());

    if !form.attributes.is_empty() {
        let _ = writeln!(out, "\n## Реквизиты формы ({})\n", form.attributes.len());
        for attr in &form.attributes {
            let main_marker = if attr.is_main { " (основной)" } else { "" };
            let _ = writeln!(out, "- {}{}: {}", attr.name, main_marker, attr.attr_type);
            for col in &attr.columns {
                let _ = writeln!(out, "    - {}: {}", col.name, col.attr_type);
            }
        }
    }

    let elements = form.elements();
    if !elements.is_empty() {
        let _ = writeln!(out, "\n## Элементы ({})\n", elements.len());
        let _ = writeln!(out, "| Имя | DataPath |");
        let _ = writeln!(out, "|-----|----------|");
        for el in elements {
            let dp = el.data_path.as_deref().unwrap_or("—");
            let _ = writeln!(out, "| {} | {dp} |", el.name);
        }
    }

    if !form.event_handlers.is_empty() {
        let _ = writeln!(out, "\n## Обработчики событий ({})\n", form.event_handlers.len());
        for h in &form.event_handlers {
            let _ = writeln!(out, "- {}", h.handler_name);
        }
    }

    if !form.command_handlers.is_empty() {
        let _ = writeln!(out, "\n## Обработчики команд ({})\n", form.command_handlers.len());
        for h in &form.command_handlers {
            let _ = writeln!(out, "- {h}");
        }
    }

    out
}

fn format_extension_summary(name: &str, config: &Configuration) -> String {
    use std::collections::BTreeMap;
    let mut out = format!("\n---\n\n# Расширение: {name}\n\n");
    let mut categories: BTreeMap<&str, usize> = BTreeMap::new();

    for obj in config.metadata_objects() {
        let key = obj.mdo_type.russian_name();
        *categories.entry(key).or_default() += 1;
    }
    for reg in config.registers() {
        let key = reg.mdo_type().russian_name();
        *categories.entry(key).or_default() += 1;
    }
    let common_modules = config.common_modules().len();
    if common_modules > 0 {
        categories.insert("ОбщийМодуль", common_modules);
    }

    let total: usize = categories.values().sum();
    let _ = writeln!(out, "Всего объектов: {total}\n");
    let _ = writeln!(out, "| Категория | Количество |");
    let _ = writeln!(out, "|-----------|------------|");
    for (category, count) in &categories {
        let _ = writeln!(out, "| {category} | {count} |");
    }
    out
}

#[cfg(test)]
fn fixture_config() -> Configuration {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
    bsl_metadata::load_from_directory(path).expect("failed to load fixture configuration")
}

fn mdo_type_to_dir(object_type: &str) -> Option<&'static str> {
    match object_type.to_lowercase().as_str() {
        "catalog" | "справочник" => Some("Catalogs"),
        "document" | "документ" => Some("Documents"),
        "dataprocessor" | "обработка" => Some("DataProcessors"),
        "report" | "отчет" | "отчёт" => Some("Reports"),
        "enum" | "перечисление" => Some("Enums"),
        "chartofcharacteristictypes" | "планвидовхарактеристик" => {
            Some("ChartsOfCharacteristicTypes")
        }
        "chartofaccounts" | "плансчетов" => Some("ChartsOfAccounts"),
        "exchangeplan" | "планобмена" => Some("ExchangePlans"),
        "businessprocess" | "бизнеспроцесс" => Some("BusinessProcesses"),
        "task" | "задача" => Some("Tasks"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_text(result: &CallToolResult) -> &str {
        result.content[0].raw.as_text().expect("expected text content").text.as_str()
    }

    #[test]
    fn test_metadata_tree_summary() {
        let config = fixture_config();
        let result = get_metadata_tree(&config, &[], None).unwrap();
        let text = extract_text(&result);

        assert!(text.contains("дерево метаданных"), "should have header");
        assert!(text.contains("Всего объектов:"), "should have total");
        assert!(text.contains("Справочник"), "should list catalogs");
        assert!(text.contains("Документ"), "should list documents");
    }

    #[test]
    fn test_metadata_tree_filter_catalogs() {
        let config = fixture_config();
        let result = get_metadata_tree(&config, &[], Some("Справочник".into())).unwrap();
        let text = extract_text(&result);

        assert!(text.contains("Справочник"), "should have category name");
        assert!(text.contains("Справочник1"), "should list Справочник1");
    }

    #[test]
    fn test_metadata_tree_filter_common_modules() {
        let config = fixture_config();
        let result = get_metadata_tree(&config, &[], Some("ОбщиеМодули".into())).unwrap();
        let text = extract_text(&result);

        assert!(text.contains("ОбщиеМодули"), "should have category name");
    }

    #[test]
    fn test_metadata_tree_filter_invalid() {
        let config = fixture_config();
        let result = get_metadata_tree(&config, &[], Some("НесуществующаяКатегория".into()));

        assert!(result.is_err(), "should return error for unknown category");
    }

    #[test]
    fn test_object_structure_catalog() {
        let config = fixture_config();
        let result = get_object_structure(&config, "Catalog", "Справочник1").unwrap();
        let text = extract_text(&result);

        assert!(text.contains("Справочник.Справочник1"), "should have object header");
    }

    #[test]
    fn test_object_structure_not_found() {
        let config = fixture_config();
        let result = get_object_structure(&config, "Catalog", "НесуществующийОбъект");

        assert!(result.is_err(), "should return error for missing object");
    }

    #[test]
    fn test_object_structure_invalid_type() {
        let config = fixture_config();
        let result = get_object_structure(&config, "InvalidType", "Test");

        assert!(result.is_err(), "should return error for invalid type");
    }

    #[test]
    fn test_configuration_info() {
        let config = fixture_config();
        let result = get_configuration_info(&config, &[]).unwrap();
        let text = extract_text(&result);

        assert!(text.contains("# Конфигурация:"), "should have config header");
        assert!(text.contains("UUID:"), "should have UUID");
        assert!(text.contains("Общие модули:"), "should have common modules count");
        assert!(text.contains("Регистры:"), "should have registers count");
    }

    #[test]
    fn test_form_structure_list_forms() {
        let fixture_root =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let root = std::path::Path::new(fixture_root);
        let result = get_form_structure(Some(root), "Document", "Документ1", None).unwrap();
        let text = extract_text(&result);

        assert!(text.contains("Формы Document.Документ1"), "should have forms header");
        assert!(text.contains("ФормаДокумента"), "should list document form");
        assert!(text.contains("ФормаСписка"), "should list list form");
    }

    #[test]
    fn test_form_structure_specific_form() {
        let fixture_root =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let root = std::path::Path::new(fixture_root);
        let result =
            get_form_structure(Some(root), "Document", "Документ1", Some("ФормаДокумента"))
                .unwrap();
        let text = extract_text(&result);

        assert!(text.contains("# Форма:"), "should have form header");
    }

    #[test]
    fn test_form_structure_no_workspace() {
        let result = get_form_structure(None, "Catalog", "Test", None);
        assert!(result.is_err(), "should fail without workspace root");
    }

    #[test]
    fn test_form_structure_form_not_found() {
        let fixture_root =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");
        let root = std::path::Path::new(fixture_root);
        let result =
            get_form_structure(Some(root), "Document", "Документ1", Some("НесуществующаяФорма"));

        assert!(result.is_err(), "should fail for missing form");
    }
}
