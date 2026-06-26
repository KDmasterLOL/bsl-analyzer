#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    ModalWindow,
    SynchronousCall,
    AsyncCall,
    SystemInformation,
    UnixUnavailableObject,
    TemporaryFilesDirectory,
    FormDataToValue,
    GetForm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    GlobalMethod,
    Method,
    Constructor,
    Type,
    GlobalProperty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Replacement {
    pub ru: &'static str,
    pub en: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityEntry {
    pub ru: &'static str,
    pub en: &'static str,
    pub kind: EntryKind,
    pub category: Category,
    pub replacement: Option<Replacement>,
}
