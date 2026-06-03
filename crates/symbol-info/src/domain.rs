use bsl_metadata::MdoType;
use hir::{MethodId, ModuleId, Name};
use smol_str::SmolStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodKind {
    Procedure,
    Function,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Russian,
    English,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub russian: SmolStr,
    pub english: Option<SmolStr>,
    pub description: Option<String>,
    pub is_hyperlink: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureParam {
    pub name: SmolStr,
    pub types: Vec<TypeRef>,
    pub is_optional: bool,
    pub default_value: Option<SmolStr>,
    pub description: Option<String>,
    pub is_val: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalleeKind {
    PlatformMethod { type_name: SmolStr, method_name: SmolStr },
    GlobalFunction { name: SmolStr },
    CommonModuleMethod { module: Name, method: Name },
    ManagerModuleMethod { mdo_type: MdoType, object: Name, method: Name },
    PlatformManagerMethod { mdo_type: MdoType, method: Name },
    LocalMethod { module_id: ModuleId, method: Name },
    PlatformConstructor { type_name: SmolStr },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureSource {
    Platform,
    GlobalFunction,
    PlatformManager,
    CommonModule,
    ManagerModule,
    Local,
    PlatformConstructor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeExample {
    pub code: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSignature {
    pub kind: MethodKind,
    pub name_russian: SmolStr,
    pub name_english: Option<SmolStr>,
    pub qualifier: Option<SmolStr>,
    pub prefix: Option<SmolStr>,
    pub params: Vec<SignatureParam>,
    pub returns: Vec<TypeRef>,
    pub purpose: Option<String>,
    pub description: Option<String>,
    pub examples: Vec<CodeExample>,
    pub notes: Option<String>,
    pub deprecation: Option<String>,
    pub is_export: bool,
    pub source: SignatureSource,
    pub method_id: Option<MethodId>,
}
