use crate::{MethodId, ModuleId, Name, VariableId};
use hir_def::DefDatabase;
use hir_ty::PlatformMethodHandle;
use std::sync::Arc;
use syntax::TextRange;
use vfs::FileId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Definition {
    Method(MethodId),
    Variable(VariableId),
    Parameter { method_id: MethodId, param_name: Name, param_index: u32 },
    Local { method_id: MethodId, var_name: Name },
    BuiltinFunction(Name),

    BuiltinMethodHandle { handle: PlatformMethodHandle, method_name: Name },
    MdoCollectionType(bsl_metadata::MdoType),
    MdoObject { mdo_type: bsl_metadata::MdoType, object_name: Name },

    MdoManagerModule { mdo_type: bsl_metadata::MdoType, object_name: Name, file_id: FileId },
    Module(ModuleId),
    VirtualTableField { table_name: Name, field_name: Name },
    Unresolved,
}

impl Definition {
    /// Whether these two name the same thing in the language, rather than in the source.
    ///
    /// Several variants carry a [`Name`] spelled the way the occurrence spelled it, and BSL
    /// is case-insensitive: `Сообщить` and `СООБЩИТЬ` are one function written twice. Derived
    /// equality compares those spellings byte for byte, so a caller deduplicating by it sees
    /// two entities where the language has one — and answers with a choice between clones of
    /// one place.
    ///
    /// Only the name-carrying variants fold. The ones keyed by an id keep exact equality:
    /// two methods called `Расчёт` in two modules are genuinely two, and folding them would
    /// hide an ambiguity that is real.
    pub fn same_entity(&self, other: &Self) -> bool {
        match (self, other) {
            (Definition::BuiltinFunction(left), Definition::BuiltinFunction(right)) => {
                left.eq_ignore_case(right)
            }
            (
                Definition::BuiltinMethodHandle { handle: left, method_name: left_name },
                Definition::BuiltinMethodHandle { handle: right, method_name: right_name },
            ) => left == right && left_name.eq_ignore_case(right_name),
            (
                Definition::MdoObject { mdo_type: left, object_name: left_name },
                Definition::MdoObject { mdo_type: right, object_name: right_name },
            ) => left == right && left_name.eq_ignore_case(right_name),
            (
                Definition::MdoManagerModule {
                    mdo_type: left,
                    object_name: left_name,
                    file_id: left_file,
                },
                Definition::MdoManagerModule {
                    mdo_type: right,
                    object_name: right_name,
                    file_id: right_file,
                },
            ) => left == right && left_file == right_file && left_name.eq_ignore_case(right_name),
            (
                Definition::VirtualTableField { table_name: left_table, field_name: left_field },
                Definition::VirtualTableField { table_name: right_table, field_name: right_field },
            ) => left_table.eq_ignore_case(right_table) && left_field.eq_ignore_case(right_field),
            (left, right) => left == right,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferenceScope {
    FileLocal,
    ModuleSymbolWorkspace,
    Unknown,
}

impl Definition {
    pub fn module(&self, _db: &dyn DefDatabase) -> Option<ModuleId> {
        match self {
            Definition::Method(id) => Some(id.module),
            Definition::Variable(id) => Some(id.module),
            Definition::Parameter { method_id, .. } => Some(method_id.module),
            Definition::Local { method_id, .. } => Some(method_id.module),
            Definition::Module(id) => Some(*id),
            Definition::MdoManagerModule { file_id, .. } => Some(ModuleId::new(*file_id)),
            Definition::BuiltinFunction(_)
            | Definition::BuiltinMethodHandle { .. }
            | Definition::MdoCollectionType(_)
            | Definition::MdoObject { .. }
            | Definition::VirtualTableField { .. }
            | Definition::Unresolved => None,
        }
    }

    pub fn name(&self, db: &dyn DefDatabase) -> Option<Name> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.name),
            Definition::Variable(id) => crate::get_variable_info(id, db).map(|i| i.name),
            Definition::Parameter { param_name, .. } => Some(param_name.clone()),
            Definition::Local { var_name, .. } => Some(var_name.clone()),
            Definition::BuiltinFunction(name) => Some(name.clone()),
            Definition::BuiltinMethodHandle { method_name, .. } => Some(method_name.clone()),
            Definition::MdoObject { object_name, .. } => Some(object_name.clone()),
            Definition::Module(_) => None,
            Definition::VirtualTableField { field_name, .. } => Some(field_name.clone()),
            Definition::MdoCollectionType(_) | Definition::MdoManagerModule { .. } => None,
            Definition::Unresolved => None,
        }
    }

