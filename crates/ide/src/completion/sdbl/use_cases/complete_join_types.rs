use crate::completion::sdbl::domain::SdblCompletionItem;
use crate::completion::CompletionItemKind;
use stdx::case::CaseExt;

pub struct CompleteJoinTypesUseCase;

impl CompleteJoinTypesUseCase {
    pub fn execute(prefix: &str) -> Vec<SdblCompletionItem> {
        let prefix_lower = prefix.fold_lower();

        let join_keywords = vec![
            ("ЛЕВОЕ СОЕДИНЕНИЕ", "Левое внешнее соединение (LEFT JOIN)"),
            ("ПРАВОЕ СОЕДИНЕНИЕ", "Правое внешнее соединение (RIGHT JOIN)"),
            ("ВНУТРЕННЕЕ СОЕДИНЕНИЕ", "Внутреннее соединение (INNER JOIN)"),
            ("ПОЛНОЕ СОЕДИНЕНИЕ", "Полное внешнее соединение (FULL JOIN)"),
            ("ЛЕВОЕ", "Левое внешнее соединение"),
            ("ПРАВОЕ", "Правое внешнее соединение"),
            ("ВНУТРЕННЕЕ", "Внутреннее соединение"),
            ("ПОЛНОЕ", "Полное внешнее соединение"),
            ("LEFT JOIN", "Left outer join"),
            ("RIGHT JOIN", "Right outer join"),
            ("INNER JOIN", "Inner join"),
            ("FULL JOIN", "Full outer join"),
            ("LEFT", "Left outer join"),
            ("RIGHT", "Right outer join"),
            ("INNER", "Inner join"),
            ("FULL", "Full outer join"),
        ];

        join_keywords
            .into_iter()
            .filter(|(keyword, _)| keyword.fold_lower().starts_with(&prefix_lower))
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

        assert_eq!(items.len(), 16);
    }

    #[test]
    fn test_complete_join_types_russian_prefix() {
        let items = CompleteJoinTypesUseCase::execute("ЛЕВ");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ"));
        assert!(items.iter().any(|i| i.label == "ЛЕВОЕ СОЕДИНЕНИЕ"));
    }

    #[test]
    fn test_complete_join_types_english_prefix() {
        let items = CompleteJoinTypesUseCase::execute("LEF");

        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|i| i.label == "LEFT"));
        assert!(items.iter().any(|i| i.label == "LEFT JOIN"));
    }

    #[test]
    fn test_complete_join_types_case_insensitive() {
        let items_upper = CompleteJoinTypesUseCase::execute("INNER");
        let items_lower = CompleteJoinTypesUseCase::execute("inner");

        assert_eq!(items_upper.len(), items_lower.len());
        assert_eq!(items_upper.len(), 2);
    }

    #[test]
    fn test_complete_join_types_full_form() {
        let items = CompleteJoinTypesUseCase::execute("ВНУТРЕННЕЕ СО");

        assert_eq!(items.len(), 1);
        assert!(items.iter().any(|i| i.label == "ВНУТРЕННЕЕ СОЕДИНЕНИЕ"));
    }

    #[test]
    fn test_complete_join_types_no_match() {
        let items = CompleteJoinTypesUseCase::execute("XXXXX");

        assert_eq!(items.len(), 0);
    }
}
