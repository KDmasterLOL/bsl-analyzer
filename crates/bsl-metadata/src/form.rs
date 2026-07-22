use stdx::case::CaseExt;
use uuid::Uuid;

use crate::enums::FormType;
use crate::metadata_object::AttributeType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FormElementKind {
    Table = 0,
    Group = 1,
    Field = 2,
    Button = 3,
    Decoration = 4,
    Addition = 5,
    Other = 6,
    UsualGroup = 7,
    Pages = 8,
    Page = 9,
    CommandBar = 10,
    ButtonGroup = 11,
}

impl FormElementKind {
    pub fn base_platform_type_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Table => "ТаблицаФормы",
            Self::Group
            | Self::UsualGroup
            | Self::Pages
            | Self::Page
            | Self::CommandBar
            | Self::ButtonGroup => "ГруппаФормы",
            Self::Field => "ПолеФормы",
            Self::Button => "КнопкаФормы",
            Self::Decoration => "ДекорацияФормы",
            Self::Addition => "ДополнениеЭлементаФормы",
            Self::Other => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormElement {
    pub name: String,
    pub id: u32,
    pub data_path: Option<String>,
    pub kind: FormElementKind,
    pub parent_id: Option<u32>,
}

impl FormElement {
    /// Heap bytes owned by this element: its name/data-path strings. `id`,
    /// `kind`, `parent_id` are `Copy` and own no heap.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity() + self.data_path.as_ref().map_or(0, String::capacity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEventHandler {
    pub event_type: String,
    pub handler_name: String,
}

impl FormEventHandler {
    /// Heap bytes owned by this handler: its event-type/handler-name strings.
    pub fn estimated_heap_size(&self) -> usize {
        self.event_type.capacity() + self.handler_name.capacity()
    }
}

impl FormElement {
    pub fn new(name: impl Into<String>, id: u32, data_path: Option<String>) -> Self {
        Self { name: name.into(), id, data_path, kind: FormElementKind::Other, parent_id: None }
    }

    pub fn with_kind(
        name: impl Into<String>,
        id: u32,
        data_path: Option<String>,
        kind: FormElementKind,
        parent_id: Option<u32>,
    ) -> Self {
        Self { name: name.into(), id, data_path, kind, parent_id }
    }

