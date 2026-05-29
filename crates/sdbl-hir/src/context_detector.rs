use std::str::FromStr;
use text_size::TextSize;

use crate::{AsContext, SdblCompletionContext};

pub fn is_mdo_type(s: &str) -> bool {
    let s_upper = s.to_uppercase();
    matches!(
        s_upper.as_str(),
        "СПРАВОЧНИК"
            | "CATALOG"
            | "ДОКУМЕНТ"
            | "DOCUMENT"
            | "РЕГИСТРСВЕДЕНИЙ"
            | "INFORMATIONREGISTER"
            | "РЕГИСТРНАКОПЛЕНИЯ"
            | "ACCUMULATIONREGISTER"
            | "РЕГИСТРБУХГАЛТЕРИИ"
            | "ACCOUNTINGREGISTER"
            | "РЕГИСТРРАСЧЕТА"
            | "CALCULATIONREGISTER"
            | "ПЛАНВИДОВХАРАКТЕРИСТИК"
            | "CHARTOFCHARACTERISTICTYPES"
            | "ПЛАНСЧЕТОВ"
            | "CHARTOFACCOUNTS"
            | "ПЛАНВИДОВРАСЧЕТА"
            | "CHARTOFCALCULATIONTYPES"
            | "БИЗНЕСПРОЦЕСС"
            | "BUSINESSPROCESS"
            | "ЗАДАЧА"
            | "TASK"
            | "ПЕРЕЧИСЛЕНИЕ"
            | "ENUM"
            | "ОБРАБОТКА"
            | "DATAPROCESSOR"
            | "ОТЧЕТ"
            | "REPORT"
            | "КОНСТАНТА"
            | "CONSTANT"
            | "ПОСЛЕДОВАТЕЛЬНОСТЬ"
            | "SEQUENCE"
            | "КРИТЕРИЙОТБОРА"
            | "FILTERCRITERION"
            | "ПЛАНОБМЕНА"
            | "EXCHANGEPLAN"
            | "ВНЕШНИЙИСТОЧНИКДАННЫХ"
            | "EXTERNALDATASOURCE"
    )
}

