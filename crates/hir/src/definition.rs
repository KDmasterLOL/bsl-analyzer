//! Definition - unified symbol representation for IDE features.
//!
//! This module provides a unified `Definition` enum that represents any symbol
//! that can be resolved in BSL code. This is the central abstraction for all
//! IDE features (goto definition, hover, find references, completion, etc.).
//!
//! ## Architecture Pattern
//!
//! - Single Definition enum covering ALL symbol types
//! - Unified resolution API in Semantics
//! - Each IDE feature converts Definition to its own representation
//!
//! ## Resolution Priority (matches BSL semantics)
//!
//! 1. Local symbols (parameters, local variables) — highest priority (shadowing)
//! 2. Builtin platform functions/methods
//! 3. MDO plural forms (Справочники, Документы)
//! 4. Module-level methods and variables
//! 5. Cross-module qualified names (Module.Method)

use crate::{MethodId, ModuleId, Name, VariableId};
use hir_def::DefDatabase;
use std::sync::Arc;
use syntax::TextRange;
use vfs::FileId;

/// A resolved definition in BSL code.
///
/// This enum represents any symbol that can be referenced in BSL:
/// - Methods (procedures and functions)
/// - Variables (module-level and local)
/// - Parameters
/// - Builtin platform functions/methods
/// - Metadata objects (MDOs)
/// - Modules
///
/// # Usage
///
/// ```ignore
/// let sema = Semantics::new(db);
/// let definition = sema.resolve_name_to_definition(file_id, token)?;
///
/// match definition {
///     Definition::Method(id) => { /* goto method definition */ }
///     Definition::BuiltinFunction(name) => { /* show platform docs */ }
///     Definition::MdoObject { mdo_type, object_name } => { /* show MDO info */ }
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Definition {
    /// Method (procedure or function)
    ///
    /// Examples: `МояПроцедура()`, `МояФункция()`
    Method(MethodId),

    /// Module-level variable
    ///
    /// Examples: `Перем МояПеременная;`
    Variable(VariableId),

    /// Method parameter
    ///
    /// Parameters are identified by method + name + index.
    /// We don't have a dedicated ParameterId yet, so we store the necessary info.
    Parameter { method_id: MethodId, param_name: Name, param_index: u32 },

    /// Local variable (declared inside a method)
    ///
    /// Local variables are identified by method + name.
    /// We don't have a dedicated LocalVarId yet, so we store the necessary info.
    Local { method_id: MethodId, var_name: Name },

    /// Builtin platform function
    ///
    /// Examples: `НачатьТранзакцию()`, `Формат()`, `Сообщить()`
    BuiltinFunction(Name),

    /// Builtin method of a platform type
    ///
    /// Examples: `Строка.ВРег()`, `Массив.Добавить()`
    BuiltinMethod { type_name: Name, method_name: Name },

    /// MDO collection type (plural form)
    ///
    /// Examples: `Справочники`, `Документы`, `РегистрыСведений`
    ///
    /// These are the plural forms used in object model calls like:
    /// `Документы.ПКО.Method()` or `Catalogs.Name.Method()`
    MdoCollectionType(bsl_metadata::MdoType),

    /// Metadata object (specific instance)
    ///
    /// Examples:
    /// - `Документы.ПКО` → Document type "ПКО"
    /// - `Справочники.Валюты` → Catalog "Валюты"
    MdoObject { mdo_type: bsl_metadata::MdoType, object_name: Name },

    /// Manager module of a metadata object
    ///
    /// This is the KEY to solving the manager method highlighting problem!
    ///
    /// Examples:
    /// - `РегистрыСведений.ОчередьЗапросовERP.ДобавитьВОчередь()`
    ///   → Manager module method "ДобавитьВОчередь"
    ///
    /// When we have a 3-level path like `Collection.Object.Method`,
    /// and Method resolves to a manager module method, we return this variant.
    MdoManagerModule { mdo_type: bsl_metadata::MdoType, object_name: Name, file_id: FileId },

    /// Module (Common Module, Form Module, etc.)
    ///
    /// Examples: `ОбщегоНазначения`, `ОбщийМодуль1`
    Module(ModuleId),

    /// Virtual table field (for SDBL query analysis)
    ///
    /// Examples: `ДокументТовары.Номенклатура`, `РегистрОстатки.Количество`
    VirtualTableField { table_name: Name, field_name: Name },

    /// Unresolved reference
    ///
    /// The identifier could not be resolved to any known symbol.
    Unresolved,
}

