#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LifecycleGroup {
    DateTime,
    StringSearch,
    UserNotification,
    ManagedForm,
    ApplicationInterface,
    ErrorProcessing,
    ChartPresentation,
    EventLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementKind {
    GlobalMethod,
    Method,
    Constructor,
    Type,
    Property,
    GlobalProperty,
    Attribute,
    EnumName,
    EnumValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompatibilityBucket {
    Any,
    CompatibilityMode8_3_6,
    CompatibilityMode8_3_10,
    CompatibilityMode8_3_12,
    CompatibilityMode8_3_14,
    CompatibilityMode8_3_17,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayKind {
    Function,
    GlobalMethod,
    Method,
    Constructor,
    Type,
    Property,
    Attribute,
    EnumName,
    EnumValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OwnerType {
    pub ru: &'static str,
    pub en: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Replacement {
    pub ru: &'static str,
    pub en: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeprecationEntry {
    pub ru: &'static str,
    pub en: &'static str,
    pub element_kind: ElementKind,
    pub owner: Option<OwnerType>,
    pub group: LifecycleGroup,
    pub replacement: Option<Replacement>,
    pub compatibility: CompatibilityBucket,
    pub display: DisplayKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lookup<'a> {
    pub element_kind: ElementKind,
    pub owner: Option<&'a str>,
    pub name: &'a str,
}

impl<'a> Lookup<'a> {
    pub const fn new(element_kind: ElementKind, owner: Option<&'a str>, name: &'a str) -> Self {
        Self { element_kind, owner, name }
    }

    pub const fn global_method(name: &'a str) -> Self {
        Self::new(ElementKind::GlobalMethod, None, name)
    }

    pub const fn method(owner: &'a str, name: &'a str) -> Self {
        Self::new(ElementKind::Method, Some(owner), name)
    }

    pub const fn type_(name: &'a str) -> Self {
        Self::new(ElementKind::Type, None, name)
    }

    pub const fn property(owner: &'a str, name: &'a str) -> Self {
        Self::new(ElementKind::Property, Some(owner), name)
    }
}
