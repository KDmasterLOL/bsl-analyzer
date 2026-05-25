//! Type system for BSL.
//!
//! This module provides basic type information for BSL values and expressions.
//! Full type inference is planned for later iterations (12+).

pub mod doc_types;

pub use bsl_metadata::FormElementKind;

/// Managed-form data wrapper flavour.
///
/// Picks the platform type whose method table backs a form-data
/// (`ДанныеФормы…`) receiver:
///
/// | Variant | Platform type | When chosen |
/// |---------|---------------|-------------|
/// | [`Self::Structure`] | `ДанныеФормыСтруктура` / `FormDataStructure` | scalar / `<MainAttribute>` form attribute (e.g. `Объект` typed as `cfg:DocumentObject.X`) |
/// | [`Self::Collection`] | `ДанныеФормыКоллекция` / `FormDataCollection` | `ValueTable`-typed attribute with `<Columns>` (table inside the form) |
/// | [`Self::StructureWithCollection`] | `ДанныеФормыСтруктураСКоллекцией` / `FormDataStructureAndCollection` | object-typed attribute that also exposes table parts (covers `Объект` for documents/catalogs with tabular sections — the platform composite that exposes both fields and tabular collections) |
///
/// The platform type names live in `bsl-platform/data/platform_data.json`;
/// `platform_type_name` maps this enum to those names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormDataKind {
    /// Plain form-data structure (no nested collections).
    Structure,
    /// Form-data collection (`ValueTable` attribute).
    Collection,
    /// Form-data structure that also has nested form-data collections.
    StructureWithCollection,
}

impl FormDataKind {
    /// Russian platform type name for method/property lookup.
    pub fn platform_type_name(self) -> &'static str {
        match self {
            Self::Structure => "ДанныеФормыСтруктура",
            Self::Collection => "ДанныеФормыКоллекция",
            Self::StructureWithCollection => "ДанныеФормыСтруктураСКоллекцией",
        }
    }
}

/// Ordered chain of platform type names for the control's property and
/// method tables — `[base, extension?]`. Every consumer (field lookup,
/// method lookup, hover, completion, `hir::Type` facade) walks this
/// chain reversed (extension first, base second) so extension-only
/// members (`<Pages>.ТекущаяСтраница`, `<UsualGroup>.Скрыть`,
/// `<Page>` page-specific properties, …) reach the user.
///
/// `chain[0]` is the user-facing display key (the base wrapper name);
/// `chain.last()` is the most specific extension. `Other` returns the
/// empty slice — no platform table to query and method lookup falls
/// through to `Ty::Unknown` instead of mis-classifying.
///
/// The five group sub-kinds (`UsualGroup`, `Pages`, `Page`,
/// `CommandBar`, `ButtonGroup`) carry both the base `ГруппаФормы` and
/// their dedicated platform extension. The catch-all `Group` keeps a
/// single-element chain — extensions are optional.
///
/// Free function rather than inherent impl because [`FormElementKind`]
/// is defined in `bsl-metadata` (orphan rule).
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

/// Primary platform type name (the base wrapper, e.g. `ТаблицаФормы` /
/// `ГруппаФормы`) — kept as a thin convenience over
/// [`form_control_platform_type_chain`] so display callers (hover label,
/// `Ty::display_name`) don't allocate a slice walk for one entry.
///
/// `Other` returns `None`. All other kinds return `Some(chain[0])`.
pub fn form_control_platform_type_name(kind: FormElementKind) -> Option<&'static str> {
    form_control_platform_type_chain(kind).first().copied()
}

/// Walk the platform-type chain for `kind` **in reverse** (most-specific
/// extension first, base last) and return the first `Some(_)` produced
/// by `lookup`.
///
/// Encapsulates the "extension overrides base" precedence rule shared
/// by [`hir_ty::method_lookup::lookup_method`] and
/// [`hir_ty::platform_property_lookup::lookup_platform_property`]: both
/// query `PlatformData` per chain segment and want the kind-specific
/// extension table (e.g. `"Расширение группы формы для обычной
/// группы"`) to win over the base `ГруппаФормы` table.
///
/// `Other` has an empty chain → immediate `None` without invoking
/// `lookup`. Single-entry chains (e.g. `Field`, `Button`, `Group`,
/// `Decoration`, `Addition`, `Table`) collapse to one `lookup` call,
/// identical to the pre-helper behaviour.
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

/// Human-facing label for a form-element kind, bilingual.
///
/// Single source of truth for completion item details, hover badges and
/// any other UI surface that needs to name a kind. Lives in `hir-def`
/// rather than `bsl-metadata` because `Locale` is an interface-adapter
/// concern (i18n) and the entity layer should not depend on it (Clean
/// Architecture decision in plan v3.1, table row #5).
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

/// Sort band for completion popups. Tables (`10`) → groups (`20`) →
/// fields (`30`) → buttons (`40`) → decorations (`50`) → additions
/// (`60`) → other (`70`). Decoupled from `derive(Ord)` because the
/// append-only discriminant policy puts new variants AFTER `Other`,
/// which is the wrong UI order.
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

/// Function or procedure signature in type-kernel form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionSignature {
    /// Parameter types in declaration order.
    pub params: Box<[bsl_types::kind::TypeId]>,

    /// Per-parameter "has default value" flag (parallel to `params`).
    pub defaults: Box<[bool]>,

    /// Return type (`Undefined` for procedures).
    pub ret: bsl_types::kind::TypeId,

    /// Maximum number of arguments the caller may supply.
    pub max_args: Option<u32>,
}

impl FunctionSignature {
    /// Number of arguments that the caller MUST supply.
    pub fn required_count(&self) -> usize {
        self.defaults.iter().rposition(|has_default| !has_default).map_or(0, |i| i + 1)
    }
}
