use crate::completion::sdbl::domain::SdblCompletionItem;
use crate::completion::CompletionItemKind;
use stdx::case::CaseExt;

pub struct CompleteKeywordsUseCase;

impl CompleteKeywordsUseCase {
    pub fn execute(prefix: &str) -> Vec<SdblCompletionItem> {
        let keywords = Self::sdbl_keywords();
        let prefix_lower = prefix.fold_lower();

        let mut items = Vec::new();

        for (russian, english, description) in &keywords {
            if russian.fold_lower().starts_with(&prefix_lower) || prefix.is_empty() {
                items.push(
                    SdblCompletionItem::new(*russian, CompletionItemKind::Keyword)
                        .with_detail(format!("Ключевое слово SDBL ({})", english))
                        .with_documentation(*description),
                );
            }

            if english.fold_lower().starts_with(&prefix_lower) || prefix.is_empty() {
                items.push(
                    SdblCompletionItem::new(*english, CompletionItemKind::Keyword)
                        .with_detail(format!("SDBL keyword ({})", russian))
                        .with_documentation(*description),
                );
            }
        }

        items
    }

    fn sdbl_keywords() -> Vec<(&'static str, &'static str, &'static str)> {
        vec![
            ("ВЫБРАТЬ", "SELECT", "Выбрать данные из таблицы"),
            ("ИЗ", "FROM", "Указать источник данных"),
            ("ГДЕ", "WHERE", "Условие фильтрации"),
            ("СГРУППИРОВАТЬ", "GROUP", "Группировка данных"),
            ("УПОРЯДОЧИТЬ", "ORDER", "Сортировка результатов"),
            ("ПО", "BY", "Указать поля для группировки/сортировки"),
            ("СОЕДИНЕНИЕ", "JOIN", "Соединение таблиц"),
            ("ЛЕВОЕ", "LEFT", "Левое внешнее соединение"),
            ("ПРАВОЕ", "RIGHT", "Правое внешнее соединение"),
            ("ПОЛНОЕ", "FULL", "Полное внешнее соединение"),
            ("ВНУТРЕННЕЕ", "INNER", "Внутреннее соединение"),
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
            ("ПОМЕСТИТЬ", "INTO", "Поместить результат во временную таблицу"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_keywords_no_prefix() {
        let items = CompleteKeywordsUseCase::execute("");
        assert!(!items.is_empty());
        assert!(items.iter().any(|i| i.label == "ВЫБРАТЬ"));
        assert!(items.iter().any(|i| i.label == "SELECT"));
    }

    #[test]
    fn test_complete_keywords_russian_prefix() {
        let items = CompleteKeywordsUseCase::execute("ВЫ");
        assert!(items.iter().any(|i| i.label == "ВЫБРАТЬ"));
        assert!(items.iter().any(|i| i.label == "ВЫРАЗИТЬ"));
        assert!(!items.iter().any(|i| i.label == "ИЗ"));
    }

    #[test]
    fn test_complete_keywords_english_prefix() {
        let items = CompleteKeywordsUseCase::execute("SEL");
        assert!(items.iter().any(|i| i.label == "SELECT"));
        assert!(!items.iter().any(|i| i.label == "FROM"));
    }

    #[test]
    fn test_complete_keywords_case_insensitive() {
        let items_upper = CompleteKeywordsUseCase::execute("SEL");
        let items_lower = CompleteKeywordsUseCase::execute("sel");
        assert_eq!(items_upper.len(), items_lower.len());
    }

    #[test]
    fn test_keyword_has_detail_and_documentation() {
        let items = CompleteKeywordsUseCase::execute("SELECT");
        let select_item = items.iter().find(|i| i.label == "SELECT").unwrap();

        assert!(select_item.detail.is_some());
        assert!(select_item.documentation.is_some());
        assert_eq!(select_item.kind, CompletionItemKind::Keyword);
    }
}
