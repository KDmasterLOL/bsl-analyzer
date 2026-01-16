//! Use case: Complete JOIN type keywords.

use crate::completion::sdbl::domain::SdblCompletionItem;
use crate::completion::CompletionItemKind;

/// Use case for completing JOIN type keywords.
///
/// Returns JOIN keywords (INNER, LEFT, RIGHT, FULL) in both Russian and English.
pub struct CompleteJoinTypesUseCase;

impl CompleteJoinTypesUseCase {
    /// Execute the use case: get JOIN type keywords.
    ///
    /// # Arguments
    /// - `prefix`: Filter prefix (case-insensitive)
    ///
    /// # Returns
    /// List of matching JOIN keywords
    pub fn execute(prefix: &str) -> Vec<SdblCompletionItem> {
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
            .map(|(keyword, desc)| {
                SdblCompletionItem::new(keyword, CompletionItemKind::Keyword)
                    .with_detail(desc.to_string())
                    .with_documentation(desc.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_join_types_no_prefix() {
        let items = CompleteJoinTypesUseCase::execute("");

        // Should return all keywords (16 total)
        assert_eq!(items.len(), 16);
    }

    #[test]
    fn test_complete_join_types_russian_prefix() {
        let items = CompleteJoinTypesUseCase::execute("ЛЕВ");

        // Should match ЛЕВОЕ and ЛЕВОЕ СОЕДИНЕНИЕ
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ"));
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"));
    }

    #[test]
    fn test_complete_join_types_english_prefix() {
        let items = CompleteJoinTypesUseCase::execute("LEF");

        // Should match LEFT and LEFT JOIN
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "LEFT"));
        assert!(items.iter().any(|i| i.label == "LEFT JOIN"));
    }

    #[test]
    fn test_complete_join_types_case_insensitive() {
        let items_upper = CompleteJoinTypesUseCase::execute("INNER");
        let items_lower = CompleteJoinTypesUseCase::execute("inner");

        // Should return same results
        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper.len(), 2); // INNER and INNER JOIN
    }

    #[test]
    fn test_complete_join_types_full_form() {
        let items = CompleteJoinTypesUseCase::execute("ВНУТРЕННЕЕ СО");

        // Should match only full form
        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ВНУТРЕННЕЕ СОЕДИНЕНИЕ"));
    }

    #[test]
    fn test_complete_join_types_no_match() {
        let items = CompleteJoinTypesUseCase::execute("XXXXX");

        // Should return empty for no matches
        assert_eq!(items.len(), 0);
    }
}
