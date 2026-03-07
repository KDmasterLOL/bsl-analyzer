use std::str::FromStr;
use text_size::TextSize;

use crate::{AsContext, SdblCompletionContext};

/// Check if a string is an MDO type keyword.
///
/// Returns true for known metadata object type names in both Russian and English.
/// Used by diagnostics to distinguish table aliases from MDO type paths.
pub fn is_mdo_type(s: &str) -> bool {
    let s_upper = s.to_uppercase();
    matches!(
        s_upper.as_str(),
        // Catalogs
        "СПРАВОЧНИК" | "CATALOG" |
        // Documents
        "ДОКУМЕНТ" | "DOCUMENT" |
        // Registers
        "РЕГИСТРСВЕДЕНИЙ" | "INFORMATIONREGISTER" |
        "РЕГИСТРНАКОПЛЕНИЯ" | "ACCUMULATIONREGISTER" |
        "РЕГИСТРБУХГАЛТЕРИИ" | "ACCOUNTINGREGISTER" |
        "РЕГИСТРРАСЧЕТА" | "CALCULATIONREGISTER" |
        // Charts
        "ПЛАНВИДОВХАРАКТЕРИСТИК" | "CHARTOFCHARACTERISTICTYPES" |
        "ПЛАНСЧЕТОВ" | "CHARTOFACCOUNTS" |
        "ПЛАНВИДОВРАСЧЕТА" | "CHARTOFCALCULATIONTYPES" |
        // Business processes
        "БИЗНЕСПРОЦЕСС" | "BUSINESSPROCESS" |
        "ЗАДАЧА" | "TASK" |
        // Other
        "ПЕРЕЧИСЛЕНИЕ" | "ENUM" |
        "ОБРАБОТКА" | "DATAPROCESSOR" |
        "ОТЧЕТ" | "REPORT" |
        "КОНСТАНТА" | "CONSTANT" |
        "ПОСЛЕДОВАТЕЛЬНОСТЬ" | "SEQUENCE" |
        "КРИТЕРИЙОТБОРА" | "FILTERCRITERION" |
        "ПЛАНОБМЕНА" | "EXCHANGEPLAN" |
        "ВНЕШНИЙИСТОЧНИКДАННЫХ" | "EXTERNALDATASOURCE"
    )
}

/// Check if column reference text represents a nested field access (dereference).
///
/// Returns `Some((alias, fields))` if text has 3+ dot-separated parts where
/// the first part is a valid table alias (not an MDO type keyword).
///
/// # Examples
/// - `"T.Ссылка.Организация"` -> `Some(("T", ["Ссылка", "Организация"]))`
/// - `"T.Ссылка"` -> `None` (only 2 parts - not nested)
/// - `"Справочник.Валюты.Код"` -> `None` (MDO type, not alias)
///
/// # Usage
/// - Diagnostics: QueryNestedFieldsByDot detection during HIR lowering
/// - Autocomplete: Can be used by `parse_nested_field_chain()`
pub fn parse_nested_column_ref(text: &str) -> Option<(String, Vec<String>)> {
    let parts: Vec<&str> = text.split('.').collect();

    // Need 3+ parts: alias.field1.field2...
    if parts.len() < 3 {
        return None;
    }

    let potential_alias = parts[0].trim();

    // Check heuristics for table alias:
    // 1. Not empty
    // 2. Starts with uppercase letter
    // 3. NOT an MDO type keyword
    if potential_alias.is_empty() {
        return None;
    }

    if !potential_alias.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
        return None;
    }

    if is_mdo_type(potential_alias) {
        return None;
    }

    let alias = potential_alias.to_string();
    let fields: Vec<String> = parts[1..].iter().map(|s| s.trim().to_string()).collect();

    Some((alias, fields))
}

/// Parse nested field reference chain from "alias.field1.field2.field3..." pattern.
///
/// Returns (alias, field_chain, prefix) where field_chain contains all intermediate fields
/// and prefix is the incomplete field name being typed.
///
/// # Distinguishing from simple 2-part paths
///
/// This function handles 3+ part paths (nested fields), while `parse_table_alias_field()`
/// handles 2-part paths (simple field access). The distinction:
///
/// - 2 parts: "Т.Код" -> handled by `parse_table_alias_field()`
/// - 3+ parts: "Т.Владелец.Родитель.Код" -> handled by this function
///
/// # Heuristics
///
/// First component is considered an alias if:
/// - Not empty
/// - Starts with uppercase letter
/// - NOT an MDO type keyword (Справочник, Документ, etc.)
///
/// # Examples
///
/// ```ignore
/// parse_nested_field_chain("ВЫБРАТЬ Т.Владелец.Родитель.Код")
/// // -> Some(("Т", vec!["Владелец", "Родитель"], "Код"))
///
/// parse_nested_field_chain("ВЫБРАТЬ Т.ВидНоменклатуры.")
/// // -> Some(("Т", vec!["ВидНоменклатуры"], ""))
///
/// parse_nested_field_chain("ВЫБРАТЬ Т.Код")
/// // -> None (only 2 parts, handled by parse_table_alias_field)
///
/// parse_nested_field_chain("ВЫБРАТЬ Справочник.Валюты.Штрихкоды")
/// // -> None (MDO type, not alias)
/// ```
fn parse_nested_field_chain(text_before: &str) -> Option<(String, Vec<String>, String)> {
    // Get last whitespace-separated word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // Check if it contains a dot (otherwise not a path)
    if !last_word.contains('.') {
        return None;
    }

    // Split by dots
    let parts: Vec<&str> = last_word.split('.').collect();

    // We only handle 3+ part patterns (nested fields)
    // 2-part patterns are handled by parse_table_alias_field()
    if parts.len() < 3 {
        return None;
    }

    let potential_alias = parts[0];

    // Check heuristics for table alias (same as parse_table_alias_field):
    // 1. Not empty
    // 2. Starts with uppercase letter
    // 3. NOT an MDO type keyword
    if !potential_alias.is_empty()
        && potential_alias.chars().next()?.is_uppercase()
        && !is_mdo_type(potential_alias)
    {
        let alias = potential_alias.to_string();

        // Middle parts are the field chain (all except first and last)
        let field_chain: Vec<String> =
            parts[1..parts.len() - 1].iter().map(|s| s.to_string()).collect();

        // Last part is the prefix for filtering
        let prefix = parts.last().unwrap().to_string();

        return Some((alias, field_chain, prefix));
    }

    None
}