    pub fn has_wrong_data_path(&self) -> bool {
        self.data_path.as_ref().is_some_and(|dp| dp.starts_with('~'))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormAttributeColumn {
    pub name: String,
    pub attr_type: AttributeType,
}

impl FormAttributeColumn {
    /// Heap bytes owned by this column: its name plus its type's own payload
    /// (only the name-carrying/composite `AttributeType` variants own heap).
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity() + self.attr_type.estimated_heap_size()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormAttribute {
    pub name: String,
    pub attr_type: AttributeType,
    pub is_main: bool,
    pub columns: Vec<FormAttributeColumn>,
}

impl FormAttribute {
    pub fn new(name: impl Into<String>, attr_type: AttributeType) -> Self {
        Self { name: name.into(), attr_type, is_main: false, columns: Vec::new() }
    }

    /// Heap bytes owned by this attribute: its name, its type's own payload,
    /// and the tabular-section column vec. `is_main` is `Copy` and owns no
    /// heap.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.attr_type.estimated_heap_size()
            + stdx::heap::vec_bytes::<FormAttributeColumn>(self.columns.len())
            + self.columns.iter().map(FormAttributeColumn::estimated_heap_size).sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub name: String,
    pub form_type: FormType,
    pub uuid: Uuid,
    pub elements: Vec<FormElement>,
    pub event_handlers: Vec<FormEventHandler>,
    pub command_handlers: Vec<String>,
    pub attributes: Vec<FormAttribute>,
}

impl Form {
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

    pub fn elements(&self) -> &[FormElement] {
        &self.elements
    }

    pub fn elements_with_wrong_data_path(&self) -> impl Iterator<Item = &FormElement> {
        self.elements.iter().filter(|e| e.has_wrong_data_path())
    }

    pub fn find_element(&self, name: &str) -> Option<&FormElement> {
        let name_lower = name.fold_lower();
        self.elements.iter().find(|e| e.name.fold_lower() == name_lower)
    }

    pub fn children_of(&self, parent_id: u32) -> impl Iterator<Item = &FormElement> {
        self.elements.iter().filter(move |e| e.parent_id == Some(parent_id))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn form_type(&self) -> FormType {
        self.form_type
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn attributes(&self) -> &[FormAttribute] {
        &self.attributes
    }

    pub fn attribute_names(&self) -> impl Iterator<Item = &str> {
        self.attributes.iter().map(|a| a.name.as_str())
    }

    pub fn find_attribute(&self, name: &str) -> Option<&FormAttribute> {
        self.attributes.iter().find(|a| stdx::case::eq_ignore_case(&a.name, name))
    }

    pub fn main_attribute(&self) -> Option<&FormAttribute> {
        self.attributes.iter().find(|a| a.is_main)
    }

    pub fn is_managed(&self) -> bool {
        self.form_type == FormType::Managed
    }

    pub fn is_ordinary(&self) -> bool {
        self.form_type == FormType::Ordinary
    }

    pub fn event_handlers(&self) -> &[FormEventHandler] {
        &self.event_handlers
    }

    pub fn event_handler_names(&self) -> Vec<&str> {
        self.event_handlers.iter().map(|h| h.handler_name.as_str()).collect()
    }

    pub fn command_handlers(&self) -> &[String] {
        &self.command_handlers
    }

    /// Heap bytes owned by this form, memoised by `ide-db`'s
    /// `module_metadata_query` for Salsa's `heap_size` hook: its name plus the
    /// element/handler/command-handler/attribute vecs and each entry's own
    /// owned payload. `form_type`/`uuid` are `Copy` and own no heap. New
    /// heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + stdx::heap::vec_bytes::<FormElement>(self.elements.len())
            + self.elements.iter().map(FormElement::estimated_heap_size).sum::<usize>()
            + stdx::heap::vec_bytes::<FormEventHandler>(self.event_handlers.len())
            + self.event_handlers.iter().map(FormEventHandler::estimated_heap_size).sum::<usize>()
            + stdx::heap::vec_bytes::<String>(self.command_handlers.len())
            + self.command_handlers.iter().map(String::capacity).sum::<usize>()
            + stdx::heap::vec_bytes::<FormAttribute>(self.attributes.len())
            + self.attributes.iter().map(FormAttribute::estimated_heap_size).sum::<usize>()
    }

    pub fn is_handler(&self, method_name: &str) -> bool {
        let name_lower = method_name.fold_lower();
        self.event_handlers.iter().any(|h| h.handler_name.fold_lower() == name_lower)
            || self.command_handlers.iter().any(|h| h.fold_lower() == name_lower)
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
        assert_eq!(FormElementKind::Table as u8, 0);
        assert_eq!(FormElementKind::Group as u8, 1);
        assert_eq!(FormElementKind::Field as u8, 2);
        assert_eq!(FormElementKind::Button as u8, 3);
        assert_eq!(FormElementKind::Decoration as u8, 4);
        assert_eq!(FormElementKind::Addition as u8, 5);
        assert_eq!(FormElementKind::Other as u8, 6);
        assert_eq!(FormElementKind::UsualGroup as u8, 7);
        assert_eq!(FormElementKind::Pages as u8, 8);
        assert_eq!(FormElementKind::Page as u8, 9);
        assert_eq!(FormElementKind::CommandBar as u8, 10);
        assert_eq!(FormElementKind::ButtonGroup as u8, 11);

        assert!(FormElementKind::Table < FormElementKind::Group);
        assert!(FormElementKind::Group < FormElementKind::Field);
        assert!(FormElementKind::Field < FormElementKind::Button);
        assert!(FormElementKind::Button < FormElementKind::Decoration);
        assert!(FormElementKind::Decoration < FormElementKind::Addition);
        assert!(FormElementKind::Addition < FormElementKind::Other);
        assert!(FormElementKind::Other < FormElementKind::UsualGroup);
        assert!(FormElementKind::UsualGroup < FormElementKind::Pages);
        assert!(FormElementKind::Pages < FormElementKind::Page);
        assert!(FormElementKind::Page < FormElementKind::CommandBar);
        assert!(FormElementKind::CommandBar < FormElementKind::ButtonGroup);
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

        assert!(form.children_of(200).next().is_none());
    }
}
