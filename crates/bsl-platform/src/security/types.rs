#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    FileSystem,
    Internet,
    ExternalApp,
    OsUsers,
    ExecuteExternalCode,
    PrivilegedMode,
    SafeMode,
    SafeModeQuery,
    PrivilegedModeQuery,
    Logging,
    Transaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    Critical,
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Path,
    Url,
    Cmd,
    ModeBool { opens_unsafe_when: bool },
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lifetime {
    Begin,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    GlobalMethod,
    Constructor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamRole {
    pub index: u8,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityEntry {
    pub ru: &'static str,
    pub en: &'static str,
    pub kind: EntryKind,
    pub category: Category,
    pub severity: Severity,
    pub params: &'static [ParamRole],
    pub lifetime: Option<Lifetime>,
}