/// Parse table alias and field name from "alias.field" pattern.
///
/// Distinguishes between table aliases (e.g., "Т", "Т1", "Очередь") and MDO types (e.g., "Справочник").
///
/// # Heuristics
///
/// A word before dot is considered an alias if:
/// - Length is 1-20 characters (reasonable for typical aliases)
/// - Starts with uppercase letter (Т, Т1, Очередь, Items)
/// - NOT an MDO type keyword (Справочник, Документ, etc.)
///
/// # Examples
///
/// ```ignore
/// parse_table_alias_field("ВЫБРАТЬ Т.Код") -> Some(("Т", "Код"))
/// parse_table_alias_field("ВЫБРАТЬ Очередь.") -> Some(("Очередь", ""))
/// parse_table_alias_field("ВЫБРАТЬ Справочник.Валюты") -> None (MDO type, not alias)
/// ```
fn parse_table_alias_field(text_before: &str) -> Option<(String, String)> {
    // Get last whitespace-separated word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // Check if it contains a dot (otherwise not a path)
    if !last_word.contains('.') {
        return None;
    }

    // Split by dots
    let parts: Vec<&str> = last_word.split('.').collect();

    // We only handle 2-part patterns: "Alias.Field"
    if parts.len() != 2 {
        return None;
    }

    let potential_alias = parts[0];
    let field_prefix = parts[1];

    // Extract the LAST identifier from potential_alias
    // This handles cases like:
    // - "(ЧекККМ" -> "ЧекККМ" (parentheses)
    // - "ЕСТЬNULL(КурсыВалют" -> "КурсыВалют" (function call)
    // - "+Таблица" -> "Таблица" (operator)
    //
    // Walk backwards from the end to find the last contiguous identifier
    let clean_alias = extract_last_identifier(potential_alias);

    // Check heuristics for table alias:
    // 1. Not empty after cleanup
    // 2. Starts with uppercase (Т, Т1, Т2, Очередь, Items, ДескрипторыДоступаРегистров, etc.)
    // 3. NOT an MDO type keyword (Справочник, Документ, etc.)
    //
    // Note: No length limit - if someone wants to use a very long alias name, that's their choice
    if !clean_alias.is_empty()
        && clean_alias.chars().next()?.is_uppercase()
        && !is_mdo_type(&clean_alias)
    {
        return Some((clean_alias, field_prefix.to_string()));
    }

    None
}

/// Extract the last identifier from a string that may contain operators/parentheses.
/// For example: "ЕСТЬNULL(КурсыВалют" -> "КурсыВалют"
fn extract_last_identifier(s: &str) -> String {
    // Walk backwards to find the start of the last identifier
    let chars: Vec<char> = s.chars().collect();
    let end = chars.len();
    let mut start = end;

    // Find where the last identifier ends (should be at the end of the string)
    // and where it starts (first non-identifier char going backwards)
    for i in (0..chars.len()).rev() {
        let c = chars[i];
        if c.is_alphanumeric() || c == '_' {
            start = i;
        } else {
            // Hit a non-identifier character, stop
            break;
        }
    }

    if start < end {
        chars[start..end].iter().collect()
    } else {
        String::new()
    }
}

/// Parse CAST expression type from text ending with `).field` or `).<cursor>`
/// Returns (MdoType, object_name, field_chain, prefix) if found
///
/// Example inputs:
/// - "ВЫРАЗИТЬ(X КАК Документ.Продажа)." -> (Document, "Продажа", [], "")
/// - "ВЫРАЗИТЬ(X КАК Документ.Продажа).Контрагент." -> (Document, "Продажа", ["Контрагент"], "")
/// - "ВЫРАЗИТЬ(X КАК Справочник.Номенклатура).Вид" -> (Catalog, "Номенклатура", [], "Вид")
fn parse_cast_field_access(
    text_before: &str,
) -> Option<(bsl_metadata::MdoType, String, Vec<String>, String)> {
    // Find the last `)` - this closes the CAST expression
    let last_rparen = text_before.rfind(')')?;

    // Text after `)` should contain `.field.field...`
    let after_paren = &text_before[last_rparen + 1..];

    // Must start with `.` for field access
    if !after_paren.starts_with('.') {
        return None;
    }

    // Parse field chain and prefix from text after `).`
    let field_access = &after_paren[1..]; // skip the first dot
    let parts: Vec<&str> = field_access.split('.').collect();

    // Last part is prefix, rest are field chain
    let (field_chain, prefix) = if parts.is_empty() {
        (vec![], String::new())
    } else if parts.len() == 1 {
        (vec![], parts[0].to_string())
    } else {
        let chain: Vec<String> = parts[..parts.len() - 1].iter().map(|s| s.to_string()).collect();
        (chain, parts[parts.len() - 1].to_string())
    };

    // Now find the CAST type by looking backwards from `)`
    // We need to find "КАК MDOType.ObjectName" pattern
    let before_paren = &text_before[..last_rparen];

    // Find "КАК" or "AS" keyword
    let as_keyword_pos = find_last_as_keyword(before_paren)?;
    let after_as = before_paren[as_keyword_pos..].trim_start();

    // Skip "КАК " or "AS " - must use byte lengths correctly for UTF-8
    // "КАК" is 6 bytes in UTF-8 (2 bytes per Cyrillic char), "AS" is 2 bytes
    let type_start = if after_as.to_uppercase().starts_with("КАК") {
        "КАК".len() // 6 bytes
    } else if after_as.to_uppercase().starts_with("AS") {
        "AS".len() // 2 bytes
    } else {
        return None;
    };

    let type_text = after_as[type_start..].trim_start();

    // Parse MDO type: "Документ.Продажа" or "Document.Sale"
    let type_parts: Vec<&str> = type_text.split('.').collect();
    if type_parts.len() < 2 {
        return None;
    }

    let mdo_type_str = type_parts[0].trim();
    let object_name = type_parts[1].trim().to_string();

    // Parse MDO type
    let mdo_type = bsl_metadata::MdoType::from_str(mdo_type_str).ok()?;

    Some((mdo_type, object_name, field_chain, prefix))
}

/// Find position of last AS/КАК keyword (case-insensitive)
fn find_last_as_keyword(text: &str) -> Option<usize> {
    let upper = text.to_uppercase();

    // Find last occurrence of КАК or AS (as whole words)
    let mut last_pos = None;

    // Check for КАК
    for (i, _) in upper.match_indices("КАК") {
        // Verify it's a whole word (not part of another word)
        let before_ok = i == 0 || !text[..i].chars().last().is_some_and(|c| c.is_alphanumeric());
        let after_ok = i + 6 >= text.len() // КАК is 6 bytes in UTF-8
            || !text[i + 6..].chars().next().is_some_and(|c| c.is_alphanumeric());
        if before_ok && after_ok {
            last_pos = Some(i);
        }
    }

    // Check for AS (English)
    for (i, _) in upper.match_indices("AS") {
        // Handle the case where "AS" appears in the original (not uppercased) text
        let original_at_i = &text[i..];
        if original_at_i.to_uppercase().starts_with("AS") {
            let before_ok =
                i == 0 || !text[..i].chars().last().is_some_and(|c| c.is_alphanumeric());
            let after_ok = i + 2 >= text.len()
                || !text[i + 2..].chars().next().is_some_and(|c| c.is_alphanumeric());
            if before_ok && after_ok && (last_pos.is_none() || i > last_pos.unwrap()) {
                last_pos = Some(i);
            }
        }
    }

    last_pos
}

