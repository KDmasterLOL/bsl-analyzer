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
    filter: Option<String>,
) -> Result<CallToolResult, McpError> {
    if let Some(ref category) = filter {
        format_filtered_tree(config, category)
    } else {
        format_summary_tree(config)
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
pub fn get_configuration_info(config: &Configuration) -> Result<CallToolResult, McpError> {
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

    Ok(CallToolResult::success(vec![Content::text(out)]))
}

/// Returns form structure for a metadata object.
pub fn get_form_structure(
    _object_type: &str,
    _object_name: &str,
    _form_name: Option<&str>,
) -> Result<CallToolResult, McpError> {
    // Forms require parsing individual Form.xml files from the configuration directory.
    // Full implementation requires extending SharedState with form cache.
    Err(McpError::invalid_params(
        "get_form_structure пока не реализован: \
         требуется кэш форм из файловой системы конфигурации. \
         Будет доступен после интеграции с загрузчиком форм.",
        None,
    ))
}
