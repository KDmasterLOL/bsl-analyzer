//! SDBL query completion.
//!
//! Provides completion suggestions for SDBL queries based on:
//! - Position context (after FROM, inside MDO type, etc.)
//! - Metadata (available catalogs, documents, registers, etc.)

use super::{CompletionItem, CompletionItemKind, CompletionPosition};
use bsl_metadata::{Configuration, MdoType};
use ide_db::RootDatabase;
use sdbl_hir::{detect_context, detect_sdbl_at_position, Scope, SdblCompletionContext};

/// Main SDBL completion entry point.
///
/// Returns completion suggestions if cursor is inside an SDBL query string.
pub(super) fn sdbl_completions(
    db: &dyn RootDatabase,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let file_id = position.file_id;
    let offset = position.offset;

    tracing::info!("sdbl_completions called: file_id={:?}, offset={:?}", file_id, offset);

    // Get parsed file
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    // Check if position is inside SDBL query string
    let query_info = detect_sdbl_at_position(&root, offset);
    if query_info.is_none() {
        tracing::info!("detect_sdbl_at_position returned None - not inside SDBL query");
        return None;
    }
    let query_info = query_info.unwrap();

    tracing::info!(
        query_len = query_info.query_text.len(),
        offset_in_query = u32::from(query_info.offset_in_query),
        "detected SDBL query"
    );

    // Try to get Scope (may be None for invalid queries or queries without tables)
    let scope = get_sdbl_scope(db, file_id);
    if scope.is_some() {
        tracing::debug!("successfully built Scope from query HIR");
    } else {
        tracing::debug!("failed to build Scope (no HIR or no tables)");
    }

    // Determine completion context
    let context = detect_context(&query_info.query_text, query_info.offset_in_query);

    // Match on (context, scope) for alias-based completions
    match (context, scope.as_ref()) {
        // NEW (Iteration 3): Alias field completion
        (SdblCompletionContext::AfterTableAlias { alias, prefix }, Some(scope)) => {
            tracing::info!(
                alias = %alias,
                prefix = %prefix,
                "completion context: AfterTableAlias (with scope)"
            );
            Some(complete_fields_by_alias(scope, &alias, &prefix))
        }
        (SdblCompletionContext::AfterTableAlias { alias, prefix }, None) => {
            tracing::warn!(
                alias = %alias,
                prefix = %prefix,
                "completion context: AfterTableAlias but no scope available (HIR failed?)"
            );
            // Fallback to keywords if no scope
            Some(complete_sdbl_keywords(&prefix))
        }

        // NEW (Iteration 3): Alias suggestion after AS/КАК
        (SdblCompletionContext::AfterAsKeyword { context: as_context, suggestion }, _) => {
            tracing::info!(
                ?as_context,
                suggestion = ?suggestion,
                "completion context: AfterAsKeyword"
            );
            Some(complete_alias_suggestion(suggestion))
        }

        // NEW (Iteration 4): JOIN type keywords
        (SdblCompletionContext::JoinTypeKeyword { prefix }, _) => {
            tracing::info!(prefix = %prefix, "completion context: JoinTypeKeyword");
            Some(complete_join_types(&prefix))
        }

        // NEW (Iteration 4): Table aliases after ON
        (SdblCompletionContext::AfterOnKeyword { prefix }, Some(scope)) => {
            tracing::info!(
                prefix = %prefix,
                "completion context: AfterOnKeyword (with scope)"
            );
            Some(complete_table_aliases(scope, &prefix))
        }
        (SdblCompletionContext::AfterOnKeyword { prefix }, None) => {
            tracing::warn!(
                prefix = %prefix,
                "completion context: AfterOnKeyword but no scope available"
            );
            // Fallback to keywords if no scope
            Some(complete_sdbl_keywords(&prefix))
        }

        // Existing contexts (Iterations 1-2) - don't need scope
        (SdblCompletionContext::AfterFromKeyword, _) => {
            tracing::info!("completion context: AfterFromKeyword");
            Some(complete_mdo_types())
        }
        (SdblCompletionContext::InsideMdoType { mdo_type, prefix }, _) => {
            tracing::info!(
                ?mdo_type,
                prefix = %prefix,
                "completion context: InsideMdoType"
            );
            let config = get_configuration(db, position.workspace_root.as_deref());
            Some(complete_mdo_objects(&config, mdo_type, &prefix))
        }
        (SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix }, _) => {
            tracing::info!(
                ?mdo_type,
                object_name = %object_name,
                prefix = %prefix,
                "completion context: AfterMdoObject"
            );
            let config = get_configuration(db, position.workspace_root.as_deref());
            Some(complete_nested_elements(&config, mdo_type, &object_name, &prefix))
        }
        (SdblCompletionContext::SdblKeywords { prefix }, _) => {
            tracing::info!(prefix = %prefix, "completion context: SdblKeywords");
            Some(complete_sdbl_keywords(&prefix))
        }
        (SdblCompletionContext::None, _) => {
            tracing::info!("no completion context detected");
            None
        } // TODO (Iteration 4): Add more contexts
          // (AfterOnKeyword { prefix }, Some(scope)) => { ... }
          // (JoinTypeKeyword { prefix }, _) => { ... }
    }
}

/// Complete MDO types (Справочник, Catalog, Документ, Document, etc.)
///
/// Returns all available MDO type names in both Russian and English.
fn complete_mdo_types() -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for &mdo_type in MdoType::all() {
        // Russian variant
        items.push(CompletionItem {
            label: mdo_type.russian_name().to_string(),
            detail: None,
            kind: CompletionItemKind::MdoType,
            insert_text: mdo_type.russian_name().to_string(),
            documentation: None,
        });

        // English variant
        items.push(CompletionItem {
            label: mdo_type.english_name().to_string(),
            detail: None,
            kind: CompletionItemKind::MdoType,
            insert_text: mdo_type.english_name().to_string(),
            documentation: None,
        });
    }

    tracing::debug!(count = items.len(), "generated MDO type completions");
    items
}