/// Detect completion context within an SDBL query.
///
/// Analyzes the query text and cursor position to determine what kind of
/// completion suggestions are appropriate.
///
/// # Arguments
///
/// * `query_text` - Full SDBL query text
/// * `offset` - Cursor offset within the query (after opening quote)
///
/// # Returns
///
/// `SdblCompletionContext` describing the completion context.
///
/// # Example
///
/// ```ignore
/// use sdbl_hir::{detect_context, SdblCompletionContext};
/// use text_size::TextSize;
///
/// let query = "SELECT * FROM ";
/// let offset = TextSize::from(query.len() as u32);
/// let context = detect_context(query, offset);
///
/// assert!(matches!(context, SdblCompletionContext::AfterFromKeyword));
/// ```
pub fn detect_context(query_text: &str, offset: TextSize) -> SdblCompletionContext {
    let offset_usize: usize = offset.into();

    // Get text before cursor (ensure we're on a char boundary for UTF-8 safety)
    let text_before_cursor = if offset_usize <= query_text.len() {
        // Find the nearest char boundary at or before offset
        let safe_offset = if query_text.is_char_boundary(offset_usize) {
            offset_usize
        } else {
            // Walk backwards to find char boundary
            (0..offset_usize).rev().find(|&i| query_text.is_char_boundary(i)).unwrap_or(0)
        };
        &query_text[..safe_offset]
    } else {
        query_text
    };

    // Safely get last ~100 chars for logging (find char boundary)
    let log_start = text_before_cursor.len().saturating_sub(200); // ~100 chars in UTF-8
    let log_start = (log_start..=text_before_cursor.len())
        .find(|&i| text_before_cursor.is_char_boundary(i))
        .unwrap_or(0);
    let text_before_sample = &text_before_cursor[log_start..];

    tracing::info!(
        query_len = query_text.len(),
        offset = offset_usize,
        text_before_len = text_before_cursor.len(),
        text_before_sample = %text_before_sample,
        "detect_context"
    );

    // Check for VALUE() function pattern FIRST (before other patterns)
    // This must come before parse_table_alias_field and parse_dot_path
    // to avoid mis-interpreting "ЗНАЧЕНИЕ(Перечисление." as a table alias
    if let Some((parts, prefix)) = parse_value_function_path(text_before_cursor) {
        return match parts.len() {
            0 => {
                tracing::info!("detected InsideValueFunction");
                SdblCompletionContext::InsideValueFunction
            }
            1 => {
                if let Ok(mdo_type) = parts[0].parse::<bsl_metadata::MdoType>() {
                    // Determine language based on the MDO type keyword
                    let is_russian = mdo_type.is_russian_keyword(&parts[0]);
                    tracing::info!(?mdo_type, prefix = %prefix, is_russian, "detected InsideValueMdoType");
                    SdblCompletionContext::InsideValueMdoType { mdo_type, prefix, is_russian }
                } else {
                    tracing::info!("detected InsideValueFunction (invalid MDO type)");
                    SdblCompletionContext::InsideValueFunction
                }
            }
            2 => {
                if let Ok(mdo_type) = parts[0].parse::<bsl_metadata::MdoType>() {
                    // Determine language based on the MDO type keyword
                    let is_russian = mdo_type.is_russian_keyword(&parts[0]);
                    tracing::info!(
                        ?mdo_type,
                        object_name = %parts[1],
                        prefix = %prefix,
                        is_russian,
                        "detected InsideValueMdoObject"
                    );
                    SdblCompletionContext::InsideValueMdoObject {
                        mdo_type,
                        object_name: parts[1].clone(),
                        prefix,
                        is_russian,
                    }
                } else {
                    tracing::info!(
                        "detected InsideValueFunction (invalid MDO type in 3-part path)"
                    );
                    SdblCompletionContext::InsideValueFunction
                }
            }
            _ => {
                tracing::info!("detected None (too many parts in VALUE path)");
                SdblCompletionContext::None
            }
        };
    }

    // Check for CAST expression field access pattern: ВЫРАЗИТЬ(...).field
    // IMPORTANT: Check this BEFORE nested field chain to detect CAST-specific patterns
    if let Some((mdo_type, object_name, field_chain, prefix)) =
        parse_cast_field_access(text_before_cursor)
    {
        tracing::info!(
            ?mdo_type,
            object_name = %object_name,
            field_chain_len = field_chain.len(),
            prefix = %prefix,
            "detected AfterCastExpression"
        );
        return SdblCompletionContext::AfterCastExpression {
            mdo_type,
            object_name,
            field_chain,
            prefix,
        };
    }

    // Check for "FROM " or "ИЗ " keyword immediately before cursor
    if is_after_from_keyword(text_before_cursor) {
        tracing::info!("detected AfterFromKeyword");
        return SdblCompletionContext::AfterFromKeyword;
    }

    // Check for nested field chain pattern (3+ parts: "Т.Field1.Field2.Field3...")
    // IMPORTANT: Check this BEFORE parse_table_alias_field (which handles 2-part paths)
    // This ensures nested chains are detected first, before falling through to simple field access
    if let Some((alias, field_chain, prefix)) = parse_nested_field_chain(text_before_cursor) {
        tracing::info!(
            alias = %alias,
            field_chain_len = field_chain.len(),
            prefix = %prefix,
            "detected AfterNestedField (nested field chain pattern)"
        );
        return SdblCompletionContext::AfterNestedField { alias, field_chain, prefix };
    }

    // Check for table alias field pattern (e.g., "Т.Код" or "Т1.")
    // IMPORTANT: Check this EARLY to avoid misinterpreting as other contexts
    // For example, "ПО Т." should be AfterTableAlias, not AfterOnKeyword
    if let Some((alias, prefix)) = parse_table_alias_field(text_before_cursor) {
        tracing::info!(
            alias = %alias,
            prefix = %prefix,
            "detected AfterTableAlias (alias.field pattern)"
        );
        return SdblCompletionContext::AfterTableAlias { alias, prefix };
    }

    // Check for ON/ПО keyword (suggest table aliases)
    if is_after_on_keyword(text_before_cursor) {
        // Extract word AFTER the ON/ПО keyword (not the keyword itself)
        let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
        let last_word = words.last().unwrap_or(&"");

        // If last word is ON/ПО, prefix is empty
        // Otherwise, prefix is the word after ON/ПО
        let prefix = if last_word.to_uppercase() == "ПО" || last_word.to_uppercase() == "ON" {
            String::new()
        } else {
            last_word.to_string()
        };

        tracing::info!(prefix = %prefix, "detected AfterOnKeyword");
        return SdblCompletionContext::AfterOnKeyword { prefix };
    }

    // Check for AS/КАК keyword (suggest alias name)
    if let Some((context, suggestion)) = is_after_as_keyword(text_before_cursor) {
        tracing::info!(
            ?context,
            suggestion = ?suggestion,
            "detected AfterAsKeyword"
        );
        return SdblCompletionContext::AfterAsKeyword { context, suggestion };
    }

    // Check for JOIN type keyword context (suggest JOIN keywords)
    if let Some(prefix) = is_join_type_context(text_before_cursor) {
        tracing::info!(prefix = %prefix, "detected JoinTypeKeyword context");
        return SdblCompletionContext::JoinTypeKeyword { prefix };
    }

    // Check for dot-separated path (handles both 2-part and 3-part paths)
    if let Some(path_parts) = parse_dot_path(text_before_cursor) {
        match path_parts.len() {
            // "Справочник.Вал" -> InsideMdoType
            2 => {
                if let Ok(mdo_type) = path_parts[0].parse::<bsl_metadata::MdoType>() {
                    let prefix = path_parts[1].clone();
                    tracing::info!(?mdo_type, prefix = %prefix, "detected InsideMdoType (2-part path)");
                    return SdblCompletionContext::InsideMdoType { mdo_type, prefix };
                }
            }
            // "Справочник.Номенклатура.Шт" -> AfterMdoObject
            3 => {
                if let Ok(mdo_type) = path_parts[0].parse::<bsl_metadata::MdoType>() {
                    let object_name = path_parts[1].clone();
                    let prefix = path_parts[2].clone();
                    tracing::info!(
                        ?mdo_type,
                        object_name = %object_name,
                        prefix = %prefix,
                        "detected AfterMdoObject (3-part path)"
                    );
                    return SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix };
                }
            }
            _ => {
                tracing::debug!(parts_len = path_parts.len(), "unexpected path parts count");
            }
        }
    }

    // Default: suggest SDBL keywords
    // Extract last word as prefix for filtering
    let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
    let prefix = words.last().map(|s| s.to_string()).unwrap_or_default();

    tracing::info!(prefix = %prefix, "detected SdblKeywords context");
    SdblCompletionContext::SdblKeywords { prefix }
}

