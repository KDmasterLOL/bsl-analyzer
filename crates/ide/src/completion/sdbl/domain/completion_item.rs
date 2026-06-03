use crate::completion::{CompletionItem, CompletionItemKind};

#[derive(Debug, Clone, PartialEq)]
pub struct SdblCompletionItem {
    pub label: String,

    pub kind: CompletionItemKind,

    pub detail: Option<String>,

    pub documentation: Option<String>,

    pub sort_text: Option<String>,

    pub filter_text: Option<String>,
}

impl SdblCompletionItem {
    pub fn new(label: impl Into<String>, kind: CompletionItemKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            sort_text: None,
            filter_text: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_documentation(mut self, documentation: impl Into<String>) -> Self {
        self.documentation = Some(documentation.into());
        self
    }

    #[allow(dead_code, reason = "used by completion tests")]
    pub fn with_sort_text(mut self, sort_text: impl Into<String>) -> Self {
        self.sort_text = Some(sort_text.into());
        self
    }

    #[allow(dead_code, reason = "used by completion tests")]
    pub fn matches_prefix(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return true;
        }
        let prefix_lower = prefix.to_lowercase();
        self.label.to_lowercase().starts_with(&prefix_lower)
    }

    pub fn into_completion_item(self) -> CompletionItem {
        CompletionItem {
            label: self.label.clone(),
            kind: self.kind,
            detail: self.detail,
            documentation: self.documentation,
            insert_text: self.label.clone(),
            sort_text: self.sort_text,
            filter_text: self.filter_text,
            source: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_prefix() {
        let item = SdblCompletionItem::new("Ссылка", CompletionItemKind::Field);

        assert!(item.matches_prefix(""));
        assert!(item.matches_prefix("С"));
        assert!(item.matches_prefix("Ссыл"));
        assert!(item.matches_prefix("ссылка"));
        assert!(!item.matches_prefix("Код"));
    }

    #[test]
    fn test_builder_pattern() {
        let item = SdblCompletionItem::new("TestField", CompletionItemKind::Field)
            .with_detail("String")
            .with_documentation("Test field documentation")
            .with_sort_text("01_TestField");

        assert_eq!(item.label, "TestField");
        assert_eq!(item.detail, Some("String".to_string()));
        assert_eq!(item.documentation, Some("Test field documentation".to_string()));
        assert_eq!(item.sort_text, Some("01_TestField".to_string()));
    }
}