    pub fn is_export(&self, db: &dyn DefDatabase) -> bool {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).is_some_and(|i| i.is_export),
            Definition::Variable(id) => {
                crate::get_variable_info(id, db).is_some_and(|i| i.is_export)
            }
            _ => false,
        }
    }

    pub fn reference_scope(&self, db: &dyn DefDatabase) -> ReferenceScope {
        match self {
            Definition::Parameter { .. } | Definition::Local { .. } => ReferenceScope::FileLocal,
            Definition::Method(_) | Definition::Variable(_) => {
                if self.is_export(db) {
                    ReferenceScope::ModuleSymbolWorkspace
                } else {
                    ReferenceScope::FileLocal
                }
            }
            Definition::BuiltinFunction(_)
            | Definition::BuiltinMethodHandle { .. }
            | Definition::MdoCollectionType(_)
            | Definition::MdoObject { .. }
            | Definition::MdoManagerModule { .. }
            | Definition::Module(_)
            | Definition::VirtualTableField { .. }
            | Definition::Unresolved => ReferenceScope::Unknown,
        }
    }

    pub fn source_range(&self, db: &dyn DefDatabase) -> Option<TextRange> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.source_range),
            Definition::Variable(id) => crate::get_variable_info(id, db).map(|i| i.source_range),
            _ => None,
        }
    }

    pub fn name_range(&self, db: &dyn DefDatabase) -> Option<TextRange> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.name_range),
            _ => None,
        }
    }

    pub fn docs(&self, db: &dyn DefDatabase) -> Option<Arc<hir_def::docs::MethodDocs>> {
        match self {
            Definition::Method(id) => db.method_docs(*id),
            _ => None,
        }
    }

    pub fn label(&self, db: &dyn DefDatabase) -> String {
        match self {
            Definition::Method(id) => {
                let info = crate::get_method_info(id, db);
                let name = info.as_ref().map_or_else(Name::missing, |i| i.name.clone());
                if info.is_some_and(|i| i.is_function) {
                    format!("Функция {}()", name.as_str())
                } else {
                    format!("Процедура {}()", name.as_str())
                }
            }
            Definition::Variable(_) => {
                let name = self.name(db).unwrap_or_else(Name::missing);
                format!("Перем {}", name.as_str())
            }
            Definition::Parameter { param_name, .. } => {
                format!("Параметр {}", param_name.as_str())
            }
            Definition::Local { var_name, .. } => {
                format!("Локальная переменная {}", var_name.as_str())
            }
            Definition::BuiltinFunction(name) => {
                format!("Builtin: {}()", name.as_str())
            }
            Definition::BuiltinMethodHandle { handle, method_name } => {
                use hir_ty::PlatformMethodOrigin;
                let qualifier = match &handle.origin {
                    PlatformMethodOrigin::Scalar { type_name } => type_name.to_string(),
                    PlatformMethodOrigin::Prefixed { mdo_type, mdo_name, .. } => {
                        format!("{}.{}", mdo_type.russian_name(), mdo_name.as_str())
                    }
                };
                format!("{}.{}()", qualifier, method_name.as_str())
            }
            Definition::MdoCollectionType(mdo_type) => {
                format!("MDO Collection: {}", mdo_type.russian_name())
            }
            Definition::MdoObject { mdo_type, object_name } => {
                format!("{}.{}", mdo_type.russian_name(), object_name.as_str())
            }
            Definition::MdoManagerModule { mdo_type, object_name, .. } => {
                format!("Manager Module: {}.{}", mdo_type.russian_name(), object_name.as_str())
            }
            Definition::Module(_) => "Module".to_string(),
            Definition::VirtualTableField { table_name, field_name } => {
                format!("{}.{}", table_name.as_str(), field_name.as_str())
            }
            Definition::Unresolved => "<unresolved>".to_string(),
        }
    }

    pub fn file_id(&self, db: &dyn DefDatabase) -> Option<FileId> {
        self.module(db).map(|module| module.file_id)
    }

    pub fn is_method(&self) -> bool {
        matches!(self, Definition::Method(_))
    }

    pub fn is_variable(&self) -> bool {
        matches!(self, Definition::Variable(_) | Definition::Local { .. })
    }

    pub fn is_parameter(&self) -> bool {
        matches!(self, Definition::Parameter { .. })
    }

    pub fn is_builtin(&self) -> bool {
        matches!(self, Definition::BuiltinFunction(_) | Definition::BuiltinMethodHandle { .. })
    }

    pub fn is_mdo(&self) -> bool {
        matches!(
            self,
            Definition::MdoCollectionType(_)
                | Definition::MdoObject { .. }
                | Definition::MdoManagerModule { .. }
        )
    }
}