impl Definition {
    /// Get the module containing this definition (if applicable).
    ///
    /// Returns `None` for builtin functions, MDO types, and unresolved symbols.
    pub fn module(&self, _db: &dyn DefDatabase) -> Option<ModuleId> {
        match self {
            Definition::Method(id) => Some(id.module),
            Definition::Variable(id) => Some(id.module),
            Definition::Parameter { method_id, .. } => Some(method_id.module),
            Definition::Local { method_id, .. } => Some(method_id.module),
            Definition::Module(id) => Some(*id),
            Definition::MdoManagerModule { file_id, .. } => {
                // Manager module file → ModuleId
                Some(ModuleId::new(*file_id))
            }
            // Builtins, MDOs, virtual tables don't have a module
            Definition::BuiltinFunction(_)
            | Definition::BuiltinMethod { .. }
            | Definition::MdoCollectionType(_)
            | Definition::MdoObject { .. }
            | Definition::VirtualTableField { .. }
            | Definition::Unresolved => None,
        }
    }

    /// Get the name of this definition.
    ///
    /// Returns `None` for unresolved symbols and some complex types.
    pub fn name(&self, db: &dyn DefDatabase) -> Option<Name> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.name),
            Definition::Variable(id) => crate::get_variable_info(id, db).map(|i| i.name),
            Definition::Parameter { param_name, .. } => Some(param_name.clone()),
            Definition::Local { var_name, .. } => Some(var_name.clone()),
            Definition::BuiltinFunction(name) => Some(name.clone()),
            Definition::BuiltinMethod { method_name, .. } => Some(method_name.clone()),
            Definition::MdoObject { object_name, .. } => Some(object_name.clone()),
            Definition::Module(_) => None,
            Definition::VirtualTableField { field_name, .. } => Some(field_name.clone()),
            Definition::MdoCollectionType(_) | Definition::MdoManagerModule { .. } => None,
            Definition::Unresolved => None,
        }
    }

    /// Check if this definition has the Export modifier.
    ///
    /// Only methods and variables can be exported.
    pub fn is_export(&self, db: &dyn DefDatabase) -> bool {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).is_some_and(|i| i.is_export),
            Definition::Variable(id) => {
                crate::get_variable_info(id, db).is_some_and(|i| i.is_export)
            }
            _ => false,
        }
    }

    /// Get the source range where this definition is declared.
    ///
    /// Returns `None` for builtin functions and unresolved symbols.
    pub fn source_range(&self, db: &dyn DefDatabase) -> Option<TextRange> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.source_range),
            Definition::Variable(id) => crate::get_variable_info(id, db).map(|i| i.source_range),
            _ => None,
        }
    }

    /// Get the name range where this definition is declared (identifier only).
    ///
    /// This is useful for diagnostics that should highlight only the name,
    /// not the entire declaration.
    pub fn name_range(&self, db: &dyn DefDatabase) -> Option<TextRange> {
        match self {
            Definition::Method(id) => crate::get_method_info(id, db).map(|i| i.name_range),
            _ => None,
        }
    }

    /// Get parsed documentation for this definition.
    ///
    /// Currently only implemented for methods.
    /// Returns structured documentation containing:
    /// - Purpose/description
    /// - Parameter types and descriptions
    /// - Return value types and descriptions
    /// - Examples, call options, deprecation info
    pub fn docs(&self, db: &dyn DefDatabase) -> Option<Arc<hir_def::docs::MethodDocs>> {
        match self {
            Definition::Method(id) => db.method_docs(*id),
            // TODO: Add docs support for other types (variables, parameters, etc.)
            _ => None,
        }
    }

    /// Get a human-readable label for this definition.
    ///
    /// This is used for UI display (completion items, hover, etc.).
    ///
    /// # Examples
    ///
    /// - Method: "МояПроцедура()"
    /// - Variable: "Перем МояПеременная"
    /// - Builtin: "Формат()"
    /// - MDO: "Документы.ПКО"
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
            Definition::BuiltinMethod { type_name, method_name } => {
                format!("{}.{}()", type_name.as_str(), method_name.as_str())
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

    /// Get the file ID where this definition is located.
    ///
    /// Returns `None` for builtin functions and unresolved symbols.
    pub fn file_id(&self, db: &dyn DefDatabase) -> Option<FileId> {
        self.module(db).map(|module| module.file_id)
    }

    /// Check if this is a method definition.
    pub fn is_method(&self) -> bool {
        matches!(self, Definition::Method(_))
    }

    /// Check if this is a variable definition (module-level or local).
    pub fn is_variable(&self) -> bool {
        matches!(self, Definition::Variable(_) | Definition::Local { .. })
    }

    /// Check if this is a parameter definition.
    pub fn is_parameter(&self) -> bool {
        matches!(self, Definition::Parameter { .. })
    }

    /// Check if this is a builtin (function or method).
    pub fn is_builtin(&self) -> bool {
        matches!(self, Definition::BuiltinFunction(_) | Definition::BuiltinMethod { .. })
    }

    /// Check if this is an MDO-related definition.
    pub fn is_mdo(&self) -> bool {
        matches!(
            self,
            Definition::MdoCollectionType(_)
                | Definition::MdoObject { .. }
                | Definition::MdoManagerModule { .. }
        )
    }
}
