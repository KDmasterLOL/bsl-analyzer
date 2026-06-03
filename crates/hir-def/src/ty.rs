pub mod doc_types;

pub use bsl_metadata::FormElementKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormDataKind {
    Structure,
    Collection,
    StructureWithCollection,
}

impl FormDataKind {
    pub fn platform_type_name(self) -> &'static str {
        match self {
            Self::Structure => "ДанныеФормыСтруктура",
            Self::Collection => "ДанныеФормыКоллекция",
            Self::StructureWithCollection => "ДанныеФормыСтруктураСКоллекцией",
        }
    }
}

pub fn form_control_platform_type_chain(kind: FormElementKind) -> &'static [&'static str] {
    match kind {
        FormElementKind::Table => &["ТаблицаФормы"],
        FormElementKind::Group => &["ГруппаФормы"],
        FormElementKind::UsualGroup => {
            &["ГруппаФормы", "Расширение группы формы для обычной группы"]
        }
        FormElementKind::Pages => &["ГруппаФормы", "Расширение группы формы для страниц"],
        FormElementKind::Page => &["ГруппаФормы", "Расширение группы формы для страницы"],
        FormElementKind::CommandBar => {
            &["ГруппаФормы", "Расширение группы формы для командной панели"]
        }
        FormElementKind::ButtonGroup => {
            &["ГруппаФормы", "Расширение группы формы для группы кнопок"]
        }
        FormElementKind::Field => &["ПолеФормы"],
        FormElementKind::Button => &["КнопкаФормы"],
        FormElementKind::Decoration => &["ДекорацияФормы"],
        FormElementKind::Addition => &["ДополнениеЭлементаФормы"],
        FormElementKind::Other => &[],
    }
}

pub fn form_control_platform_type_name(kind: FormElementKind) -> Option<&'static str> {
    form_control_platform_type_chain(kind).first().copied()
}

pub fn form_control_chain_first_hit<T, F>(kind: FormElementKind, mut lookup: F) -> Option<T>
where
    F: FnMut(&str) -> Option<T>,
{
    for type_name in form_control_platform_type_chain(kind).iter().rev() {
        if let Some(res) = lookup(type_name) {
            return Some(res);
        }
    }
    None
}

pub fn form_element_kind_label(kind: FormElementKind, locale: base_db::Locale) -> &'static str {
    use base_db::Locale;
    match (kind, locale) {
        (FormElementKind::Table, Locale::Ru) => "Таблица",
        (FormElementKind::Table, Locale::En) => "Table",
        (FormElementKind::Group, Locale::Ru) => "Группа",
        (FormElementKind::Group, Locale::En) => "Group",
        (FormElementKind::UsualGroup, Locale::Ru) => "Обычная группа",
        (FormElementKind::UsualGroup, Locale::En) => "Usual group",
        (FormElementKind::Pages, Locale::Ru) => "Страницы",
        (FormElementKind::Pages, Locale::En) => "Pages",
        (FormElementKind::Page, Locale::Ru) => "Страница",
        (FormElementKind::Page, Locale::En) => "Page",
        (FormElementKind::CommandBar, Locale::Ru) => "Командная панель",
        (FormElementKind::CommandBar, Locale::En) => "Command bar",
        (FormElementKind::ButtonGroup, Locale::Ru) => "Группа кнопок",
        (FormElementKind::ButtonGroup, Locale::En) => "Button group",
        (FormElementKind::Field, Locale::Ru) => "Поле",
        (FormElementKind::Field, Locale::En) => "Field",
        (FormElementKind::Button, Locale::Ru) => "Кнопка",
        (FormElementKind::Button, Locale::En) => "Button",
        (FormElementKind::Decoration, Locale::Ru) => "Декорация",
        (FormElementKind::Decoration, Locale::En) => "Decoration",
        (FormElementKind::Addition, Locale::Ru) => "Дополнение",
        (FormElementKind::Addition, Locale::En) => "Addition",
        (FormElementKind::Other, _) => "Элемент формы",
    }
}

pub fn form_element_kind_sort_band(kind: FormElementKind) -> u8 {
    match kind {
        FormElementKind::Table => 10,
        FormElementKind::Group
        | FormElementKind::UsualGroup
        | FormElementKind::Pages
        | FormElementKind::Page
        | FormElementKind::CommandBar
        | FormElementKind::ButtonGroup => 20,
        FormElementKind::Field => 30,
        FormElementKind::Button => 40,
        FormElementKind::Decoration => 50,
        FormElementKind::Addition => 60,
        FormElementKind::Other => 70,
    }
}

pub use bsl_types::kind::MetadataKind;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSignature {
    pub params: Box<[bsl_types::kind::TypeId]>,

    pub defaults: Box<[bool]>,

    pub ret: bsl_types::kind::TypeId,

    pub max_args: Option<u32>,
}

impl FunctionSignature {
    pub fn required_count(&self) -> usize {
        self.defaults.iter().rposition(|has_default| !has_default).map_or(0, |i| i + 1)
    }
}