pub fn parse_nested_column_ref(text: &str) -> Option<(String, Vec<String>)> {
    let parts: Vec<&str> = text.split('.').collect();

    if parts.len() < 3 {
        return None;
    }

    let potential_alias = parts[0].trim();

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

fn parse_nested_field_chain(text_before: &str) -> Option<(String, Vec<String>, String)> {
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    if !last_word.contains('.') {
        return None;
    }

    let parts: Vec<&str> = last_word.split('.').collect();

    if parts.len() < 3 {
        return None;
    }

    let potential_alias = parts[0];

    if !potential_alias.is_empty()
        && potential_alias.chars().next()?.is_uppercase()
        && !is_mdo_type(potential_alias)
    {
        let alias = potential_alias.to_string();

        let field_chain: Vec<String> =
            parts[1..parts.len() - 1].iter().map(|s| s.to_string()).collect();

        let prefix = parts.last().unwrap().to_string();

        return Some((alias, field_chain, prefix));
    }

    None
}

fn parse_table_alias_field(text_before: &str) -> Option<(String, String)> {
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    if !last_word.contains('.') {
        return None;
    }

    let parts: Vec<&str> = last_word.split('.').collect();

    if parts.len() != 2 {
        return None;
    }

    let potential_alias = parts[0];
    let field_prefix = parts[1];

    let clean_alias = extract_last_identifier(potential_alias);

    if !clean_alias.is_empty()
        && clean_alias.chars().next()?.is_uppercase()
        && !is_mdo_type(&clean_alias)
    {
        return Some((clean_alias, field_prefix.to_string()));
    }

    None
}

fn extract_last_identifier(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let end = chars.len();
    let mut start = end;

    for i in (0..chars.len()).rev() {
        let c = chars[i];
        if c.is_alphanumeric() || c == '_' {
            start = i;
        } else {
            break;
        }
    }

    if start < end {
        chars[start..end].iter().collect()
    } else {
        String::new()
    }
}

fn parse_cast_field_access(
    text_before: &str,
) -> Option<(bsl_metadata::MdoType, String, Vec<String>, String)> {
    let last_rparen = text_before.rfind(')')?;

    let after_paren = &text_before[last_rparen + 1..];

    if !after_paren.starts_with('.') {
        return None;
    }

    let field_access = &after_paren[1..];
    let parts: Vec<&str> = field_access.split('.').collect();

    let (field_chain, prefix) = if parts.is_empty() {
        (vec![], String::new())
    } else if parts.len() == 1 {
        (vec![], parts[0].to_string())
    } else {
        let chain: Vec<String> = parts[..parts.len() - 1].iter().map(|s| s.to_string()).collect();
        (chain, parts[parts.len() - 1].to_string())
    };

    let before_paren = &text_before[..last_rparen];

    let as_keyword_pos = find_last_as_keyword(before_paren)?;
    let after_as = before_paren[as_keyword_pos..].trim_start();

    let type_start = if after_as.to_uppercase().starts_with("КАК") {
        "КАК".len()
    } else if after_as.to_uppercase().starts_with("AS") {
        "AS".len()
    } else {
        return None;
    };

    let type_text = after_as[type_start..].trim_start();

    let type_parts: Vec<&str> = type_text.split('.').collect();
    if type_parts.len() < 2 {
        return None;
    }

    let mdo_type_str = type_parts[0].trim();
    let object_name = type_parts[1].trim().to_string();

    let mdo_type = bsl_metadata::MdoType::from_str(mdo_type_str).ok()?;

    Some((mdo_type, object_name, field_chain, prefix))
}

fn find_last_as_keyword(text: &str) -> Option<usize> {
    let upper = text.to_uppercase();

    let mut last_pos = None;

    for (i, _) in upper.match_indices("КАК") {
        let before_ok = i == 0 || !text[..i].chars().last().is_some_and(|c| c.is_alphanumeric());
        let after_ok = i + 6 >= text.len()
            || !text[i + 6..].chars().next().is_some_and(|c| c.is_alphanumeric());
        if before_ok && after_ok {
            last_pos = Some(i);
        }
    }

    for (i, _) in upper.match_indices("AS") {
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

pub fn detect_context(query_text: &str, offset: TextSize) -> SdblCompletionContext {
    let offset_usize: usize = offset.into();

    let text_before_cursor = if offset_usize <= query_text.len() {
        let safe_offset = if query_text.is_char_boundary(offset_usize) {
            offset_usize
        } else {
            (0..offset_usize).rev().find(|&i| query_text.is_char_boundary(i)).unwrap_or(0)
        };
        &query_text[..safe_offset]
    } else {
        query_text
    };

    let log_start = text_before_cursor.len().saturating_sub(200);
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

    if let Some((parts, prefix)) = parse_value_function_path(text_before_cursor) {
        return match parts.len() {
            0 => {
                tracing::info!("detected InsideValueFunction");
                SdblCompletionContext::InsideValueFunction
            }
            1 => {
                if let Ok(mdo_type) = parts[0].parse::<bsl_metadata::MdoType>() {
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

    if is_after_from_keyword(text_before_cursor) {
        tracing::info!("detected AfterFromKeyword");
        return SdblCompletionContext::AfterFromKeyword;
    }

    if let Some((alias, field_chain, prefix)) = parse_nested_field_chain(text_before_cursor) {
        tracing::info!(
            alias = %alias,
            field_chain_len = field_chain.len(),
            prefix = %prefix,
            "detected AfterNestedField (nested field chain pattern)"
        );
        return SdblCompletionContext::AfterNestedField { alias, field_chain, prefix };
    }

    if let Some((alias, prefix)) = parse_table_alias_field(text_before_cursor) {
        tracing::info!(
            alias = %alias,
            prefix = %prefix,
            "detected AfterTableAlias (alias.field pattern)"
        );
        return SdblCompletionContext::AfterTableAlias { alias, prefix };
    }

    if is_after_on_keyword(text_before_cursor) {
        let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
        let last_word = words.last().unwrap_or(&"");

        let prefix = if last_word.to_uppercase() == "ПО" || last_word.to_uppercase() == "ON" {
            String::new()
        } else {
            last_word.to_string()
        };

        tracing::info!(prefix = %prefix, "detected AfterOnKeyword");
        return SdblCompletionContext::AfterOnKeyword { prefix };
    }

    if let Some((context, suggestion)) = is_after_as_keyword(text_before_cursor) {
        tracing::info!(
            ?context,
            suggestion = ?suggestion,
            "detected AfterAsKeyword"
        );
        return SdblCompletionContext::AfterAsKeyword { context, suggestion };
    }

    if let Some(prefix) = is_join_type_context(text_before_cursor) {
        tracing::info!(prefix = %prefix, "detected JoinTypeKeyword context");
        return SdblCompletionContext::JoinTypeKeyword { prefix };
    }

    if let Some(path_parts) = parse_dot_path(text_before_cursor) {
        match path_parts.len() {
            2 => {
                if let Ok(mdo_type) = path_parts[0].parse::<bsl_metadata::MdoType>() {
                    let prefix = path_parts[1].clone();
                    tracing::info!(?mdo_type, prefix = %prefix, "detected InsideMdoType (2-part path)");
                    return SdblCompletionContext::InsideMdoType { mdo_type, prefix };
                }
            }
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

    let words: Vec<&str> = text_before_cursor.split_whitespace().collect();
    let prefix = words.last().map(|s| s.to_string()).unwrap_or_default();

    tracing::info!(prefix = %prefix, "detected SdblKeywords context");
    SdblCompletionContext::SdblKeywords { prefix }
}

fn parse_value_function_path(text_before: &str) -> Option<(Vec<String>, String)> {
    let open_paren_pos = text_before.rfind('(')?;

    let before_paren = &text_before[..open_paren_pos].trim_end();
    let before_upper = before_paren.to_uppercase();

    if !before_upper.ends_with("ЗНАЧЕНИЕ") && !before_upper.ends_with("VALUE") {
        return None;
    }

    let inside_paren = &text_before[open_paren_pos + 1..].trim_start();

    if inside_paren.contains(')') {
        return None;
    }

    let segments: Vec<&str> = inside_paren.split('.').collect();

    if segments.is_empty() || (segments.len() == 1 && segments[0].is_empty()) {
        return Some((vec![], String::new()));
    }

    let prefix = segments.last().unwrap().to_string();

    let completed_parts: Vec<String> =
        segments[..segments.len().saturating_sub(1)].iter().map(|s| s.to_string()).collect();

    Some((completed_parts, prefix))
}

fn is_after_from_keyword(text_before: &str) -> bool {
    let text_upper = text_before.to_uppercase();

    text_upper.trim_end().ends_with("FROM") || text_upper.trim_end().ends_with("ИЗ")
}

fn parse_dot_path(text_before: &str) -> Option<Vec<String>> {
    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    if !last_word.contains('.') {
        return None;
    }

    let parts: Vec<String> = last_word.split('.').map(|s| s.to_string()).collect();

    Some(parts)
}

pub(crate) fn is_sdbl_query(text: &str) -> bool {
    let text_upper = text.to_uppercase();

    text_upper.contains("ВЫБРАТЬ")
        || text_upper.contains("ВЫБОР")
        || text_upper.contains("SELECT")
        || text_upper.contains("ИЗ ")
        || text_upper.contains("FROM ")
}

fn is_after_as_keyword(text_before: &str) -> Option<(AsContext, Option<String>)> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    let last_word_upper = words.last()?.to_uppercase();
    if last_word_upper != "КАК" && last_word_upper != "AS" {
        return None;
    }

    let text_upper = text_before.to_uppercase();

    let context = if text_upper.contains("ВЫБРАТЬ") || text_upper.contains("SELECT") {
        let last_from = text_upper.rfind("ИЗ").or(text_upper.rfind("FROM"));
        let last_join = text_upper.rfind("СОЕДИНЕНИЕ").or(text_upper.rfind("JOIN"));
        let last_select = text_upper.rfind("ВЫБРАТЬ").or(text_upper.rfind("SELECT"));

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
    } else {
        AsContext::InFromClause
    };

    let suggestion = match context {
        AsContext::InSelectField => extract_field_name_before_as(text_before),
        AsContext::InFromClause | AsContext::InJoinClause => {
            extract_table_name_before_as(text_before)
        }
    };

    Some((context, suggestion))
}

fn extract_field_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    let expression = words[words.len() - 2];

    if expression.contains('.') {
        let parts: Vec<&str> = expression.split('.').collect();
        return Some(parts.last()?.to_string());
    }

    Some(expression.to_string())
}

fn extract_table_name_before_as(text_before: &str) -> Option<String> {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.len() < 2 {
        return None;
    }

    let reference = words[words.len() - 2];

    if reference.contains('.') {
        let parts: Vec<&str> = reference.split('.').collect();

        return Some(parts.last()?.to_string());
    }

    None
}

fn is_after_on_keyword(text_before: &str) -> bool {
    let words: Vec<&str> = text_before.split_whitespace().collect();

    if words.is_empty() {
        return false;
    }

    let last_word = words.last().unwrap().to_uppercase();
    if last_word == "ПО" || last_word == "ON" {
        return true;
    }

    if words.len() >= 2 {
        let prev_word = words[words.len() - 2].to_uppercase();
        if prev_word == "ПО" || prev_word == "ON" {
            return true;
        }
    }

    false
}

fn is_join_type_context(text_before: &str) -> Option<String> {
    let text_upper = text_before.to_uppercase();

    if !text_upper.contains("КАК") && !text_upper.contains("AS") {
        return None;
    }

    if text_upper.ends_with("СОЕДИНЕНИЕ") || text_upper.ends_with("JOIN") {
        return None;
    }

    let text_trimmed = text_upper.trim_end();
    if text_trimmed.ends_with("ПО") || text_trimmed.ends_with("ON") {
        return None;
    }

    let words: Vec<&str> = text_before.split_whitespace().collect();
    let last_word = words.last()?;

    if last_word.contains('.') {
        return None;
    }

    if !last_word.is_empty() && last_word.len() <= 4 {
        let first_char = last_word.chars().next()?;
        if first_char.is_uppercase() {
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

    #[test]
    fn test_detect_context_incomplete_table_ref() {
        let query = r#"ВЫБРАТЬ
    Валюты.Наименование
ИЗ
    Справочник."#;

        let offset = query.len() as u32;
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

        let query = "ВЫБРАТЬ * ИЗ РегистрСведений. ";
        let offset = TextSize::from((query.len() - 1) as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::InsideMdoType { mdo_type, prefix } => {
                assert_eq!(mdo_type, MdoType::InformationRegister);
                assert_eq!(prefix, "");
            }
            _ => panic!("Expected InsideMdoType, got {:?}", context),
        }
    }

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
        let parts = parse_dot_path("SELECT * FROM Справочник.Вал");
        assert_eq!(parts, Some(vec!["Справочник".to_string(), "Вал".to_string()]));

        let parts = parse_dot_path("SELECT * FROM Справочник.Номенклатура.Шт");
        assert_eq!(
            parts,
            Some(vec!["Справочник".to_string(), "Номенклатура".to_string(), "Шт".to_string()])
        );

        let parts = parse_dot_path("SELECT * FROM Catalog.");
        assert_eq!(parts, Some(vec!["Catalog".to_string(), "".to_string()]));

        let parts = parse_dot_path("SELECT * FROM NoDots");
        assert_eq!(parts, None);
    }

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
        let query = "ИЗ Справочник.Валюты Л";
        let offset = TextSize::from(query.len() as u32);
        let context = detect_context(query, offset);

        match context {
            SdblCompletionContext::SdblKeywords { prefix } => {
                assert_eq!(prefix, "Л");
            }
            _ => panic!("Expected SdblKeywords, got {:?}", context),
        }
    }

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
        let text = "ВЫБРАТЬ Т.Код";
        let result = parse_nested_field_chain(text);

        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_nested_field_chain_mdo_type() {
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

    #[test]
    fn test_parse_nested_column_ref_3_parts() {
        let result = parse_nested_column_ref("T.Ссылка.Организация");
        assert!(result.is_some());
        let (alias, fields) = result.unwrap();
        assert_eq!(alias, "T");
        assert_eq!(fields, vec!["Ссылка", "Организация"]);
    }

    #[test]
    fn test_parse_nested_column_ref_4_parts() {
        let result = parse_nested_column_ref("ЗаказКлиентаТовары.Ссылка.Контрагент.Наименование");
        assert!(result.is_some());
        let (alias, fields) = result.unwrap();
        assert_eq!(alias, "ЗаказКлиентаТовары");
        assert_eq!(fields, vec!["Ссылка", "Контрагент", "Наименование"]);
    }

    #[test]
    fn test_parse_nested_column_ref_2_parts_returns_none() {
        let result = parse_nested_column_ref("T.Ссылка");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_nested_column_ref_mdo_type_returns_none() {
        let result = parse_nested_column_ref("Справочник.Валюты.Код");
        assert!(result.is_none());

        let result = parse_nested_column_ref("Документ.Заказ.Товары");
        assert!(result.is_none());

        let result = parse_nested_column_ref("РегистрСведений.Курсы.Валюта");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_nested_column_ref_lowercase_first_part_returns_none() {
        let result = parse_nested_column_ref("table.Field1.Field2");
        assert!(result.is_none());
    }
}
