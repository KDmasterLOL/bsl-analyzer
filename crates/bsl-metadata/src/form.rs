//! Form metadata structure.
//!
//! Represents 1C:Enterprise form metadata with type information.

use uuid::Uuid;

use crate::enums::FormType;
use crate::metadata_object::AttributeType;

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

/// Form event handler metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEventHandler {
    /// Platform event type from the XML `name` attribute.
    pub event_type: String,
    /// Method name called for the event.
    pub handler_name: String,
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

/// Form attribute column (for ValueTable / FormDataCollection-typed
/// attributes that declare a `<Columns>` schema in `Form.xml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormAttributeColumn {
    /// Column name as written in `<Column name="…">`.
    pub name: String,
    /// Column type lowered from the nested `<Type>` element.
    pub attr_type: AttributeType,
}

/// Form attribute declared in `Form.xml` `<Attributes>`.
///
/// Carries the full type information needed for type inference inside
/// the form module (`Объект.Дата`, `Замечание`, `ТаблицаРасходов.Колонка`).
/// `is_main` flags the form's main attribute (`<MainAttribute>true</...>`),
/// which the platform exposes as `Объект` and wraps in
/// `ДанныеФормыСтруктура` so `Объект.Записать()` is **not** the same as
/// `<Object>Object.Записать()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormAttribute {
    /// Attribute name as written in `<Attribute name="…">`.
    pub name: String,
    /// Type lowered from the `<Type>` element. `Unknown` if the type
    /// element is absent or unrecognised.
    pub attr_type: AttributeType,
    /// `true` when `<MainAttribute>true</...>` is set — the platform exposes
    /// this attribute as `Объект` inside the form module.
    pub is_main: bool,
    /// `<Columns>` schema for `ValueTable` / `FormDataCollection`-typed
    /// attributes; empty otherwise.
    pub columns: Vec<FormAttributeColumn>,
}

impl FormAttribute {
    /// Create a plain attribute (no MainAttribute flag, no columns).
    pub fn new(name: impl Into<String>, attr_type: AttributeType) -> Self {
        Self { name: name.into(), attr_type, is_main: false, columns: Vec::new() }
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
    /// Event handlers (from `<Events><Event>handler</Event></Events>`)
    ///
    /// These are methods called by the platform for form events like
    /// OnCreateAtServer, OnOpen, BeforeClose, etc.
    pub event_handlers: Vec<FormEventHandler>,
    /// Command handler method names (from `<Commands><Command><Action>handler</Action></Command></Commands>`)
    ///
    /// These are methods called when form commands are executed.
    pub command_handlers: Vec<String>,
    /// Form attributes with full type information (from `<Attributes><Attribute>`).
    ///
    /// In FormModule, assignments like `Замечание = Параметры.Замечание` write to
    /// form attributes, not local variables. The attribute names must be excluded
    /// from UnusedLocalVariable diagnostic (see [`Self::attribute_names`]); the
    /// types feed type inference inside the form module.
    pub attributes: Vec<FormAttribute>,
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
        event_handlers: Vec<FormEventHandler>,
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

    /// Get form attributes (typed).
    pub fn attributes(&self) -> &[FormAttribute] {
        &self.attributes
    }

    /// Iterate over form attribute names.
    ///
    /// Convenience for consumers (UnusedLocalVariable, formatting) that
    /// only need the names of реквизиты формы — they declared their own
    /// projection before [`FormAttribute`] carried type information.
    pub fn attribute_names(&self) -> impl Iterator<Item = &str> {
        self.attributes.iter().map(|a| a.name.as_str())
    }

    /// Find an attribute by name (case-insensitive — BSL identifiers are
    /// case-insensitive).
    pub fn find_attribute(&self, name: &str) -> Option<&FormAttribute> {
        let name_lower = name.to_lowercase();
        self.attributes.iter().find(|a| a.name.to_lowercase() == name_lower)
    }

    /// The form's main attribute (the one flagged `<MainAttribute>true</...>`),
    /// exposed as `Объект` inside the form module.
    pub fn main_attribute(&self) -> Option<&FormAttribute> {
        self.attributes.iter().find(|a| a.is_main)
    }

    /// Check if this is a managed form.
    pub fn is_managed(&self) -> bool {
        self.form_type == FormType::Managed
    }

    /// Check if this is an ordinary form.
    pub fn is_ordinary(&self) -> bool {
        self.form_type == FormType::Ordinary
    }

    /// Get event handlers.
    ///
    /// These methods are called by the platform for form events.
    pub fn event_handlers(&self) -> &[FormEventHandler] {
        &self.event_handlers
    }

    /// Get event handler method names.
    pub fn event_handler_names(&self) -> Vec<&str> {
        self.event_handlers.iter().map(|h| h.handler_name.as_str()).collect()
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
        self.event_handlers.iter().any(|h| h.handler_name.to_lowercase() == name_lower)
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
