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

/// Where a name may legitimately appear. A name matched outside its own
/// surface belongs to something else that happens to share the spelling:
/// `ЗаписьXML.ОткрытьФайл` is a serializer opening a stream, not the
/// like-named library method that hands a file to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Global context: callable only bare, never after a dot.
    GlobalMethod,
    Constructor,
    /// Method of a library common module, callable only as `<owner>.<name>`.
    /// Owners are Russian-only — the libraries shipping them are.
    ModuleMethod {
        owners: &'static [&'static str],
    },
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
