//! Domain entities for method/function signatures.
//!
//! All types here are plain data. No Salsa, no database references, no IO.

use bsl_metadata::MdoType;
use hir::{MethodId, ModuleId, Name};
use smol_str::SmolStr;

/// Whether a callable returns a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MethodKind {
    Procedure,
    Function,
}

/// Language choice for presenters that support bilingual output.
///
/// Only [`Russian`](Lang::Russian) is currently rendered; [`English`](Lang::English)
/// is reserved for a future hover/completion variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Russian,
    English,
}

/// A single type alternative.
///
/// For union types a `SignatureParam` holds several `TypeRef`s — presenters join
/// them with ` | `.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    /// Russian type name (e.g. `"Строка"`).
    pub russian: SmolStr,
    /// English type name (e.g. `"String"`). `None` for user-defined types.
    pub english: Option<SmolStr>,
    /// Per-type description, used for union types where each alternative has its
    /// own explanation (`- ЛюбаяСсылка - объект …`, `- Строка - полное имя …`).
    pub description: Option<String>,
    /// True when the declared type is a hyperlink target rather than a real type.
    pub is_hyperlink: bool,
}

/// A single parameter in a [`SymbolSignature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureParam {
    /// Original-case name from source (`"ИмяРеквизита"`).
    pub name: SmolStr,
    /// Declared types. Empty means the doc did not declare any — presenters show
    /// `"Произвольный"` or nothing.
    pub types: Vec<TypeRef>,
    /// Whether the parameter has a default value (and is therefore optional at
    /// the call site).
    pub is_optional: bool,
    /// Textual default value from source (`"Неопределено"`, `"0"`, `""`).
    pub default_value: Option<SmolStr>,
    /// Long-form description for this parameter.
    pub description: Option<String>,
    /// `Знач` modifier. Presenters hide it (server-side calling convention with
    /// no caller-observable effect), but we keep it in the entity so that future
    /// formatters (linters, refactor previews) can surface it.
    pub is_val: bool,
}

/// What is being called at a specific source position.
///
/// Produced by the `resolve_callee` use case from CST walk; consumed by
/// adapters to look up the concrete [`SymbolSignature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalleeKind {
    /// Platform-defined method on a known type (`Строка.Найти`).
    PlatformMethod { type_name: SmolStr, method_name: SmolStr },
    /// Platform-defined global function (`НачатьТранзакцию`).
    GlobalFunction { name: SmolStr },
    /// User-defined method in a common module (`ОбщегоНазначения.ЗначениеРеквизитаОбъекта`).
    CommonModuleMethod { module: Name, method: Name },
    /// User-defined method in a manager module (`Справочники.Склады.ВариантыВыбораГруппыСкладов`).
    ManagerModuleMethod { mdo_type: MdoType, object: Name, method: Name },
    /// Platform-defined manager method (`Справочники.Склады.НайтиПоКоду`).
    PlatformManagerMethod { mdo_type: MdoType, method: Name },
    /// Method in the caller's own module.
    LocalMethod { module_id: ModuleId, method: Name },
    /// Constructor of a platform type (`Новый Массив(...)`). Carries the
    /// user-typed (original-case) type name so presenters can reuse it
    /// unchanged in the rendered signature.
    PlatformConstructor { type_name: SmolStr },
}

/// Data source a signature was built from; carried mainly for diagnostics and
/// test assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureSource {
    Platform,
    GlobalFunction,
    PlatformManager,
    CommonModule,
    ManagerModule,
    Local,
    /// Built from [`CalleeKind::PlatformConstructor`]. Kept separate from
    /// `Platform` so hover can render "Конструктор" instead of "Метод" and
    /// completion can surface `CompletionItemKind::Constructor`.
    PlatformConstructor,
}

/// A source code example, surfaced by hover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeExample {
    pub code: String,
    pub description: Option<String>,
}

/// The unified, presenter-ready view of a callable symbol.
///
/// Filled by adapters from a source-specific representation; consumed by
/// presenters to build LSP `SignatureHelp`, hover markdown, and completion
/// detail + documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSignature {
    pub kind: MethodKind,
    /// Russian method/function name (`"ВРег"`).
    pub name_russian: SmolStr,
    /// English name (`"Upper"`). `None` for user-defined symbols.
    pub name_english: Option<SmolStr>,
    /// Dotted qualifier printed in front of the name, including trailing dot
    /// (`"ОбщегоНазначения."`, `"Справочники.Партнеры."`). `None` for free
    /// functions and local methods.
    pub qualifier: Option<SmolStr>,
    /// Non-dotted prefix printed before `qualifier + name` (e.g. `"Новый "`
    /// for platform constructors). Kept separate from `qualifier` because
    /// that field is contractually dot-terminated and callers compare on its
    /// dotted shape.
    pub prefix: Option<SmolStr>,
    pub params: Vec<SignatureParam>,
    /// Declared return types. Empty for procedures and for undocumented
    /// functions.
    pub returns: Vec<TypeRef>,
    /// Short one-line description used by SignatureHelp's `doc` field.
    pub purpose: Option<String>,
    /// Long-form description used by hover.
    pub description: Option<String>,
    pub examples: Vec<CodeExample>,
    pub notes: Option<String>,
    pub deprecation: Option<String>,
    pub is_export: bool,
    pub source: SignatureSource,
    /// For user-defined methods, the backing method id; enables downstream
    /// features (navigation, references) to route back into HIR.
    pub method_id: Option<MethodId>,
}
