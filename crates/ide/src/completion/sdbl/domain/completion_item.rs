//! Domain model for SDBL completion items.

use crate::completion::{CompletionItem, CompletionItemKind};

/// Domain model for SDBL completion item.
///
/// This is a domain representation that can be converted to IDE's CompletionItem.
#[derive(Debug, Clone, PartialEq)]
pub struct SdblCompletionItem {
    /// Label shown in completion list.
    pub label: String,

    /// Kind of completion item (Field, Type, Keyword, etc.).
    pub kind: CompletionItemKind,

    /// Detail text (type information, etc.).
    pub detail: Option<String>,

    /// Documentation text (long description).
    pub documentation: Option<String>,

    /// Sort text (for ordering in completion list).
    pub sort_text: Option<String>,

    /// Filter text (for filtering when user types).
    pub filter_text: Option<String>,
}

impl SdblCompletionItem {
    /// Create a new completion item.
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

    /// Set detail text.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set documentation text.
    pub fn with_documentation(mut self, documentation: impl Into<String>) -> Self {
        self.documentation = Some(documentation.into());
        self
    }

    /// Set sort text.
    pub fn with_sort_text(mut self, sort_text: impl Into<String>) -> Self {
        self.sort_text = Some(sort_text.into());
        self
    }

    /// Check if item matches prefix (case-insensitive).
    pub fn matches_prefix(&self, prefix: &str) -> bool {
        if prefix.is_empty() {
            return true;
        }
        let prefix_lower = prefix.to_lowercase();
        self.label.to_lowercase().starts_with(&prefix_lower)
    }

    /// Convert to IDE's CompletionItem.
    pub fn into_completion_item(self) -> CompletionItem {
        CompletionItem {
            label: self.label.clone(),
            kind: self.kind,
            detail: self.detail,
            documentation: self.documentation,
            insert_text: self.label.clone(), // Default: insert same as label
            sort_text: self.sort_text,
            filter_text: self.filter_text,
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