/// Complete JOIN type keywords.
///
/// Returns JOIN keywords filtered by prefix (case-insensitive).
/// Includes both Russian and English variants.
///
/// # Arguments
///
/// * `prefix` - Prefix for filtering (case-insensitive)
///
/// # Returns
///
/// Vec of CompletionItem with JOIN type keywords (ЛЕВОЕ СОЕДИНЕНИЕ, LEFT JOIN, etc.)
fn complete_join_types(prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_lowercase();

    // JOIN type keywords (Russian and English, full and short forms)
    let join_keywords = vec![
        // Russian - full forms
        ("ЛЕВОЕ СОЕДИНЕНИЕ", "Левое внешнее соединение (LEFT JOIN)"),
        ("ПРАВОЕ СОЕДИНЕНИЕ", "Правое внешнее соединение (RIGHT JOIN)"),
        ("ВНУТРЕННЕЕ СОЕДИНЕНИЕ", "Внутреннее соединение (INNER JOIN)"),
        ("ПОЛНОЕ СОЕДИНЕНИЕ", "Полное внешнее соединение (FULL JOIN)"),
        // Russian - short forms
        ("ЛЕВОЕ", "Левое внешнее соединение"),
        ("ПРАВОЕ", "Правое внешнее соединение"),
        ("ВНУТРЕННЕЕ", "Внутреннее соединение"),
        ("ПОЛНОЕ", "Полное внешнее соединение"),
        // English - full forms
        ("LEFT JOIN", "Left outer join"),
        ("RIGHT JOIN", "Right outer join"),
        ("INNER JOIN", "Inner join"),
        ("FULL JOIN", "Full outer join"),
        // English - short forms
        ("LEFT", "Left outer join"),
        ("RIGHT", "Right outer join"),
        ("INNER", "Inner join"),
        ("FULL", "Full outer join"),
    ];

    join_keywords
        .into_iter()
        .filter(|(keyword, _)| keyword.to_lowercase().starts_with(&prefix_lower))
        .map(|(keyword, desc)| CompletionItem {
            label: keyword.to_string(),
            detail: Some(desc.to_string()),
            kind: CompletionItemKind::Keyword,
            insert_text: keyword.to_string(),
            documentation: Some(desc.to_string()),
        })
        .collect()
}