/// Parse path inside VALUE() function.
///
/// Detects patterns like:
/// - "ЗНАЧЕНИЕ(<cursor>)" -> ([], "")
/// - "ЗНАЧЕНИЕ(Перечисление.<cursor>)" -> (["Перечисление"], "")
/// - "ЗНАЧЕНИЕ(Перечисление.Статусы.<cursor>)" -> (["Перечисление", "Статусы"], "")
/// - "ЗНАЧЕНИЕ(Перечисление.Стат<cursor>)" -> (["Перечисление"], "Стат")
///
/// Returns Some((completed_parts, prefix)) if inside VALUE(), None otherwise.
fn parse_value_function_path(text_before: &str) -> Option<(Vec<String>, String)> {
    // Find the last opening parenthesis
    let open_paren_pos = text_before.rfind('(')?;

    // Check if there's text before the paren ending with ЗНАЧЕНИЕ or VALUE
    let before_paren = &text_before[..open_paren_pos].trim_end();
    let before_upper = before_paren.to_uppercase();

    if !before_upper.ends_with("ЗНАЧЕНИЕ") && !before_upper.ends_with("VALUE") {
        return None;
    }

    // Extract content after the opening paren
    let inside_paren = &text_before[open_paren_pos + 1..].trim_start();

    // Check that there's no closing paren (we're still inside the function)
    if inside_paren.contains(')') {
        return None;
    }

    // Split by dots
    let segments: Vec<&str> = inside_paren.split('.').collect();

    if segments.is_empty() || (segments.len() == 1 && segments[0].is_empty()) {
        // Empty or just whitespace -> ЗНАЧЕНИЕ(<cursor>)
        return Some((vec![], String::new()));
    }

    // Last segment is the prefix (possibly incomplete)
    let prefix = segments.last().unwrap().to_string();

    // Everything before last segment are completed parts
    let completed_parts: Vec<String> =
        segments[..segments.len().saturating_sub(1)].iter().map(|s| s.to_string()).collect();

    Some((completed_parts, prefix))
}

/// Check if cursor is after FROM/ИЗ keyword.
///
/// Looks for pattern: "... FROM " or "... ИЗ " at the end of text.
fn is_after_from_keyword(text_before: &str) -> bool {
    let text_upper = text_before.to_uppercase();

    // Check if text ends with FROM or ИЗ followed by whitespace
    text_upper.trim_end().ends_with("FROM") || text_upper.trim_end().ends_with("ИЗ")
}

/// Parse dot-separated path from text before cursor.
///
/// Extracts the last whitespace-separated word and splits it by dots.
/// Returns the parts as a Vec of Strings.
///
/// # Examples
///
/// ```ignore
/// parse_dot_path("SELECT * FROM Справочник.Номенклатура.Шт")
/// // -> Some(vec!["Справочник", "Номенклатура", "Шт"])
///
/// parse_dot_path("SELECT * FROM Справочник.")
/// // -> Some(vec!["Справочник", ""])
///
/// parse_dot_path("SELECT * FROM NoDotsHere")
/// // -> None (no dots)
/// ```
fn parse_dot_path(text_before: &str) -> Option<Vec<String>> {
    // Get last whitespace-separated word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // Check if it contains a dot (otherwise not a path)
    if !last_word.contains('.') {
        return None;
    }

    // Split by dots and collect into Vec<String>
    let parts: Vec<String> = last_word.split('.').map(|s| s.to_string()).collect();

    Some(parts)
}

/// Check if a string contains SDBL keywords.
///
/// This is a heuristic check - we look for common SDBL keywords like
/// ВЫБРАТЬ, SELECT, ИЗ, FROM, etc.
pub(crate) fn is_sdbl_query(text: &str) -> bool {
    let text_upper = text.to_uppercase();

    // Russian keywords
    text_upper.contains("ВЫБРАТЬ")
        || text_upper.contains("ВЫБОР")
        // English keywords
        || text_upper.contains("SELECT")
        // FROM clause (both languages)
        || text_upper.contains("ИЗ ")
        || text_upper.contains("FROM ")
}

/// Check if cursor is after AS/КАК keyword and determine context.
///
/// Returns AsContext if the last word before cursor is AS/КАК, along with suggested alias.
///
/// # Algorithm
///
/// 1. Check if last word is AS/КАК
/// 2. Look backwards for context keywords:
///    - ВЫБРАТЬ/SELECT -> InSelectField (suggest field name from expression)
///    - ИЗ/FROM -> InFromClause (suggest table name)
///    - СОЕДИНЕНИЕ/JOIN -> InJoinClause (suggest table name)
///
/// # Examples
///
/// ```ignore
/// is_after_as_keyword("ВЫБРАТЬ Т.Код КАК")
/// // -> Some((InSelectField, Some("Код")))
///
/// is_after_as_keyword("ИЗ Справочник.Номенклатура КАК")
/// // -> Some((InFromClause, Some("Номенклатура")))
/// ```
fn is_after_as_keyword(text_before: &str) -> Option<(AsContext, Option<String>)> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Check if last word is AS/КАК
    let last_word_upper = words.last()?.to_uppercase();
    if last_word_upper != "КАК" && last_word_upper != "AS" {
        return None;
    }

    // Look backwards to determine context and extract suggestion
    let text_upper = text_before.to_uppercase();

    // Find the most recent context keyword (working backwards)
    let context = if text_upper.contains("ВЫБРАТЬ") || text_upper.contains("SELECT") {
        // Check if we're in FROM/JOIN or still in SELECT
        let last_from = text_upper.rfind("ИЗ").or(text_upper.rfind("FROM"));
        let last_join = text_upper.rfind("СОЕДИНЕНИЕ").or(text_upper.rfind("JOIN"));
        let last_select = text_upper.rfind("ВЫБРАТЬ").or(text_upper.rfind("SELECT"));

        // If FROM/JOIN appears after SELECT, we're in table context
        let in_table_context = last_from
            .or(last_join)
            .map(|pos| last_select.map(|sel_pos| pos > sel_pos).unwrap_or(true))
            .unwrap_or(false);

        if in_table_context {
            if last_join.is_some() && last_join > last_from {
                AsContext::InJoinClause
            } else {
                AsContext::InFromClause
            }
        } else {
            AsContext::InSelectField
        }
    } else if text_upper.contains("ИЗ") || text_upper.contains("FROM") {
        // In FROM clause (no SELECT found)
        AsContext::InFromClause
    } else {
        // Default to FROM clause
        AsContext::InFromClause
    };

    // Extract suggestion based on context
    let suggestion = match context {
        AsContext::InSelectField => extract_field_name_before_as(text_before),
        AsContext::InFromClause | AsContext::InJoinClause => {
            extract_table_name_before_as(text_before)
        }
    };

    Some((context, suggestion))
}

