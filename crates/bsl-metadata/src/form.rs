//! Form metadata structure.
//!
//! Represents 1C:Enterprise form metadata with type information.

use uuid::Uuid;

use crate::enums::FormType;
use crate::metadata_object::AttributeType;

/// Coarse taxonomy of form elements (XML tag → kind).
///
/// Mirrors the «Элементы коллекции» section of the BSL platform docs:
/// every concrete UI control belongs to exactly one of these buckets.
/// Used by [`FormElement::kind`] to drive type resolution of
/// `Элементы.<имя>`: a `<Table>` lowers to `ТаблицаФормы`, a `<Button>`
/// to `КнопкаФормы`, a `<UsualGroup>` to `ГруппаФормы`, etc.
///
/// `Other` is a fail-safe for tags the parser does not recognise; a
/// future tag must be classified explicitly rather than inheriting
/// silent platform-control semantics.
///
/// **Ordering:** discriminants are pinned (`Table = 0` … `Other = 6`) so
/// that derived `Ord` is stable across reshuffles of declaration order —
/// `Ty` derives `Ord` and a later phase wraps `FormElementKind` inside
/// `Ty::FormControl`, where ordering changes would invalidate the Salsa
/// cache silently. Reordering variants is now safe; renumbering is the
/// breaking operation, and the canonical-ordering test below pins it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FormElementKind {
    /// `<Table>` — UI table control bound to a tabular section.
    Table = 0,
    /// `<UsualGroup>`, `<Pages>`, `<Page>`, `<CommandBar>`, `<ButtonGroup>`.
    Group = 1,
    /// `<InputField>`, `<LabelField>`, `<CheckBoxField>`, `<RadioButtonField>`,
    /// `<HTMLField>`, `<PictureField>`, `<SpreadsheetDocumentField>`, etc.
    Field = 2,
    /// `<Button>`.
    Button = 3,
    /// `<Decoration>`.
    Decoration = 4,
    /// `<ContextMenu>`, `<ExtendedTooltip>`, `<SearchStringAddition>`,
    /// `<ViewStatusAddition>`, `<SearchControlAddition>`, `<AutoCommandBar>`.
    Addition = 5,
    /// Unknown XML tag — keeps form parsing fail-safe; element is still
    /// surfaced (its name participates in `Элементы.<имя>` lookup) but
    /// type resolution falls through to `Unknown`.
    Other = 6,
}

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
    /// Coarse taxonomy of the underlying XML tag (Table/Group/Field/...).
    pub kind: FormElementKind,
    /// ID of the immediate parent element when the control sits inside
    /// `<ChildItems>` of another container (group, table, page).
    /// `None` for top-level elements.
    pub parent_id: Option<u32>,
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
    /// Construct an element with a sensible default kind ([`FormElementKind::Other`])
    /// and no parent. Reserved for tests and legacy call-sites that have
    /// no XML tag context. **Production code paths from the XML parser
    /// must use [`FormElement::with_kind`]** — passing through `new`
    /// silently degrades type resolution to fall-through (`Other` → no
    /// `Ty::FormControl` mapping).
    pub fn new(name: impl Into<String>, id: u32, data_path: Option<String>) -> Self {
        Self { name: name.into(), id, data_path, kind: FormElementKind::Other, parent_id: None }
    }

    /// Construct an element with the kind decoded from the XML tag and
    /// the parent's element id. Used by `xml_parser::form::collect_child_items`.
    pub fn with_kind(
        name: impl Into<String>,
        id: u32,
        data_path: Option<String>,
        kind: FormElementKind,
        parent_id: Option<u32>,
    ) -> Self {
        Self { name: name.into(), id, data_path, kind, parent_id }
    }

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

    /// Find an element by name (case-insensitive — BSL identifiers are
    /// case-insensitive, so `Элементы.переприемка` and `Элементы.Переприемка`
    /// must resolve to the same control).
    pub fn find_element(&self, name: &str) -> Option<&FormElement> {
        let name_lower = name.to_lowercase();
        self.elements.iter().find(|e| e.name.to_lowercase() == name_lower)
    }

    /// Iterate over the immediate children of `parent_id` in declaration order.
    ///
    /// Used to walk a `<Table>`'s columns or a `<UsualGroup>`'s controls
    /// without re-parsing XML — the hierarchy is captured during parsing
    /// and stored as parent links on each [`FormElement`].
    pub fn children_of(&self, parent_id: u32) -> impl Iterator<Item = &FormElement> {
        self.elements.iter().filter(move |e| e.parent_id == Some(parent_id))
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
        let wrong = FormElement::new(
            "НесуществующийРеквизит",
            1,
            Some("~Объект.НесуществующийРеквизит".to_string()),
        );
        assert!(wrong.has_wrong_data_path());

        let ok = FormElement::new("Код", 2, Some("Объект.Code".to_string()));
        assert!(!ok.has_wrong_data_path());

        let no_path = FormElement::new("Кнопка", 3, None);
        assert!(!no_path.has_wrong_data_path());
    }

    #[test]
    fn test_form_element_new_defaults_to_other_kind_and_no_parent() {
        let elem = FormElement::new("Код", 1, Some("Объект.Code".to_string()));
        assert_eq!(elem.kind, FormElementKind::Other);
        assert_eq!(elem.parent_id, None);
    }

    #[test]
    fn test_form_element_kind_discriminants_are_pinned() {
        // Discriminants are part of the public ordering contract: `Ty` derives
        // `Ord` and Phase 3 wraps `FormElementKind` inside `Ty::FormControl`.
        // If a future variant renumbers these values, Salsa caches keyed on
        // `Ty` would silently invalidate. This test fails the build instead.
        assert_eq!(FormElementKind::Table as u8, 0);
        assert_eq!(FormElementKind::Group as u8, 1);
        assert_eq!(FormElementKind::Field as u8, 2);
        assert_eq!(FormElementKind::Button as u8, 3);
        assert_eq!(FormElementKind::Decoration as u8, 4);
        assert_eq!(FormElementKind::Addition as u8, 5);
        assert_eq!(FormElementKind::Other as u8, 6);

        // Derived `Ord` follows the discriminants.
        assert!(FormElementKind::Table < FormElementKind::Group);
        assert!(FormElementKind::Group < FormElementKind::Field);
        assert!(FormElementKind::Field < FormElementKind::Button);
        assert!(FormElementKind::Button < FormElementKind::Decoration);
        assert!(FormElementKind::Decoration < FormElementKind::Addition);
        assert!(FormElementKind::Addition < FormElementKind::Other);
    }

    #[test]
    fn test_form_with_elements() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let elements = vec![
            FormElement::new("Код", 1, Some("Объект.Code".to_string())),
            FormElement::new(
                "НесуществующийРеквизит",
                2,
                Some("~Объект.НесуществующийРеквизит".to_string()),
            ),
        ];

        let form =
            Form::with_elements("ФормаЭлемента".to_string(), FormType::Managed, uuid, elements);

        assert_eq!(form.elements().len(), 2);
        let wrong: Vec<_> = form.elements_with_wrong_data_path().collect();
        assert_eq!(wrong.len(), 1);
        assert_eq!(wrong[0].name, "НесуществующийРеквизит");
    }

    #[test]
    fn test_find_element_case_insensitive() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let elements = vec![
            FormElement {
                name: "Переприемка".to_string(),
                id: 255,
                data_path: Some("Объект.Переприемка".to_string()),
                kind: FormElementKind::Table,
                parent_id: None,
            },
            FormElement {
                name: "ШтрихКод".to_string(),
                id: 256,
                data_path: Some("Объект.Переприемка.ШтрихКод".to_string()),
                kind: FormElementKind::Field,
                parent_id: Some(255),
            },
        ];
        let form = Form::with_elements("Ф".to_string(), FormType::Managed, uuid, elements);

        // BSL identifiers are case-insensitive.
        assert_eq!(form.find_element("Переприемка").map(|e| e.id), Some(255));
        assert_eq!(form.find_element("переприемка").map(|e| e.id), Some(255));
        assert_eq!(form.find_element("ПЕРЕПРИЕМКА").map(|e| e.id), Some(255));
        assert!(form.find_element("Несуществующий").is_none());
    }

    #[test]
    fn test_children_of_returns_immediate_children() {
        let uuid = Uuid::parse_str("12345678-1234-1234-1234-123456789012").unwrap();
        let elements = vec![
            FormElement {
                name: "ГруппаОбщая".to_string(),
                id: 100,
                data_path: None,
                kind: FormElementKind::Group,
                parent_id: None,
            },
            FormElement {
                name: "ПолеВГруппе".to_string(),
                id: 101,
                data_path: Some("Объект.Code".to_string()),
                kind: FormElementKind::Field,
                parent_id: Some(100),
            },
            FormElement {
                name: "КнопкаТопЛевел".to_string(),
                id: 200,
                data_path: None,
                kind: FormElementKind::Button,
                parent_id: None,
            },
        ];
        let form = Form::with_elements("Ф".to_string(), FormType::Managed, uuid, elements);

        let group_kids: Vec<_> = form.children_of(100).map(|e| e.id).collect();
        assert_eq!(group_kids, vec![101]);

        // Top-level elements are not children of any container.
        assert!(form.children_of(200).next().is_none());
    }
}
