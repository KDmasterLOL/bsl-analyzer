//! Form metadata structure.
//!
//! Represents 1C:Enterprise form metadata with type information.

use uuid::Uuid;

use crate::enums::FormType;

/// Form element with data path information.
///
/// Represents form controls (InputField, LabelField, CheckBoxField, etc.)
/// that may have a DataPath binding to form attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormElement {
    /// Element name
    pub name: String,
    /// Element ID
    pub id: u32,
    /// Data path (may start with `~` for unresolved references)
    pub data_path: Option<String>,
}

impl FormElement {
    /// Check if the element has a wrong (unresolved) data path.
    ///
    /// Returns true if data_path starts with `~`, which indicates
    /// that the form attribute was deleted or renamed.
    pub fn has_wrong_data_path(&self) -> bool {
        self.data_path.as_ref().is_some_and(|dp| dp.starts_with('~'))
    }
}

/// Form metadata (minimal structure for diagnostics).
///
/// Contains form name, type (Managed/Ordinary), UUID, elements, and event handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// Form name
    pub name: String,
    /// Form type (Managed or Ordinary)
    pub form_type: FormType,
    /// Form UUID
    pub uuid: Uuid,
    /// Form elements with data path bindings
    pub elements: Vec<FormElement>,
    /// Event handler method names (from `<Events><Event>handler</Event></Events>`)
    ///
    /// These are methods called by the platform for form events like
    /// OnCreateAtServer, OnOpen, BeforeClose, etc.
    pub event_handlers: Vec<String>,
    /// Command handler method names (from `<Commands><Command><Action>handler</Action></Command></Commands>`)
    ///
    /// These are methods called when form commands are executed.
    pub command_handlers: Vec<String>,
    /// Form attribute names (from `<Attributes><Attribute name="...">`)
    ///
    /// In FormModule, assignments like `Замечание = Параметры.Замечание` write to
    /// form attributes, not local variables. These names must be excluded from
    /// UnusedLocalVariable diagnostic.
    pub attributes: Vec<String>,
}

impl Form {
    /// Create a new Form instance.
    pub fn new(name: String, form_type: FormType, uuid: Uuid) -> Self {
        Self {
            name,
            form_type,
            uuid,
            elements: Vec::new(),
            event_handlers: Vec::new(),
            command_handlers: Vec::new(),
            attributes: Vec::new(),
        }
    }

    /// Create a new Form instance with elements.
    pub fn with_elements(
        name: String,
        form_type: FormType,
        uuid: Uuid,
        elements: Vec<FormElement>,
    ) -> Self {
        Self {
            name,
            form_type,
            uuid,
            elements,
            event_handlers: Vec::new(),
            command_handlers: Vec::new(),
            attributes: Vec::new(),
        }
    }

    /// Create a new Form instance with elements and handlers.
    pub fn with_handlers(
        name: String,
        form_type: FormType,
        uuid: Uuid,
        elements: Vec<FormElement>,
        event_handlers: Vec<String>,
        command_handlers: Vec<String>,
    ) -> Self {
        Self {
            name,
            form_type,
            uuid,
            elements,
            event_handlers,
            command_handlers,
            attributes: Vec::new(),
        }
    }

    /// Get form elements.
    pub fn elements(&self) -> &[FormElement] {
        &self.elements
    }

    /// Get form elements with wrong data path (starting with `~`).
    pub fn elements_with_wrong_data_path(&self) -> impl Iterator<Item = &FormElement> {
        self.elements.iter().filter(|e| e.has_wrong_data_path())
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

    /// Get form attribute names.
    ///
    /// These are form-level attributes (реквизиты формы) defined in `<Attributes>` section.
    pub fn attributes(&self) -> &[String] {
        &self.attributes
    }

    /// Check if this is a managed form.
    pub fn is_managed(&self) -> bool {
        self.form_type == FormType::Managed
    }

    /// Check if this is an ordinary form.
    pub fn is_ordinary(&self) -> bool {
        self.form_type == FormType::Ordinary
    }

    /// Get event handler method names.
    ///
    /// These methods are called by the platform for form events.
    pub fn event_handlers(&self) -> &[String] {
        &self.event_handlers
    }

    /// Get command handler method names.
    ///
    /// These methods are called when form commands are executed.
    pub fn command_handlers(&self) -> &[String] {
        &self.command_handlers
    }

    /// Check if a method name is a form handler (event or command).
    ///
    /// Comparison is case-insensitive.
    pub fn is_handler(&self, method_name: &str) -> bool {
        let name_lower = method_name.to_lowercase();
        self.event_handlers.iter().any(|h| h.to_lowercase() == name_lower)
            || self.command_handlers.iter().any(|h| h.to_lowercase() == name_lower)
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
        assert!(form.elements().is_empty());
    }

    #[test]
    fn test_ordinary_form() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let form = Form::new("ОбычнаяФорма".to_string(), FormType::Ordinary, uuid);

        assert!(form.is_ordinary());
        assert!(!form.is_managed());
    }

    #[test]
    fn test_form_element_has_wrong_data_path() {
        let wrong = FormElement {
            name: "НесуществующийРеквизит".to_string(),
            id: 1,
            data_path: Some("~Объект.НесуществующийРеквизит".to_string()),
        };
        assert!(wrong.has_wrong_data_path());

        let ok = FormElement {
            name: "Код".to_string(),
            id: 2,
            data_path: Some("Объект.Code".to_string()),
        };
        assert!(!ok.has_wrong_data_path());

        let no_path = FormElement { name: "Кнопка".to_string(), id: 3, data_path: None };
        assert!(!no_path.has_wrong_data_path());
    }

    #[test]
    fn test_form_with_elements() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let elements = vec![
            FormElement {
                name: "Код".to_string(),
                id: 1,
                data_path: Some("Объект.Code".to_string()),
            },
            FormElement {
                name: "НесуществующийРеквизит".to_string(),
                id: 2,
                data_path: Some("~Объект.НесуществующийРеквизит".to_string()),
            },
        ];

        let form =
            Form::with_elements("ФормаЭлемента".to_string(), FormType::Managed, uuid, elements);

        assert_eq!(form.elements().len(), 2);
        let wrong: Vec<_> = form.elements_with_wrong_data_path().collect();
        assert_eq!(wrong.len(), 1);
        assert_eq!(wrong[0].name, "НесуществующийРеквизит");
    }
}