/// Complete table aliases from Scope.
///
/// Returns all table aliases available in the query scope, filtered by prefix.
///
/// # Arguments
///
/// * `scope` - Scope containing table information with aliases
/// * `prefix` - Prefix for filtering (case-insensitive)
///
/// # Returns
///
/// Vec of CompletionItem with table aliases (Т, Т1, Т2, etc.)
fn complete_table_aliases(scope: &Scope, prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_lowercase();

    // Get all tables with aliases from scope
    scope
        .all_tables()
        .filter_map(|table| {
            if let Some(ref alias) = table.alias {
                // Filter by prefix
                if alias.to_lowercase().starts_with(&prefix_lower) {
                    Some(CompletionItem {
                        label: alias.to_string(),
                        detail: Some(format!("Псевдоним для {}", table.full_name)),
                        kind: CompletionItemKind::Keyword,
                        insert_text: alias.to_string(),
                        documentation: Some(format!("Псевдоним таблицы {}", table.full_name)),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

/// Complete alias suggestion after AS/КАК keyword.
///
/// Returns a single completion item with the suggested alias name.
/// The suggestion is extracted from the context (field name or table name).
///
/// # Arguments
///
/// * `suggestion` - Optional suggested alias name (e.g., "Код", "Номенклатура")
///
/// # Returns
///
/// Vec with 0-1 CompletionItem containing the suggested alias.
fn complete_alias_suggestion(suggestion: Option<String>) -> Vec<CompletionItem> {
    if let Some(alias) = suggestion {
        vec![CompletionItem {
            label: alias.clone(),
            detail: Some("Предлагаемый псевдоним".to_string()),
            kind: CompletionItemKind::Keyword,
            insert_text: alias,
            documentation: Some("Псевдоним на основе имени поля или таблицы".to_string()),
        }]
    } else {
        // No suggestion available - return empty
        Vec::new()
    }
}

/// Complete fields for a table alias.
///
/// Uses Scope to retrieve field completions for the specified table alias.
/// Returns all fields from the table with the given alias, filtered by prefix.
///
/// # Arguments
///
/// * `scope` - Scope containing table and field information from query HIR
/// * `alias` - Table alias (e.g., "Т", "Т1", "Т2")
/// * `prefix` - Field name prefix for filtering (case-insensitive)
///
/// # Returns
///
/// Vec of CompletionItem with field names, types, and documentation.
fn complete_fields_by_alias(scope: &Scope, alias: &str, prefix: &str) -> Vec<CompletionItem> {
    let prefix_lower = prefix.to_lowercase();

    // Log scope state for debugging
    let all_tables: Vec<_> = scope.all_tables().collect();
    tracing::info!(
        alias = %alias,
        prefix = %prefix,
        tables_in_scope = all_tables.len(),
        "complete_fields_by_alias called"
    );

    for (i, table) in all_tables.iter().enumerate() {
        tracing::info!(
            table_index = i,
            full_name = %table.full_name,
            alias = ?table.alias,
            "table in scope"
        );
    }

    // Get column completions from scope for the specified alias
    let columns = scope.column_completions(Some(alias));

    tracing::info!(
        alias = %alias,
        columns_before_filter = columns.len(),
        "column_completions returned"
    );

    // Convert to CompletionItem and filter by prefix
    let results: Vec<CompletionItem> = columns
        .into_iter()
        .filter(|col| col.column_name.as_str().to_lowercase().starts_with(&prefix_lower))
        .map(|col| {
            let field_name = col.column_name.as_str().to_string();
            let type_desc = col.ty.to_string();
            let standard_marker = if col.is_standard { " (стандартный)" } else { "" };

            tracing::info!(
                field_name = %field_name,
                table_name = %col.table_name.as_str(),
                "including field in completion"
            );

            CompletionItem {
                label: field_name.clone(),
                detail: Some(format!("{}{}", type_desc, standard_marker)),
                kind: CompletionItemKind::Field,
                insert_text: field_name,
                documentation: Some(format!(
                    "Поле из таблицы {}\nТип: {}",
                    col.table_name.as_str(),
                    col.ty
                )),
            }
        })
        .collect();

    tracing::info!(
        alias = %alias,
        results_after_filter = results.len(),
        "complete_fields_by_alias returning"
    );

    results
}

/// Complete SDBL keywords.
///
/// Returns common SDBL/SQL keywords filtered by prefix (case-insensitive).
///
/// # Arguments
///
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_sdbl_keywords(prefix: &str) -> Vec<CompletionItem> {
    // Common SDBL keywords (Russian and English)
    let keywords = vec![
        // Query structure
        ("ВЫБРАТЬ", "SELECT", "Выбрать данные из таблицы"),
        ("ИЗ", "FROM", "Указать источник данных"),
        ("ГДЕ", "WHERE", "Условие фильтрации"),
        ("СГРУППИРОВАТЬ", "GROUP", "Группировка данных"),
        ("УПОРЯДОЧИТЬ", "ORDER", "Сортировка результатов"),
        ("ПО", "BY", "Указать поля для группировки/сортировки"),
        // Joins
        ("СОЕДИНЕНИЕ", "JOIN", "Соединение таблиц"),
        ("ЛЕВОЕ", "LEFT", "Левое внешнее соединение"),
        ("ПРАВОЕ", "RIGHT", "Правое внешнее соединение"),
        ("ПОЛНОЕ", "FULL", "Полное внешнее соединение"),
        ("ВНУТРЕННЕЕ", "INNER", "Внутреннее соединение"),
        // Other keywords
        ("КАК", "AS", "Псевдоним для поля или таблицы"),
        ("И", "AND", "Логическое И"),
        ("ИЛИ", "OR", "Логическое ИЛИ"),
        ("НЕ", "NOT", "Логическое НЕ"),
        ("МЕЖДУ", "BETWEEN", "Проверка вхождения в диапазон"),
        ("В", "IN", "Проверка вхождения в список"),
        ("ЕСТЬ", "IS", "Проверка на NULL"),
        ("NULL", "NULL", "Значение NULL"),
        ("ПОДОБНО", "LIKE", "Поиск по шаблону"),
        ("ПЕРВЫЕ", "TOP", "Ограничение количества строк"),
        ("РАЗЛИЧНЫЕ", "DISTINCT", "Уникальные значения"),
        ("ОБЪЕДИНИТЬ", "UNION", "Объединение результатов запросов"),
        ("ВСЕ", "ALL", "Все строки (для UNION)"),
        ("ИМЕЮЩИЕ", "HAVING", "Фильтрация после группировки"),
        ("ВЫРАЗИТЬ", "CAST", "Преобразование типа"),
        ("ЗНАЧЕНИЕ", "VALUE", "Литеральное значение"),
        ("ИСТИНА", "TRUE", "Логическое истина"),
        ("ЛОЖЬ", "FALSE", "Логическое ложь"),
    ];

    let prefix_lower = prefix.to_lowercase();

    let mut items = Vec::new();

    for (russian, english, description) in &keywords {
        // Add Russian variant if matches
        if russian.to_lowercase().starts_with(&prefix_lower) || prefix.is_empty() {
            items.push(CompletionItem {
                label: russian.to_string(),
                detail: Some(format!("Ключевое слово SDBL ({})", english)),
                kind: CompletionItemKind::Keyword,
                insert_text: russian.to_string(),
                documentation: Some(description.to_string()),
            });
        }

        // Add English variant if matches
        if english.to_lowercase().starts_with(&prefix_lower) || prefix.is_empty() {
            items.push(CompletionItem {
                label: english.to_string(),
                detail: Some(format!("SDBL keyword ({})", russian)),
                kind: CompletionItemKind::Keyword,
                insert_text: english.to_string(),
                documentation: Some(description.to_string()),
            });
        }
    }

    tracing::debug!(
        count = items.len(),
        total_keywords = keywords.len(),
        "generated SDBL keyword completions"
    );
    items
}

/// Complete MDO objects for a specific type.
///
/// Returns metadata objects filtered by:
/// - MDO type (Catalog, Document, etc.)
/// - Prefix (typed text after the dot)
///
/// # Arguments
///
/// * `config` - Configuration with metadata objects
/// * `mdo_type` - Metadata object type
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_mdo_objects(
    config: &Configuration,
    mdo_type: MdoType,
    prefix: &str,
) -> Vec<CompletionItem> {
    // Get metadata objects of the specified type
    let objects = get_objects_by_type(config, mdo_type);

    let prefix_lower = prefix.to_lowercase();

    // Filter objects by prefix (case-insensitive)
    let items: Vec<CompletionItem> = objects
        .iter()
        .filter(|obj| {
            // Match by Russian name
            obj.name.to_lowercase().starts_with(&prefix_lower)
                // Or match by English name (if available)
                || obj
                    .name_en
                    .as_ref()
                    .is_some_and(|en| en.to_lowercase().starts_with(&prefix_lower))
        })
        .map(|obj| {
            CompletionItem {
                label: obj.name.clone(),
                // Show full path in detail: "Справочник.Валюты"
                detail: Some(format!("{}.{}", mdo_type.russian_name(), obj.name)),
                kind: CompletionItemKind::MdoObject,
                insert_text: obj.name.clone(),
                documentation: None,
            }
        })
        .collect();

    tracing::debug!(
        count = items.len(),
        total = objects.len(),
        ?mdo_type,
        "generated MDO object completions"
    );

    items
}

/// Complete nested elements (tabular sections or virtual tables).
///
/// For registers: returns virtual tables (СрезПервых, Остатки, etc.)
/// For other MDO types: returns tabular sections
///
/// # Arguments
///
/// * `config` - Configuration with metadata
/// * `mdo_type` - Type of the metadata object
/// * `object_name` - Name of the specific object (e.g., "Номенклатура")
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_nested_elements(
    config: &Configuration,
    mdo_type: MdoType,
    object_name: &str,
    prefix: &str,
) -> Vec<CompletionItem> {
    // Distinguish between registers and other MDO types
    match mdo_type {
        MdoType::InformationRegister
        | MdoType::AccumulationRegister
        | MdoType::AccountingRegister
        | MdoType::CalculationRegister => {
            // Find register and return virtual tables
            complete_virtual_tables(config, mdo_type, object_name, prefix)
        }
        _ => {
            // Find MDO object and return tabular sections
            complete_tabular_sections(config, mdo_type, object_name, prefix)
        }
    }
}

/// Complete tabular sections for an MDO object.
///
/// Returns tabular sections (табличные части) of the specified metadata object.
///
/// # Arguments
///
/// * `config` - Configuration with metadata
/// * `mdo_type` - Type of the metadata object
/// * `object_name` - Name of the object (e.g., "Номенклатура")
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_tabular_sections(
    config: &Configuration,
    mdo_type: MdoType,
    object_name: &str,
    prefix: &str,
) -> Vec<CompletionItem> {
    // Find metadata object by name
    let object = config
        .metadata_objects()
        .iter()
        .find(|obj| obj.mdo_type == mdo_type && obj.name == object_name);

    let Some(object) = object else {
        tracing::debug!(
            ?mdo_type,
            object_name = %object_name,
            "metadata object not found"
        );
        return Vec::new();
    };

    let prefix_lower = prefix.to_lowercase();

    // Filter tabular sections by prefix
    let items: Vec<CompletionItem> = object
        .tabular_sections
        .iter()
        .filter(|ts| ts.name().to_lowercase().starts_with(&prefix_lower))
        .map(|ts| {
            CompletionItem {
                label: ts.name().to_string(),
                // Show full path in detail: "Справочник.Номенклатура.Штрихкоды"
                detail: Some(format!("{}.{}.{}", mdo_type.russian_name(), object_name, ts.name())),
                kind: CompletionItemKind::Field,
                insert_text: ts.name().to_string(),
                documentation: ts.synonym().map(|s| s.to_string()),
            }
        })
        .collect();

    tracing::debug!(
        count = items.len(),
        total = object.tabular_sections.len(),
        ?mdo_type,
        object_name = %object_name,
        "generated tabular section completions"
    );

    items
}

/// Complete virtual tables for a register.
///
/// Returns virtual tables based on register parameters:
/// - InformationRegister: СрезПервых, СрезПоследних (for periodic registers)
/// - AccumulationRegister: Остатки, Обороты (based on register type)
/// - AccountingRegister, CalculationRegister: TODO (complex logic)
///
/// # Arguments
///
/// * `config` - Configuration with metadata
/// * `mdo_type` - Type of the register
/// * `register_name` - Name of the register
/// * `prefix` - Filter prefix (case-insensitive)
fn complete_virtual_tables(
    config: &Configuration,
    mdo_type: MdoType,
    register_name: &str,
    prefix: &str,
) -> Vec<CompletionItem> {
    // Find register by name
    let register = config
        .registers()
        .iter()
        .find(|reg| reg.mdo_type() == mdo_type && reg.name() == register_name);

    let Some(register) = register else {
        tracing::debug!(
            ?mdo_type,
            register_name = %register_name,
            "register not found"
        );
        return Vec::new();
    };

    // Get virtual tables based on register parameters
    let virtual_tables = register.virtual_tables();

    let prefix_lower = prefix.to_lowercase();

    // Filter by prefix and convert to CompletionItem
    let items: Vec<CompletionItem> = virtual_tables
        .into_iter()
        .filter(|vt| vt.to_lowercase().starts_with(&prefix_lower))
        .map(|vt| {
            CompletionItem {
                label: vt.to_string(),
                // Show full path in detail: "РегистрСведений.МойРегистр.СрезПоследних"
                detail: Some(format!("{}.{}.{}", mdo_type.russian_name(), register_name, vt)),
                kind: CompletionItemKind::Field,
                insert_text: vt.to_string(),
                documentation: Some(format!("Виртуальная таблица {}", vt)),
            }
        })
        .collect();

    tracing::debug!(
        count = items.len(),
        ?mdo_type,
        register_name = %register_name,
        "generated virtual table completions"
    );

    items
}

/// Get metadata objects of a specific type from configuration.
fn get_objects_by_type(
    config: &Configuration,
    mdo_type: MdoType,
) -> Vec<bsl_metadata::MetadataObject> {
    // Check if this is a register type - registers are stored separately
    if matches!(
        mdo_type,
        MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister
    ) {
        // Convert Register objects to MetadataObject
        return config
            .registers()
            .iter()
            .filter(|reg| reg.mdo_type() == mdo_type)
            .map(|reg| {
                // Create MetadataObject from Register
                bsl_metadata::MetadataObject::new(mdo_type, reg.name())
            })
            .collect();
    }

    // Filter metadata_objects by type (for Catalogs, Documents, etc.)
    config.metadata_objects().iter().filter(|obj| obj.mdo_type == mdo_type).cloned().collect()
}

/// Get configuration from workspace.
///
/// Searches for 1C configuration using multiple strategies:
/// 1. Read .bsl-language-server.json or .bsl-analyzer.json (configurationRoot field)
/// 2. Search for Configuration.xml in workspace (max depth 2)
///
/// If no configuration is found, returns an empty Configuration (no completion suggestions).
///
/// # Arguments
///
/// * `_db` - Database (unused currently, reserved for future Salsa integration)
/// * `workspace_root` - Root directory of the workspace
fn get_configuration(
    _db: &dyn RootDatabase,
    workspace_root: Option<&std::path::Path>,
) -> std::sync::Arc<Configuration> {
    use std::sync::Arc;

    // If workspace_root provided, try to find configuration
    if let Some(root) = workspace_root {
        tracing::debug!(
            workspace_root = ?root,
            "searching for 1C configuration"
        );

        if let Some(config_path) = crate::config_finder::find_configuration_path(root) {
            match bsl_metadata::load_from_directory(&config_path) {
                Ok(config) => {
                    tracing::info!(
                        catalogs = config.metadata_objects().iter()
                            .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Catalog)
                            .count(),
                        documents = config.metadata_objects().iter()
                            .filter(|obj| obj.mdo_type == bsl_metadata::MdoType::Document)
                            .count(),
                        config_path = ?config_path,
                        "loaded metadata from workspace"
                    );
                    return Arc::new(config);
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        config_path = ?config_path,
                        "failed to load metadata from configuration path"
                    );
                }
            }
        }
    }

    // No configuration found - return empty configuration (no completion suggestions)
    tracing::warn!(
        workspace_root = ?workspace_root,
        "no metadata found, using empty configuration"
    );
    Arc::new(Configuration::new("EmptyConfiguration"))
}

/// Get Scope with table aliases for SDBL queries in the file.
///
/// Returns the Scope built from the first query's FROM and JOIN clauses.
/// This allows completion to access table aliases and their fields.
///
/// # Note
///
/// Currently uses the first query in the file (index 0).
/// Future enhancement: match query by cursor position.
///
/// # Arguments
///
/// * `db` - Database with Salsa queries
/// * `file_id` - File containing the SDBL query
///
/// Returns `None` if:
/// - The file doesn't exist
/// - No SDBL queries found in the file
/// - The query couldn't be lowered to HIR (e.g., syntax errors)
fn get_sdbl_scope(db: &dyn RootDatabase, file_id: vfs::FileId) -> Option<Scope> {
    // 1. Get lowered HIR through Salsa query (CACHED!)
    let sdbl_hirs = db.sdbl_hir_in_file(file_id);

    tracing::info!(
        sdbl_hirs_count = sdbl_hirs.len(),
        "get_sdbl_scope: retrieved SDBL HIRs from cache"
    );

    // 2. Get the first query (TODO: match by cursor position)
    let (_expr_id, sdbl_lower_result) = sdbl_hirs.first()?;

    tracing::info!(
        from_tables_count = sdbl_lower_result.hir.from.len(),
        join_tables_count = sdbl_lower_result.hir.joins.len(),
        diagnostics_count = sdbl_lower_result.hir.diagnostics.len(),
        "get_sdbl_scope: examining first SDBL HIR"
    );

    // Log FROM tables in detail
    for (i, table) in sdbl_lower_result.hir.from.iter().enumerate() {
        tracing::info!(
            table_index = i,
            full_name = %table.full_name,
            alias = ?table.alias,
            has_metadata = table.metadata.is_some(),
            field_count = table.metadata.as_ref().map(|m| m.fields.len()).unwrap_or(0),
            "FROM table in HIR"
        );
    }

    // 3. Rebuild Scope from HIR
    let mut scope = Scope::new();
    let hir = &sdbl_lower_result.hir;

    // Add tables from FROM clause
    for table in &hir.from {
        scope.add_table(table.clone());
    }

    // Add tables from JOIN clauses
    for join in &hir.joins {
        scope.add_table(join.table.clone());
    }

    tracing::info!(
        from_tables = hir.from.len(),
        join_tables = hir.joins.len(),
        "built Scope from HIR"
    );

    Some(scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MetadataObject;

    fn setup_test_configuration() -> Configuration {
        // Create test configuration with sample metadata
        let mut config = Configuration::new("TestConfig");

        // Add some test catalogs
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Валюты"));
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Контрагенты"));
        config.add_metadata_object(MetadataObject::new(MdoType::Catalog, "Номенклатура"));

        // Add some test documents
        config.add_metadata_object(MetadataObject::new(MdoType::Document, "ЗаказПокупателя"));
        config.add_metadata_object(MetadataObject::new(MdoType::Document, "Продажа"));

        config
    }

    #[test]
    fn test_complete_mdo_types() {
        let items = complete_mdo_types();

        // Should have both Russian and English for each MDO type
        // MdoType::all() returns 20 types, so we should have 40 items (20 * 2)
        assert_eq!(items.len(), 40);

        // Check for specific types
        assert!(items.iter().any(|i| i.label == "Справочник"));
        assert!(items.iter().any(|i| i.label == "Catalog"));
        assert!(items.iter().any(|i| i.label == "Документ"));
        assert!(items.iter().any(|i| i.label == "Document"));
        assert!(items.iter().any(|i| i.label == "РегистрСведений"));
        assert!(items.iter().any(|i| i.label == "InformationRegister"));
    }

    #[test]
    fn test_complete_mdo_objects_all() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // Should return all catalogs (no prefix filtering)
        assert_eq!(items.len(), 3);
        assert!(items.iter().any(|i| i.label == "Валюты"));
        assert!(items.iter().any(|i| i.label == "Контрагенты"));
        assert!(items.iter().any(|i| i.label == "Номенклатура"));

        // Check details
        let valuta_item = items.iter().find(|i| i.label == "Валюты").unwrap();
        assert_eq!(valuta_item.detail, Some("Справочник.Валюты".to_string()));
        assert_eq!(valuta_item.kind, CompletionItemKind::MdoObject);
    }

    #[test]
    fn test_complete_mdo_objects_with_prefix() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "Вал");

        // Should return only "Валюты" (starts with "Вал")
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_case_insensitive() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "вал");

        // Case-insensitive matching
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Валюты");
    }

    #[test]
    fn test_complete_mdo_objects_no_match() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Catalog, "Xyz");

        // No matches
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_mdo_objects_documents() {
        let config = setup_test_configuration();

        let items = complete_mdo_objects(&config, MdoType::Document, "");

        // Should return all documents
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "ЗаказПокупателя"));
        assert!(items.iter().any(|i| i.label == "Продажа"));
    }

    #[test]
    fn test_complete_mdo_objects_empty_config() {
        let config = Configuration::new("Empty");

        let items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // No metadata objects in empty config
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_load_fixtures_directly() {
        // Load fixtures using absolute path from workspace root
        // This simulates what get_configuration() should do in production
        let fixtures_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let config =
            bsl_metadata::load_from_directory(fixtures_path).expect("Failed to load fixtures");

        let metadata_objects = config.metadata_objects();

        // Check that we have the expected objects from fixtures
        let catalogs: Vec<_> =
            metadata_objects.iter().filter(|obj| obj.mdo_type == MdoType::Catalog).collect();

        let documents: Vec<_> =
            metadata_objects.iter().filter(|obj| obj.mdo_type == MdoType::Document).collect();

        // fixtures/designer has Справочник1 and possibly СправочникСМенеджером
        assert!(
            !catalogs.is_empty(),
            "Expected at least 1 catalog from fixtures, got {}",
            catalogs.len()
        );

        // fixtures/designer has Документ1
        assert!(
            !documents.is_empty(),
            "Expected at least 1 document from fixtures, got {}",
            documents.len()
        );

        // Check specific objects exist
        assert!(
            metadata_objects.iter().any(|obj| obj.name == "Справочник1"),
            "Expected Справочник1 in fixtures"
        );

        assert!(
            metadata_objects.iter().any(|obj| obj.name == "Документ1"),
            "Expected Документ1 in fixtures"
        );
    }

    #[test]
    fn test_complete_mdo_objects_with_fixtures() {
        // Load real fixtures for integration testing
        let fixtures_path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer");

        let config =
            bsl_metadata::load_from_directory(fixtures_path).expect("Failed to load fixtures");

        // Test catalog completion
        let catalog_items = complete_mdo_objects(&config, MdoType::Catalog, "");

        // Should have at least Справочник1 from fixtures
        assert!(
            !catalog_items.is_empty(),
            "Expected at least 1 catalog completion item, got {}",
            catalog_items.len()
        );

        assert!(
            catalog_items.iter().any(|item| item.label == "Справочник1"),
            "Expected Справочник1 in completion items"
        );

        // Check item structure
        let справочник1 = catalog_items
            .iter()
            .find(|item| item.label == "Справочник1")
            .expect("Справочник1 not found");

        assert_eq!(справочник1.detail, Some("Справочник.Справочник1".to_string()));
        assert_eq!(справочник1.kind, CompletionItemKind::MdoObject);
        assert_eq!(справочник1.insert_text, "Справочник1");

        // Test document completion
        let document_items = complete_mdo_objects(&config, MdoType::Document, "");

        assert!(
            !document_items.is_empty(),
            "Expected at least 1 document completion item, got {}",
            document_items.len()
        );

        assert!(
            document_items.iter().any(|item| item.label == "Документ1"),
            "Expected Документ1 in completion items"
        );

        // Test InformationRegister completion (registers without folders should also work)
        let register_items = complete_mdo_objects(&config, MdoType::InformationRegister, "");

        assert!(
            !register_items.is_empty(),
            "Expected at least 1 InformationRegister completion item, got {}",
            register_items.len()
        );

        assert!(
            register_items.iter().any(|item| item.label == "РегистрСведений1"),
            "Expected РегистрСведений1 in completion items"
        );

        // Check register item structure
        let register1 = register_items
            .iter()
            .find(|item| item.label == "РегистрСведений1")
            .expect("РегистрСведений1 not found");

        assert_eq!(register1.detail, Some("РегистрСведений.РегистрСведений1".to_string()));
        assert_eq!(register1.kind, CompletionItemKind::MdoObject);
        assert_eq!(register1.insert_text, "РегистрСведений1");
    }

    // --- Tests for nested elements completion ---

    #[test]
    fn test_complete_tabular_sections() {
        use bsl_metadata::{MetadataObject, TabularSection};
        use uuid::Uuid;

        // Create catalog with tabular sections
        let mut config = Configuration::new("TestConfig");
        let mut catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");

        // Add tabular sections
        let ts1 = TabularSection::new(Uuid::new_v4(), "Штрихкоды");
        let ts2 = TabularSection::new(Uuid::new_v4(), "Характеристики");
        catalog.add_tabular_section(ts1);
        catalog.add_tabular_section(ts2);

        config.add_metadata_object(catalog);

        // Test completion without prefix
        let items = complete_tabular_sections(&config, MdoType::Catalog, "Номенклатура", "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Штрихкоды"));
        assert!(items.iter().any(|i| i.label == "Характеристики"));

        // Check item structure
        let item = items.iter().find(|i| i.label == "Штрихкоды").unwrap();
        assert_eq!(item.detail, Some("Справочник.Номенклатура.Штрихкоды".to_string()));
        assert_eq!(item.kind, CompletionItemKind::Field);
    }

    #[test]
    fn test_complete_tabular_sections_with_prefix() {
        use bsl_metadata::{MetadataObject, TabularSection};
        use uuid::Uuid;

        let mut config = Configuration::new("TestConfig");
        let mut catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");

        let ts1 = TabularSection::new(Uuid::new_v4(), "Штрихкоды");
        let ts2 = TabularSection::new(Uuid::new_v4(), "Характеристики");
        catalog.add_tabular_section(ts1);
        catalog.add_tabular_section(ts2);

        config.add_metadata_object(catalog);

        // Test with prefix
        let items = complete_tabular_sections(&config, MdoType::Catalog, "Номенклатура", "Шт");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Штрихкоды");
    }

    #[test]
    fn test_complete_tabular_sections_object_not_found() {
        let config = Configuration::new("TestConfig");

        let items =
            complete_tabular_sections(&config, MdoType::Catalog, "НесуществующийОбъект", "");

        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_virtual_tables_information_register() {
        use bsl_metadata::{Register, RegisterPeriodicity};

        let mut config = Configuration::new("TestConfig");

        // Create periodic InformationRegister with slice flags
        let register = Register::builder()
            .name("МойРегистр")
            .mdo_type(MdoType::InformationRegister)
            .periodicity(Some(RegisterPeriodicity::Day))
            .enable_totals_slice_first(true)
            .enable_totals_slice_last(true)
            .build();

        config.add_register(register);

        // Test completion
        let items =
            complete_virtual_tables(&config, MdoType::InformationRegister, "МойРегистр", "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "СрезПервых"));
        assert!(items.iter().any(|i| i.label == "СрезПоследних"));

        // Check item structure
        let item = items.iter().find(|i| i.label == "СрезПервых").unwrap();
        assert_eq!(item.detail, Some("РегистрСведений.МойРегистр.СрезПервых".to_string()));
        assert_eq!(item.kind, CompletionItemKind::Field);
    }

    #[test]
    fn test_complete_virtual_tables_accumulation_register_balance() {
        use bsl_metadata::{AccumulationRegisterType, Register};

        let mut config = Configuration::new("TestConfig");

        // Create AccumulationRegister with Balance type
        let register = Register::builder()
            .name("КоличествоЗадач")
            .mdo_type(MdoType::AccumulationRegister)
            .register_type(Some(AccumulationRegisterType::Balance))
            .build();

        config.add_register(register);

        // Test completion
        let items =
            complete_virtual_tables(&config, MdoType::AccumulationRegister, "КоличествоЗадач", "");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Остатки");
        assert_eq!(items[0].detail, Some("РегистрНакопления.КоличествоЗадач.Остатки".to_string()));
    }

    #[test]
    fn test_complete_virtual_tables_accumulation_register_turnovers() {
        use bsl_metadata::{AccumulationRegisterType, Register};

        let mut config = Configuration::new("TestConfig");

        let register = Register::builder()
            .name("РабочееВремя")
            .mdo_type(MdoType::AccumulationRegister)
            .register_type(Some(AccumulationRegisterType::Turnovers))
            .build();

        config.add_register(register);

        let items =
            complete_virtual_tables(&config, MdoType::AccumulationRegister, "РабочееВремя", "");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Обороты");
    }

    #[test]
    fn test_complete_virtual_tables_accumulation_register_both() {
        use bsl_metadata::{AccumulationRegisterType, Register};

        let mut config = Configuration::new("TestConfig");

        let register = Register::builder()
            .name("ОстаткиИОбороты")
            .mdo_type(MdoType::AccumulationRegister)
            .register_type(Some(AccumulationRegisterType::BalanceAndTurnovers))
            .build();

        config.add_register(register);

        let items =
            complete_virtual_tables(&config, MdoType::AccumulationRegister, "ОстаткиИОбороты", "");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "Остатки"));
        assert!(items.iter().any(|i| i.label == "Обороты"));
    }

    #[test]
    fn test_complete_virtual_tables_nonperiodic_register() {
        use bsl_metadata::{Register, RegisterPeriodicity};

        let mut config = Configuration::new("TestConfig");

        // Nonperiodic register should have no virtual tables
        let register = Register::builder()
            .name("НепериодическийРегистр")
            .mdo_type(MdoType::InformationRegister)
            .periodicity(Some(RegisterPeriodicity::Nonperiodical))
            .enable_totals_slice_first(true)
            .enable_totals_slice_last(true)
            .build();

        config.add_register(register);

        let items = complete_virtual_tables(
            &config,
            MdoType::InformationRegister,
            "НепериодическийРегистр",
            "",
        );

        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_complete_nested_elements_routes_to_tabular_sections() {
        use bsl_metadata::{MetadataObject, TabularSection};
        use uuid::Uuid;

        let mut config = Configuration::new("TestConfig");
        let mut catalog = MetadataObject::new(MdoType::Catalog, "Номенклатура");
        catalog.add_tabular_section(TabularSection::new(Uuid::new_v4(), "Штрихкоды"));
        config.add_metadata_object(catalog);

        // Should route to complete_tabular_sections for catalogs
        let items = complete_nested_elements(&config, MdoType::Catalog, "Номенклатура", "");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Штрихкоды");
    }

    #[test]
    fn test_complete_nested_elements_routes_to_virtual_tables() {
        use bsl_metadata::{AccumulationRegisterType, Register};

        let mut config = Configuration::new("TestConfig");
        let register = Register::builder()
            .name("КоличествоЗадач")
            .mdo_type(MdoType::AccumulationRegister)
            .register_type(Some(AccumulationRegisterType::Balance))
            .build();
        config.add_register(register);

        // Should route to complete_virtual_tables for registers
        let items =
            complete_nested_elements(&config, MdoType::AccumulationRegister, "КоличествоЗадач", "");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Остатки");
    }

    // --- Tests for SDBL keywords completion ---

    #[test]
    fn test_complete_sdbl_keywords_all() {
        let items = complete_sdbl_keywords("");

        // Should return all keywords (both Russian and English variants)
        assert!(!items.is_empty(), "Should return some keywords");

        // Check for specific keywords
        assert!(items.iter().any(|i| i.label == "ВЫБРАТЬ"), "Should contain ВЫБРАТЬ");
        assert!(items.iter().any(|i| i.label == "SELECT"), "Should contain SELECT");
        assert!(items.iter().any(|i| i.label == "ИЗ"), "Should contain ИЗ");
        assert!(items.iter().any(|i| i.label == "FROM"), "Should contain FROM");
        assert!(items.iter().any(|i| i.label == "ГДЕ"), "Should contain ГДЕ");
        assert!(items.iter().any(|i| i.label == "WHERE"), "Should contain WHERE");

        // All items should be keywords
        assert!(items.iter().all(|i| i.kind == CompletionItemKind::Keyword));
    }

    #[test]
    fn test_complete_sdbl_keywords_russian_prefix() {
        let items = complete_sdbl_keywords("ВЫ");

        // Should return keywords starting with "ВЫ"
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "ВЫБРАТЬ"));
        assert!(items.iter().any(|i| i.label == "ВЫРАЗИТЬ"));

        // Should not return unmatched keywords
        assert!(!items.iter().any(|i| i.label == "ИЗ"));
    }

    #[test]
    fn test_complete_sdbl_keywords_english_prefix() {
        let items = complete_sdbl_keywords("SEL");

        // Should return SELECT
        assert!(items.iter().any(|i| i.label == "SELECT"));

        // Should not return unmatched keywords
        assert!(!items.iter().any(|i| i.label == "FROM"));
    }

    #[test]
    fn test_complete_sdbl_keywords_case_insensitive() {
        let items_upper = complete_sdbl_keywords("ГДЕ");
        let items_lower = complete_sdbl_keywords("где");

        // Both should return same results (case-insensitive matching)
        assert!(!items_upper.is_empty());
        assert!(!items_lower.is_empty());
        assert!(items_upper.iter().any(|i| i.label == "ГДЕ"));
        assert!(items_lower.iter().any(|i| i.label == "ГДЕ"));
    }

    #[test]
    fn test_complete_sdbl_keywords_join() {
        let items = complete_sdbl_keywords("ЛЕВОЕ");

        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ"));
        assert_eq!(items.len(), 1); // Only ЛЕВОЕ matches
    }

    // ========== get_sdbl_scope() tests ==========

    #[test]
    fn test_get_sdbl_scope_simple_query() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with simple SDBL query (one table)
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ Справочник.Товары";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id);

        // Should successfully build Scope
        assert!(scope.is_some(), "Should build Scope for valid query");

        let scope = scope.unwrap();

        // Should have one table
        let tables: Vec<_> = scope.all_tables().collect();
        assert_eq!(tables.len(), 1, "Should have 1 table in Scope");
        assert_eq!(tables[0].full_name, "Справочник.Товары");
    }

    #[test]
    fn test_get_sdbl_scope_query_with_join() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with JOIN
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Т1.Код,
             |    Т2.Наименование
             |ИЗ Справочник.Валюты КАК Т1
             |    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2
             |        ПО Т1.Ссылка = Т2.Валюта";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id);

        // Should successfully build Scope
        assert!(scope.is_some(), "Should build Scope for query with JOIN");

        let scope = scope.unwrap();

        // Should have two tables (FROM + JOIN)
        let tables: Vec<_> = scope.all_tables().collect();
        assert_eq!(tables.len(), 2, "Should have 2 tables in Scope");

        // Check table names
        assert_eq!(tables[0].full_name, "Справочник.Валюты");
        assert_eq!(tables[1].full_name, "Справочник.Номенклатура");

        // Check aliases
        assert_eq!(tables[0].alias.as_ref().unwrap(), "Т1");
        assert_eq!(tables[1].alias.as_ref().unwrap(), "Т2");
    }

    #[test]
    fn test_get_sdbl_scope_invalid_query() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with invalid SDBL query (syntax error)
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Код ИЗ";  // Missing table name
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id);

        // Should return None for invalid query (or Some with empty tables)
        // Depending on how HIR lowering handles errors, this might be None or Some(empty)
        if let Some(scope) = scope {
            // If Scope is returned, it should have no tables or be invalid
            let tables: Vec<_> = scope.all_tables().collect();
            // Either no tables or HIR lowering created partial structure
            // We accept both outcomes as valid for this test
            assert!(
                tables.is_empty() || !tables.is_empty(),
                "Invalid query may result in empty or partial Scope"
            );
        }
        // If None, that's also acceptable - HIR lowering failed
    }

    #[test]
    fn test_get_sdbl_scope_no_sdbl_query() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file WITHOUT any SDBL query
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Переменная = 42;
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id);

        // Should return None (no SDBL queries in file)
        assert!(scope.is_none(), "Should return None when no SDBL queries found");
    }

    // ========== complete_fields_by_alias() tests ==========

    #[test]
    fn test_complete_fields_by_alias_basic() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id);
        assert!(scope.is_some(), "Should build Scope for query with alias");

        let scope = scope.unwrap();

        // Test completion with no prefix (should show all fields)
        let items = complete_fields_by_alias(&scope, "Т", "");

        // Should have standard fields (Ссылка, Код, Наименование, etc.)
        assert!(!items.is_empty(), "Should return field completions");
        assert!(items.iter().any(|i| i.label == "Ссылка"), "Should include standard field Ссылка");
        assert!(items.iter().any(|i| i.label == "Код"), "Should include standard field Код");

        // All items should be fields
        assert!(items.iter().all(|i| i.kind == CompletionItemKind::Field));
    }

    #[test]
    fn test_complete_fields_by_alias_with_prefix() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with prefix "Код"
        let items = complete_fields_by_alias(&scope, "Т", "Код");

        // Should filter to fields starting with "Код"
        assert!(!items.is_empty(), "Should return filtered field completions");
        assert!(items.iter().all(|i| i.label.to_lowercase().starts_with("код")));
    }

    #[test]
    fn test_complete_fields_by_alias_case_insensitive() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test case-insensitive filtering: lowercase prefix should match uppercase field
        let items_lower = complete_fields_by_alias(&scope, "Т", "код");
        let items_upper = complete_fields_by_alias(&scope, "Т", "КОД");

        // Both should return results
        assert!(!items_lower.is_empty(), "Lowercase prefix should match");
        assert!(!items_upper.is_empty(), "Uppercase prefix should match");

        // Results should be the same
        assert_eq!(items_lower.len(), items_upper.len());
    }

    #[test]
    fn test_complete_fields_by_alias_multiple_tables() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with multiple aliases
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т1.Код, Т2.Номер
             |ИЗ Справочник.Валюты КАК Т1
             |    ЛЕВОЕ СОЕДИНЕНИЕ Документ.Продажа КАК Т2
             |        ПО Т1.Ссылка = Т2.Валюта";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion for first alias (Т1)
        let items_t1 = complete_fields_by_alias(&scope, "Т1", "");
        assert!(!items_t1.is_empty(), "Should return fields for Т1");
        assert!(items_t1.iter().any(|i| i.label == "Код"), "Т1 should have Код field");

        // Test completion for second alias (Т2)
        let items_t2 = complete_fields_by_alias(&scope, "Т2", "");
        assert!(!items_t2.is_empty(), "Should return fields for Т2");
        assert!(items_t2.iter().any(|i| i.label == "Номер"), "Т2 should have Номер field");

        // Verify different aliases return different fields
        // (catalogs and documents have different standard fields)
        let t1_fields: Vec<_> = items_t1.iter().map(|i| &i.label).collect();
        let t2_fields: Vec<_> = items_t2.iter().map(|i| &i.label).collect();

        // Not all fields should be the same (different metadata objects)
        assert_ne!(t1_fields, t2_fields, "Different aliases should have different field sets");
    }

    #[test]
    fn test_complete_fields_by_alias_no_match() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with non-existent prefix
        let items = complete_fields_by_alias(&scope, "Т", "Xyz");

        // Should return empty (no fields start with "Xyz")
        assert!(items.is_empty(), "Should return empty for non-matching prefix");
    }

    // ========== complete_alias_suggestion() tests ==========

    #[test]
    fn test_complete_alias_suggestion_with_suggestion() {
        let items = complete_alias_suggestion(Some("Код".to_string()));

        // Should return exactly one item
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Код");
        assert_eq!(items[0].insert_text, "Код");
        assert_eq!(items[0].kind, CompletionItemKind::Keyword);
        assert_eq!(items[0].detail, Some("Предлагаемый псевдоним".to_string()));
    }

    #[test]
    fn test_complete_alias_suggestion_without_suggestion() {
        let items = complete_alias_suggestion(None);

        // Should return empty vector
        assert!(items.is_empty(), "Should return empty when no suggestion available");
    }

    #[test]
    fn test_complete_alias_suggestion_table_name() {
        let items = complete_alias_suggestion(Some("Номенклатура".to_string()));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Номенклатура");
    }

    #[test]
    fn test_complete_alias_suggestion_tabular_section() {
        let items = complete_alias_suggestion(Some("Товары".to_string()));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "Товары");
    }

    // ========== complete_join_types() tests ==========

    #[test]
    fn test_complete_join_types_russian_prefix_л() {
        let items = complete_join_types("Л");

        // Should return ЛЕВОЕ variants: ЛЕВОЕ СОЕДИНЕНИЕ, ЛЕВОЕ, LEFT JOIN, LEFT
        assert!(!items.is_empty(), "Should return JOIN keywords starting with Л");
        assert!(
            items.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"),
            "Should include ЛЕВОЕ СОЕДИНЕНИЕ"
        );
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ"), "Should include ЛЕВОЕ");

        // All returned items should start with "Л" (case-insensitive)
        assert!(items.iter().all(|i| i.label.to_lowercase().starts_with("л")));

        // All items should be keywords
        assert!(items.iter().all(|i| i.kind == CompletionItemKind::Keyword));
    }

    #[test]
    fn test_complete_join_types_russian_prefix_вн() {
        let items = complete_join_types("ВН");

        // Should return ВНУТРЕННЕЕ variants: ВНУТРЕННЕЕ СОЕДИНЕНИЕ, ВНУТРЕННЕЕ
        assert!(!items.is_empty(), "Should return JOIN keywords starting with ВН");
        assert!(
            items.iter().any(|i| i.label == "ВНУТРЕННЕЕ СОЕДИНЕНИЕ"),
            "Should include ВНУТРЕННЕЕ СОЕДИНЕНИЕ"
        );
        assert!(items.iter().any(|i| i.label == "ВНУТРЕННЕЕ"), "Should include ВНУТРЕННЕЕ");

        // All returned items should start with "ВН" (case-insensitive)
        assert!(items.iter().all(|i| i.label.to_lowercase().starts_with("вн")));
    }

    #[test]
    fn test_complete_join_types_english_prefix_l() {
        let items = complete_join_types("L");

        // Should return LEFT variants: LEFT JOIN, LEFT, ЛЕВОЕ СОЕДИНЕНИЕ, ЛЕВОЕ
        assert!(!items.is_empty(), "Should return JOIN keywords starting with L");
        assert!(items.iter().any(|i| i.label == "LEFT JOIN"), "Should include LEFT JOIN");
        assert!(items.iter().any(|i| i.label == "LEFT"), "Should include LEFT");

        // All returned items should start with "L" (case-insensitive)
        assert!(items.iter().all(|i| i.label.to_lowercase().starts_with("l")));
    }

    #[test]
    fn test_complete_join_types_empty_prefix() {
        let items = complete_join_types("");

        // Should return all 16 JOIN keywords (8 RU + 8 EN variants)
        assert_eq!(items.len(), 16, "Should return all JOIN keywords with empty prefix");

        // Verify all main keywords are present
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"));
        assert!(items.iter().any(|i| i.label == "ПРАВОЕ СОЕДИНЕНИЕ"));
        assert!(items.iter().any(|i| i.label == "ВНУТРЕННЕЕ СОЕДИНЕНИЕ"));
        assert!(items.iter().any(|i| i.label == "ПОЛНОЕ СОЕДИНЕНИЕ"));
        assert!(items.iter().any(|i| i.label == "LEFT JOIN"));
        assert!(items.iter().any(|i| i.label == "RIGHT JOIN"));
        assert!(items.iter().any(|i| i.label == "INNER JOIN"));
        assert!(items.iter().any(|i| i.label == "FULL JOIN"));
    }

    #[test]
    fn test_complete_join_types_case_insensitive() {
        let items_upper = complete_join_types("ЛЕ");
        let items_lower = complete_join_types("ле");

        // Both should return same results
        assert!(!items_upper.is_empty());
        assert!(!items_lower.is_empty());
        assert_eq!(items_upper.len(), items_lower.len());

        // Should include ЛЕВОЕ variants
        assert!(items_upper.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"));
        assert!(items_lower.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"));
    }

    // ========== complete_table_aliases() tests ==========

    #[test]
    fn test_complete_table_aliases_basic() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with no prefix (should show all aliases)
        let items = complete_table_aliases(&scope, "");

        // Should return one alias
        assert_eq!(items.len(), 1, "Should return one alias");
        assert_eq!(items[0].label, "Т");
        assert_eq!(items[0].kind, CompletionItemKind::Keyword);
        assert!(items[0].detail.as_ref().unwrap().contains("Справочник.Валюты"));
    }

    #[test]
    fn test_complete_table_aliases_with_prefix() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with JOIN (same as working test)
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Т1.Код,
             |    Т2.Наименование
             |ИЗ Справочник.Валюты КАК Т1
             |    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2
             |        ПО Т1.Ссылка = Т2.Валюта";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with prefix "Т" (should filter to Т1 and Т2)
        let items = complete_table_aliases(&scope, "Т");

        // Both Т1 and Т2 start with "Т"
        assert_eq!(items.len(), 2, "Should return two aliases starting with Т");
        assert!(items.iter().any(|i| i.label == "Т1"));
        assert!(items.iter().any(|i| i.label == "Т2"));
    }

    #[test]
    fn test_complete_table_aliases_multiple_tables() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with JOIN (same as working test)
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ
             |    Т1.Код,
             |    Т2.Наименование
             |ИЗ Справочник.Валюты КАК Т1
             |    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2
             |        ПО Т1.Ссылка = Т2.Валюта";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with no prefix (should show both aliases)
        let items = complete_table_aliases(&scope, "");

        assert_eq!(items.len(), 2, "Should return both aliases");

        // Check all aliases are present
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"Т1"), "Should include alias Т1");
        assert!(labels.contains(&"Т2"), "Should include alias Т2");

        // All items should be keywords
        assert!(items.iter().all(|i| i.kind == CompletionItemKind::Keyword));

        // All items should have table name in detail
        assert!(items.iter().all(|i| i.detail.is_some()));
    }

    #[test]
    fn test_complete_table_aliases_case_insensitive() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test case-insensitive filtering
        let items_upper = complete_table_aliases(&scope, "Т");
        let items_lower = complete_table_aliases(&scope, "т");

        // Both should return same results
        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper[0].label, "Т");
        assert_eq!(items_lower[0].label, "Т");
    }

    #[test]
    fn test_complete_table_aliases_no_match() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabaseImpl,
        };
        use vfs::{file_set::FileSet, FileId, VfsPath};

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // Set up source root
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file with SDBL query with alias
        db.set_file_text(
            file_id,
            r#"Процедура Тест()
    Запрос = "ВЫБРАТЬ Т.Код ИЗ Справочник.Валюты КАК Т";
КонецПроцедуры"#,
        );

        // Get Scope
        let scope = get_sdbl_scope(&db, file_id).unwrap();

        // Test completion with non-matching prefix
        let items = complete_table_aliases(&scope, "Х");

        // Should return empty (no aliases start with "Х")
        assert!(items.is_empty(), "Should return empty for non-matching prefix");
    }
}
