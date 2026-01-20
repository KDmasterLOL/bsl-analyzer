//! Form metadata structure.
//!
//! Represents 1C:Enterprise form metadata with type information.

use uuid::Uuid;

use crate::enums::FormType;

/// Form metadata (minimal structure for diagnostics).
///
/// Contains form name, type (Managed/Ordinary), and UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// Form name
    pub name: String,
    /// Form type (Managed or Ordinary)
    pub form_type: FormType,
    /// Form UUID
    pub uuid: Uuid,
}

impl Form {
    /// Create a new Form instance.
    pub fn new(name: String, form_type: FormType, uuid: Uuid) -> Self {
        Self { name, form_type, uuid }
    }

    /// Get form name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get form type.
    pub fn form_type(&self) -> FormType {
        self.form_type
    }

    /// Get form UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Check if this is a managed form.
    pub fn is_managed(&self) -> bool {
        self.form_type == FormType::Managed
    }

    /// Check if this is an ordinary form.
    pub fn is_ordinary(&self) -> bool {
        self.form_type == FormType::Ordinary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_creation() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let form = Form::new("ФормаЭлемента".to_string(), FormType::Managed, uuid);

        assert_eq!(form.name(), "ФормаЭлемента");
        assert_eq!(form.form_type(), FormType::Managed);
        assert!(form.is_managed());
        assert!(!form.is_ordinary());
    }

    #[test]
    fn test_ordinary_form() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let form = Form::new("ОбычнаяФорма".to_string(), FormType::Ordinary, uuid);

        assert!(form.is_ordinary());
        assert!(!form.is_managed());
    }
}
