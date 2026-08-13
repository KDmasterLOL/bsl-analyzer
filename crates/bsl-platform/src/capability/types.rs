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
    /// A method matched by spelling alone, whatever the receiver turns out to
    /// be. The over-approximation is deliberate and belongs only to diagnostics
    /// whose own docs declare that breadth: a name owned by several platform
    /// types, or by user code, is reported all the same. Where the receiver
    /// decides, use the security registry's `ModuleMethod` instead.
    AnyReceiverMethod,
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