/// Extract field name from expression before AS keyword.
///
/// Parses expressions like "Т.Код КАК" -> "Код" or "Т.ВидНоменклатуры.Наименование КАК" -> "Наименование"
fn extract_field_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Get word before AS/КАК
    let expression = words[words.len() - 2];

    // If it contains dots, take the last part
    if expression.contains('.') {
        let parts: Vec<&str> = expression.split('.').collect();
        return Some(parts.last()?.to_string());
    }

    // Simple identifier
    Some(expression.to_string())
}

/// Extract table name from MDO reference before AS keyword.
///
/// Parses patterns like:
/// - "Справочник.Номенклатура КАК" -> "Номенклатура"
/// - "Документ.Продажа.Товары КАК" -> "Товары" (tabular section)
fn extract_table_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    // Get word before AS/КАК
    let reference = words[words.len() - 2];

    // Parse dot-separated path
    if reference.contains('.') {
        let parts: Vec<&str> = reference.split('.').collect();

        // For "Справочник.Номенклатура", return "Номенклатура"
        // For "Документ.Продажа.Товары", return "Товары"
        return Some(parts.last()?.to_string());
    }

    None
}

/// Check if cursor is after ON/ПО keyword.
///
/// Returns true if:
/// - Text ends with "ПО" or "ON" (e.g., "...JOIN Table AS T ON")
/// - Last word before cursor came after "ПО" or "ON" (e.g., "...ON Т")
fn is_after_on_keyword(text_before: &str) -> bool {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.is_empty() {
        return false;
    }

    // Check if last word is ON/ПО
    let last_word = words.last().unwrap().to_uppercase();
    if last_word == "ПО" || last_word == "ON" {
        return true;
    }

    // Check if second-to-last word is ON/ПО (for "...ON prefix" case)
    if words.len() >= 2 {
        let prev_word = words[words.len() - 2].to_uppercase();
        if prev_word == "ПО" || prev_word == "ON" {
            return true;
        }
    }

    false
}

/// Check if cursor is at a position where JOIN type keyword should appear.
///
/// Detects patterns where user is starting to type a JOIN keyword after a table alias.
/// Examples:
/// - "ИЗ Справочник.Валюты КАК Т Л" (starting "ЛЕВОЕ")
/// - "... КАК Т1 ВН" (starting "ВНУТРЕННЕЕ")
///
/// # Algorithm
///
/// 1. Check if there's a КАК/AS keyword in the text (indicates table alias exists)
/// 2. Last word should NOT be a complete SQL keyword (FROM, WHERE, etc.)
/// 3. Last word should be a short partial word (1-3 chars starting with uppercase)
fn is_join_type_context(text_before: &str) -> Option<String> {
    let text_upper = text_before.to_uppercase();

    // Must have КАК/AS in the text (indicates table alias context)
    if !text_upper.contains("КАК") && !text_upper.contains("AS") {
        return None;
    }

    // Already in a JOIN clause - don't suggest again
    if text_upper.ends_with("СОЕДИНЕНИЕ") || text_upper.ends_with("JOIN") {
        return None;
    }

    // Don't suggest JOIN keywords after ON/ПО (that's for ON clause, not JOIN)
    let text_trimmed = text_upper.trim_end();
    if text_trimmed.ends_with("ПО") || text_trimmed.ends_with("ON") {
        return None;
    }

    // Get last word
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    // If word contains a dot, it's likely an alias field pattern (e.g., "Т."), not a JOIN keyword
    if last_word.contains('.') {
        return None;
    }

    // Check if it's a partial word that could be a JOIN keyword
    // Russian: ЛЕВОЕ (Л), ПРАВОЕ (ПР), ВНУТРЕННЕЕ (ВН), ПОЛНОЕ (ПОЛ)
    // English: LEFT (L), RIGHT (R), INNER (I), FULL (F)
    if !last_word.is_empty() && last_word.len() <= 4 {
        let first_char = last_word.chars().next()?;
        if first_char.is_uppercase() {
            // Check it's not a complete keyword we already handle
            let word_upper = last_word.to_uppercase();
            if !matches!(
                word_upper.as_str(),
                "ИЗ" | "FROM"
                    | "ГДЕ"
                    | "WHERE"
                    | "И"
                    | "AND"
                    | "ИЛИ"
                    | "OR"
                    | "КАК"
                    | "AS"
                    | "ПО"
                    | "ON"
            ) {
                return Some(last_word.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_sdbl_query_russian() {
        assert!(is_sdbl_query("ВЫБРАТЬ * ИЗ Справочник.Валюты"));
        assert!(is_sdbl_query("выбрать * из справочник.валюты"));
        assert!(is_sdbl_query("\"ВЫБРАТЬ * ИЗ Справочник.Валюты\""));
    }

    #[test]
    fn test_is_sdbl_query_english() {
        assert!(is_sdbl_query("SELECT * FROM Catalog.Currencies"));
        assert!(is_sdbl_query("select * from catalog.currencies"));
        assert!(is_sdbl_query("\"SELECT * FROM Catalog.Currencies\""));
    }

    #[test]
    fn test_is_sdbl_query_negative() {
        assert!(!is_sdbl_query("Это обычная строка"));
        assert!(!is_sdbl_query("Regular string without keywords"));
        assert!(!is_sdbl_query("123"));
        assert!(!is_sdbl_query(""));
    }

    // --- Context detection tests ---

    #[test]
    fn test_detect_context_incomplete_table_ref() {
        // User's case: cursor after "Справочник."
        let query = r#"ВЫБРАТЬ
    Валюты.Наименование
ИЗ
    Справочник."#;

        let offset = query.len() as u32; // cursor at end
        let context = detect_context(query, offset.into());

        println!("Query: {:?}", query);
        println!("Offset: {}", offset);
        println!("Context: {:?}", context);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                println!("InsideMdoType detected!");
                println!("  mdo_type: {:?}", mdo_type);
                println!("  prefix: {:?}", prefix);
                assert_eq!(prefix, "");
            }
            other => {
                panic!("Expected InsideMdoType, got: {:?}", other);
            }
        }
    }

    #[test]
    fn test_detect_context_after_from_russian() {
        let query = "ВЫБРАТЬ * ИЗ ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert_eq!(context, SdblCompletionContext::AfterFromKeyword);
    }

    #[test]
    fn test_detect_context_after_from_english() {
        let query = "SELECT * FROM ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert_eq!(context, SdblCompletionContext::AfterFromKeyword);
    }

    #[test]
    fn test_detect_context_inside_mdo_type_russian() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Вал";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "Вал");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_english() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Catalog.Curr";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "Curr");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_document() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Документ.Заказ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Document);
                assert_eq!(prefix, "Заказ");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_mdo_type_empty_prefix() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Catalog.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_keywords_in_select() {
        let query = "ВЫБРАТЬ * ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // Now returns SdblKeywords instead of None
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "*");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_keywords_inside_word() {
        let query = "ВЫБРАТЬ * ИЗ Спр";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // "Спр" without dot - suggest keywords
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "Спр");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_register() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM InformationRegister.Settings";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(prefix, "Settings");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_completion_after_mdo_type_with_trailing_space() {
        use bsl_metadata::MdoType;

        // Test with trailing space after dot (common when typing)
        let query = "ВЫБРАТЬ * ИЗ РегистрСведений. ";
        let offset = TextSize::from((query.len() - 1) as u32); // Position after dot, before space
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    // --- AfterMdoObject context tests ---

    #[test]
    fn test_detect_context_after_catalog_object() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Номенклатура.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(object_name, "Номенклатура");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_catalog_object_with_prefix() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ Справочник.Номенклатура.Шт";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Catalog);
                assert_eq!(object_name, "Номенклатура");
                assert_eq!(prefix, "Шт");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_document_object() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM Document.SalesOrder.T";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::Document);
                assert_eq!(object_name, "SalesOrder");
                assert_eq!(prefix, "T");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_information_register() {
        use bsl_metadata::MdoType;

        let query = "ВЫБРАТЬ * ИЗ РегистрСведений.МойРегистр.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(object_name, "МойРегистр");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_accumulation_register() {
        use bsl_metadata::MdoType;

        let query = "SELECT * FROM AccumulationRegister.TaskCount.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterMdoObject { mdo_type, object_name, prefix } => {
                assert_eq!(mdo_type, MdoType::AccumulationRegister);
                assert_eq!(object_name, "TaskCount");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_parse_dot_path_helper() {
        // 2-part path
        let parts = parse_dot_path("SELECT * FROM Справочник.Вал");
        assert_eq!(parts, Some(vec!["Справочник".to_string(), "Вал".to_string()]));

        // 3-part path
        let parts = parse_dot_path("SELECT * FROM Справочник.Номенклатура.Шт");
        assert_eq!(
            parts,
            Some(vec!["Справочник".to_string(), "Номенклатура".to_string(), "Шт".to_string()])
        );

        // Empty prefix after dot
        let parts = parse_dot_path("SELECT * FROM Catalog.");
        assert_eq!(parts, Some(vec!["Catalog".to_string(), "".to_string()]));

        // No dots - should return None
        let parts = parse_dot_path("SELECT * FROM NoDots");
        assert_eq!(parts, None);
    }

    // --- SdblKeywords context tests ---

    #[test]
    fn test_detect_context_sdbl_keywords_simple() {
        let query = "ВЫБРАТЬ * ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "*");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_partial_word() {
        let query = "ГДЕ";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "ГДЕ");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_empty() {
        let query = "";
        let offset = TextSize::from(0);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_sdbl_keywords_after_space() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты ГД";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "ГД");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    // ========== AfterTableAlias tests ==========

    #[test]
    fn test_detect_context_after_table_alias_no_prefix() {
        let query = "ВЫБРАТЬ Т.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_table_alias_with_prefix() {
        let query = "ВЫБРАТЬ Т.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_table_alias_multichar() {
        let query = "ВЫБРАТЬ Т1.Наименование";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т1");
                assert_eq!(prefix, "Наименование");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_alias_справочник() {
        // "Справочник." should be recognized as InsideMdoType, NOT AfterTableAlias
        let query = "ВЫБРАТЬ * ИЗ Справочник.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Catalog);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_alias_document() {
        // "Документ.Продажа" should be recognized as InsideMdoType, NOT AfterTableAlias
        let query = "ВЫБРАТЬ * ИЗ Документ.Продажа";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(prefix, "Продажа");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_in_where_clause() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты КАК Т ГДЕ Т.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_in_join() {
        let query = "ВЫБРАТЬ Т2.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т2");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_long_alias() {
        // Test for longer alias names (e.g., "Очередь", "Items")
        let query = "ВЫБРАТЬ Очередь.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Очередь");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias for long alias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_long_alias_with_prefix() {
        let query = "ВЫБРАТЬ Очередь.Поп";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Очередь");
                assert_eq!(prefix, "Поп");
            }
            _ => panic!("Expected AfterTableAlias for long alias with prefix, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_very_long_alias() {
        // Test for very long alias names (no arbitrary length limit)
        // Real-world example: ДескрипторыДоступаРегистров (27 chars)
        let query = "ВЫБРАТЬ ДескрипторыДоступаРегистров.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "ДескрипторыДоступаРегистров");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias for very long alias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_extremely_long_alias() {
        // Test for extremely long alias names - no arbitrary limit
        let query =
            "ВЫБРАТЬ ОченьДлинноеНазваниеТаблицыПростоПотомуЧтоМогуНазыватьКакУгодно.Ссылка";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(
                    alias,
                    "ОченьДлинноеНазваниеТаблицыПростоПотомуЧтоМогуНазыватьКакУгодно"
                );
                assert_eq!(prefix, "Ссылка");
            }
            _ => panic!("Expected AfterTableAlias for extremely long alias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_inside_function_call() {
        // Test for alias inside function call: ЕСТЬNULL(КурсыВалют.
        // Should extract "КурсыВалют" as alias, not "ЕСТЬNULL(КурсыВалют"
        let query = "ВЫБРАТЬ ЕСТЬNULL(КурсыВалют.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "КурсыВалют");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias for alias inside function, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_after_operator() {
        // Test for alias after operator: price / КурсыВалют.Курс
        let query = "ВЫБРАТЬ price / КурсыВалют.Курс";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "КурсыВалют");
                assert_eq!(prefix, "Курс");
            }
            _ => panic!("Expected AfterTableAlias for alias after operator, got {:?}", context),
        }
    }

    // ========== AfterAsKeyword tests ==========

    #[test]
    fn test_detect_context_after_as_in_select() {
        let query = "ВЫБРАТЬ Т.Код КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Код".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_select_chain() {
        let query = "ВЫБРАТЬ Т.ВидНоменклатуры.Наименование КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Наименование".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_from() {
        let query = "ИЗ Справочник.Номенклатура КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InFromClause);
                assert_eq!(suggestion, Some("Номенклатура".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_in_join() {
        let query = "ВЫБРАТЬ * ИЗ Справочник.Валюты ЛЕВОЕ СОЕДИНЕНИЕ Документ.Продажа КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InJoinClause);
                assert_eq!(suggestion, Some("Продажа".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_tabular_section() {
        let query = "ИЗ Документ.ЗаказПокупателя.Товары КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InFromClause);
                assert_eq!(suggestion, Some("Товары".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_english() {
        let query = "SELECT T.Code AS";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InSelectField);
                assert_eq!(suggestion, Some("Code".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_as_complex_query() {
        // Test that context detection works correctly in complex queries
        let query = "ВЫБРАТЬ Т1.Код ИЗ Справочник.Валюты КАК Т1 ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterAsKeyword { context, suggestion } => {
                assert_eq!(context, AsContext::InJoinClause);
                assert_eq!(suggestion, Some("Номенклатура".to_string()));
            }
            _ => panic!("Expected AfterAsKeyword, got {:?}", context),
        }
    }

    // ========== JoinTypeKeyword tests ==========

    #[test]
    fn test_detect_context_join_type_russian_l() {
        let query = "ИЗ Справочник.Валюты КАК Т Л";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "Л");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_join_type_russian_vn() {
        let query = "ИЗ Справочник.Валюты КАК Т ВН";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "ВН");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_join_type_english_l() {
        let query = "FROM Catalog.Currencies AS T L";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::JoinTypeKeyword { prefix } => {
                assert_eq!(prefix, "L");
            }
            _ => panic!("Expected JoinTypeKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_not_join_type_without_alias() {
        // Should not detect JOIN context if no КАК/AS present
        let query = "ИЗ Справочник.Валюты Л";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        // Should be keywords, not JoinTypeKeyword
        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "Л");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

    // ========== AfterOnKeyword tests ==========

    #[test]
    fn test_detect_context_after_on_russian() {
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_with_prefix() {
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО Т";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "Т");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_english() {
        let query = "LEFT JOIN Catalog.Items AS T2 ON";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterOnKeyword { prefix } => {
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterOnKeyword, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_after_on_then_alias_field() {
        // After typing "ПО Т.", should detect AfterTableAlias (not AfterOnKeyword)
        let query = "ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Номенклатура КАК Т2 ПО Т.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    // ========================================
    // Tests for parse_nested_field_chain()
    // ========================================

    #[test]
    fn test_parse_nested_field_chain_three_levels() {
        let text = "ВЫБРАТЬ Т.Владелец.Родитель.Код";
        let result = parse_nested_field_chain(text);

        assert_eq!(
            result,
            Some((
                "Т".to_string(),
                vec!["Владелец".to_string(), "Родитель".to_string()],
                "Код".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_nested_field_chain_empty_prefix() {
        let text = "ВЫБРАТЬ Т.ВидНоменклатуры.";
        let result = parse_nested_field_chain(text);

        assert_eq!(
            result,
            Some(("Т".to_string(), vec!["ВидНоменклатуры".to_string()], "".to_string()))
        );
    }

    #[test]
    fn test_parse_nested_field_chain_four_levels() {
        let text = "WHERE Т1.Field1.Field2.Field3.Field4";
        let result = parse_nested_field_chain(text);

        assert_eq!(
            result,
            Some((
                "Т1".to_string(),
                vec!["Field1".to_string(), "Field2".to_string(), "Field3".to_string()],
                "Field4".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_nested_field_chain_two_parts_only() {
        // Should not match simple 2-part path (handled by parse_table_alias_field)
        let text = "ВЫБРАТЬ Т.Код";
        let result = parse_nested_field_chain(text);

        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_nested_field_chain_mdo_type() {
        // Should not match MDO type (not an alias)
        let text = "ВЫБРАТЬ Справочник.Номенклатура.Владелец";
        let result = parse_nested_field_chain(text);

        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_nested_field_chain_english() {
        let text = "SELECT T.Owner.Parent.Code";
        let result = parse_nested_field_chain(text);

        assert_eq!(
            result,
            Some((
                "T".to_string(),
                vec!["Owner".to_string(), "Parent".to_string()],
                "Code".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_nested_field_chain_no_dots() {
        let text = "ВЫБРАТЬ Код";
        let result = parse_nested_field_chain(text);

        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_nested_field_chain_lowercase_alias() {
        // Should not match lowercase alias (not following conventions)
        let text = "ВЫБРАТЬ т.Владелец.Родитель.Код";
        let result = parse_nested_field_chain(text);

        assert_eq!(result, None);
    }

    #[test]
    fn test_detect_context_nested_field_three_levels() {
        let query = "ВЫБРАТЬ Т.Владелец.Родитель.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterNestedField { alias, field_chain, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(field_chain, vec!["Владелец", "Родитель"]);
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterNestedField, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_nested_field_empty_prefix() {
        let query = "ВЫБРАТЬ Т.ВидНоменклатуры.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterNestedField { alias, field_chain, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(field_chain, vec!["ВидНоменклатуры"]);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterNestedField, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_simple_field_not_nested() {
        // Simple 2-part path should still be AfterTableAlias, not AfterNestedField
        let query = "ВЫБРАТЬ Т.Код";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "Т");
                assert_eq!(prefix, "Код");
            }
            _ => panic!("Expected AfterTableAlias for 2-part path, got {:?}", context),
        }
    }

    // VALUE() function completion tests

    #[test]
    fn test_parse_value_function_path_empty() {
        let text = "ЗНАЧЕНИЕ(";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, Vec::<String>::new());
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_parse_value_function_path_after_type() {
        let text = "ЗНАЧЕНИЕ(Перечисление.";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, vec!["Перечисление"]);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_parse_value_function_path_after_object() {
        let text = "ЗНАЧЕНИЕ(Перечисление.МоеПеречисление.";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, vec!["Перечисление", "МоеПеречисление"]);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_parse_value_function_path_with_prefix() {
        let text = "ЗНАЧЕНИЕ(Перечисление.Статус";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, vec!["Перечисление"]);
        assert_eq!(prefix, "Статус");
    }

    #[test]
    fn test_parse_value_function_path_english() {
        let text = "VALUE(Enum.";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, vec!["Enum"]);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_parse_value_function_path_case_insensitive() {
        let text = "значение(перечисление.";
        let result = parse_value_function_path(text);
        assert!(result.is_some());
        let (parts, prefix) = result.unwrap();
        assert_eq!(parts, vec!["перечисление"]);
        assert_eq!(prefix, "");
    }

    #[test]
    fn test_parse_value_function_path_not_value() {
        let text = "ВЫБРАТЬ * ИЗ Справочник.";
        let result = parse_value_function_path(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_value_function_path_closed_paren() {
        let text = "ЗНАЧЕНИЕ(Перечисление.Статус)";
        let result = parse_value_function_path(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_detect_context_inside_value_function() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        assert!(
            matches!(context, SdblCompletionContext::InsideValueFunction),
            "Expected InsideValueFunction, got {:?}",
            context
        );
    }

    #[test]
    fn test_detect_context_inside_value_mdo_type() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(Перечисление.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoType { mdo_type, prefix, is_russian } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Enum);
                assert_eq!(prefix, "");
                assert!(is_russian, "Expected Russian context for 'Перечисление'");
            }
            _ => panic!("Expected InsideValueMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_value_mdo_type_with_prefix() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(Перечисление.Стат";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoType { mdo_type, prefix, is_russian } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Enum);
                assert_eq!(prefix, "Стат");
                assert!(is_russian, "Expected Russian context for 'Перечисление'");
            }
            _ => panic!("Expected InsideValueMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_value_mdo_object() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(Перечисление.Статусы.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoObject {
                mdo_type,
                object_name,
                prefix,
                is_russian,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Enum);
                assert_eq!(object_name, "Статусы");
                assert_eq!(prefix, "");
                assert!(is_russian, "Expected Russian context for 'Перечисление'");
            }
            _ => panic!("Expected InsideValueMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_value_mdo_object_with_prefix() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(Перечисление.Статусы.Акт";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoObject {
                mdo_type,
                object_name,
                prefix,
                is_russian,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Enum);
                assert_eq!(object_name, "Статусы");
                assert_eq!(prefix, "Акт");
                assert!(is_russian, "Expected Russian context for 'Перечисление'");
            }
            _ => panic!("Expected InsideValueMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_value_english() {
        let query = "SELECT VALUE(Enum.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoType { mdo_type, prefix, is_russian } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Enum);
                assert_eq!(prefix, "");
                assert!(!is_russian, "Expected English context for 'Enum'");
            }
            _ => panic!("Expected InsideValueMdoType, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_inside_value_catalog() {
        let query = "ВЫБРАТЬ ЗНАЧЕНИЕ(Справочник.Валюты.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideValueMdoObject {
                mdo_type,
                object_name,
                prefix,
                is_russian,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Catalog);
                assert_eq!(object_name, "Валюты");
                assert_eq!(prefix, "");
                assert!(is_russian, "Expected Russian context for 'Справочник'");
            }
            _ => panic!("Expected InsideValueMdoObject, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_with_parenthesis() {
        // Real-world case: cursor after "(ЧекККМ." in WHERE condition
        let query = r#"ВЫБРАТЬ
    ЧекККМ.Ссылка КАК Ссылка
ИЗ
    Документ.ЧекККМ КАК ЧекККМ
ГДЕ
    (ЧекККМ.Партнер = &Партнер)
    И (ЧекККМ.Проведен = ИСТИНА)
    И (ЧекККМ.ПометкаУдаления = ЛОЖЬ)
    И (ЧекККМ."#;

        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "ЧекККМ", "Should extract alias without parenthesis");
                assert_eq!(prefix, "", "Prefix should be empty after dot");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_alias_with_parenthesis_and_prefix() {
        // Cursor in the middle of field name: "(ЧекККМ.Парт"
        let query = r#"ВЫБРАТЬ
    ЧекККМ.Ссылка
ИЗ
    Документ.ЧекККМ КАК ЧекККМ
ГДЕ
    (ЧекККМ.Парт"#;

        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterTableAlias { alias, prefix } => {
                assert_eq!(alias, "ЧекККМ", "Should extract alias without parenthesis");
                assert_eq!(prefix, "Парт", "Should extract field prefix");
            }
            _ => panic!("Expected AfterTableAlias, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_empty_prefix() {
        // ВЫРАЗИТЬ(...КАК Документ.Продажа).
        let query = "ВЫБРАТЬ ВЫРАЗИТЬ(Т.Регистратор КАК Документ.Продажа).";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(object_name, "Продажа");
                assert!(field_chain.is_empty());
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_with_prefix() {
        // ВЫРАЗИТЬ(...КАК Документ.Продажа).Конт
        let query = "ВЫБРАТЬ ВЫРАЗИТЬ(Т.Регистратор КАК Документ.Продажа).Конт";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(object_name, "Продажа");
                assert!(field_chain.is_empty());
                assert_eq!(prefix, "Конт");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_nested_field() {
        // ВЫРАЗИТЬ(...КАК Документ.Продажа).Контрагент.
        let query = "ВЫБРАТЬ ВЫРАЗИТЬ(Т.Регистратор КАК Документ.Продажа).Контрагент.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(object_name, "Продажа");
                assert_eq!(field_chain, vec!["Контрагент"]);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_catalog() {
        // ВЫРАЗИТЬ(...КАК Справочник.Номенклатура).
        let query = "ВЫБРАТЬ ВЫРАЗИТЬ(Поле КАК Справочник.Номенклатура).";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Catalog);
                assert_eq!(object_name, "Номенклатура");
                assert!(field_chain.is_empty());
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_english() {
        // CAST(... AS Document.Sale).
        let query = "SELECT CAST(T.Recorder AS Document.Sale).";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(object_name, "Sale");
                assert!(field_chain.is_empty());
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    #[test]
    fn test_detect_context_cast_expression_real_world() {
        // Real-world example from user request
        let query = "ЕСТЬNULL(ВЫРАЗИТЬ(БонусныеБаллы.Регистратор КАК Документ.НачислениеИСписаниеБонусныхБаллов).ПричинаНачисленияИСписанияБонусныхБаллов.";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::AfterCastExpression {
                mdo_type,
                object_name,
                field_chain,
                prefix,
            } => {
                assert_eq!(mdo_type, bsl_metadata::MdoType::Document);
                assert_eq!(object_name, "НачислениеИСписаниеБонусныхБаллов");
                assert_eq!(field_chain, vec!["ПричинаНачисленияИСписанияБонусныхБаллов"]);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected AfterCastExpression, got {:?}", context),
        }
    }

    // ========== parse_nested_column_ref tests ==========

    #[test]
    fn test_parse_nested_column_ref_3_parts() {
        // 3 parts: alias.field1.field2
        let result = parse_nested_column_ref("T.Ссылка.Организация");
        assert!(result.is_some());
        let (alias, fields) = result.unwrap();
        assert_eq!(alias, "T");
        assert_eq!(fields, vec!["Ссылка", "Организация"]);
    }

    #[test]
    fn test_parse_nested_column_ref_4_parts() {
        // 4 parts
        let result = parse_nested_column_ref("ЗаказКлиентаТовары.Ссылка.Контрагент.Наименование");
        assert!(result.is_some());
        let (alias, fields) = result.unwrap();
        assert_eq!(alias, "ЗаказКлиентаТовары");
        assert_eq!(fields, vec!["Ссылка", "Контрагент", "Наименование"]);
    }

    #[test]
    fn test_parse_nested_column_ref_2_parts_returns_none() {
        // 2 parts - should return None
        let result = parse_nested_column_ref("T.Ссылка");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_nested_column_ref_mdo_type_returns_none() {
        // MDO type paths should return None
        let result = parse_nested_column_ref("Справочник.Валюты.Код");
        assert!(result.is_none());

        let result = parse_nested_column_ref("Документ.Заказ.Товары");
        assert!(result.is_none());

        let result = parse_nested_column_ref("РегистрСведений.Курсы.Валюта");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_nested_column_ref_lowercase_first_part_returns_none() {
        // First part should start with uppercase
        let result = parse_nested_column_ref("table.Field1.Field2");
        assert!(result.is_none());
    }
}
