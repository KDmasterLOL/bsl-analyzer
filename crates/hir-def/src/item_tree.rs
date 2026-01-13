//! ItemTree - a simplified AST that only contains items (module-level definitions).
//!
//! This is the primary IR for HIR. ItemTree serves as an "invalidation barrier" for
//! incremental computations: when you edit a procedure body (comments, logic), the ItemTree
//! doesn't change, so name resolution doesn't need to rerun.
//!
//! ItemTree is built per file, representing all top-level definitions:
//! - Procedures
//! - Functions
//! - Module variables
//!
//! ## Architecture
//!
//! ```text
//! AST (syntax) → ItemTree (hir-def) → HIR Queries → Public API → IDE Features
//!                    ↑
//!           "Invalidation Barrier"
//! ```
//!
//! When you change only the body of a procedure (not its signature), ItemTree stays
//! the same, avoiding expensive recomputation.

pub mod lower;

use crate::Name;
use base_db::RootQueryDb;
use la_arena::{Arena, Idx};
use std::sync::Arc;
use text_size::TextRange;
use vfs::FileId;

/// Lower a file's AST into an ItemTree.
///
/// This is the main public entry point for ItemTree construction.
pub fn lower_file(db: &dyn RootQueryDb, file_id: FileId) -> Arc<ItemTree> {
    lower::Ctx::lower_file(db, file_id)
}

/// Compact representation of all top-level definitions in a file.
///
/// ItemTree is the "invalidation barrier" - it only changes when signatures change,
/// not when procedure bodies are edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTree {
    /// Top-level items in the order they appear in the file.
    pub(crate) top_level: Vec<ModItem>,

    /// Arena for procedures (compact storage with stable indices).
    pub(crate) procedures: Arena<Procedure>,

    /// Arena for functions.
    pub(crate) functions: Arena<Function>,

    /// Arena for module variables.
    pub(crate) variables: Arena<Variable>,
}

impl Default for ItemTree {
    fn default() -> Self {
        Self {
            top_level: Vec::new(),
            procedures: Arena::new(),
            functions: Arena::new(),
            variables: Arena::new(),
        }
    }
}

impl ItemTree {
    /// Build ItemTree from a parse result (without Salsa).
    ///
    /// This is the pure version for streaming mode.
    pub fn from_parse(parse: &syntax::Parse<syntax::SyntaxNode>) -> Self {
        use syntax::ast::{self, AstNode};

        let file = match ast::SourceFile::cast(parse.syntax_node()) {
            Some(f) => f,
            None => return ItemTree::default(),
        };

        let mut tree = ItemTree::default();
        lower::lower_module_items_into(&file, &mut tree);
        tree
    }

    /// Get all top-level items.
    pub fn top_level_items(&self) -> &[ModItem] {
        &self.top_level
    }

    /// Get a procedure by its index.
    pub fn procedure(&self, idx: Idx<Procedure>) -> &Procedure {
        &self.procedures[idx]
    }

    /// Get a function by its index.
    pub fn function(&self, idx: Idx<Function>) -> &Function {
        &self.functions[idx]
    }

    /// Get a variable by its index.
    pub fn variable(&self, idx: Idx<Variable>) -> &Variable {
        &self.variables[idx]
    }

    /// Iterate over all procedures.
    pub fn procedures(&self) -> impl Iterator<Item = (Idx<Procedure>, &Procedure)> {
        self.procedures.iter()
    }

    /// Iterate over all functions.
    pub fn functions(&self) -> impl Iterator<Item = (Idx<Function>, &Function)> {
        self.functions.iter()
    }

    /// Iterate over all variables.
    pub fn variables(&self) -> impl Iterator<Item = (Idx<Variable>, &Variable)> {
        self.variables.iter()
    }
}

/// Top-level item in a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModItem {
    Procedure(Idx<Procedure>),
    Function(Idx<Function>),
    Variable(Idx<Variable>),
}

/// Procedure definition.
///
/// In BSL: `Процедура ИмяПроцедуры(Параметры) Экспорт`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Procedure {
    pub name: Name,
    pub is_export: bool,
    pub params: Box<[Param]>,
    pub annotations: Box<[Annotation]>,
    /// Source location for mapping back to AST.
    pub source_range: TextRange,
    /// Source location of the procedure name (for diagnostics).
    pub name_range: TextRange,
}

/// Function definition.
///
/// In BSL: `Функция ИмяФункции(Параметры) Экспорт`
/// Functions differ from procedures by having a return value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: Name,
    pub is_export: bool,
    pub params: Box<[Param]>,
    pub annotations: Box<[Annotation]>,
    /// Source location for mapping back to AST.
    pub source_range: TextRange,
    /// Source location of the function name (for diagnostics).
    pub name_range: TextRange,
}

/// Module-level variable.
///
/// In BSL: `Перем ИмяПеременной Экспорт;`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: Name,
    pub is_export: bool,
    /// Source location for mapping back to AST.
    pub source_range: TextRange,
}

/// Function or procedure parameter.
///
/// In BSL: `Знач Параметр = ЗначениеПоУмолчанию`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Name,
    /// Is this a value parameter (`Знач` keyword)?
    pub is_val: bool,
    /// Does this parameter have a default value?
    pub has_default: bool,
}

/// Annotation on a procedure or function.
///
/// In BSL: `&НаКлиенте`, `&НаСервере`, `&НаКлиентеНаСервере`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Annotation {
    pub kind: AnnotationKind,
}

/// Kind of annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    /// `&НаКлиенте` or `&AtClient` - runs on client
    AtClient,
    /// `&НаСервере` or `&AtServer` - runs on server
    AtServer,
    /// `&НаКлиентеНаСервере` or `&AtClientAtServer` - runs on both
    AtClientAtServer,
    /// `&НаКлиентеНаСервереБезКонтекста` or `&AtClientAtServerNoContext`
    AtClientAtServerNoContext,
    /// `&НаСервереБезКонтекста` or `&AtServerNoContext`
    AtServerNoContext,
    /// `&До` or `&Before` - extension method (before original)
    Before,
    /// `&После` or `&After` - extension method (after original)
    After,
    /// `&Вместо` or `&Instead` - extension method (instead of original)
    Instead,
}

/// Salsa tracked query for ItemTree construction.
///
/// This query is automatically cached and invalidated by Salsa when file content changes.
///
/// ## Performance
/// - LRU: 512 files (signatures don't change often)
/// - Depends on: parse (via FileIdInput)
/// - Invalidation: Automatic when file text changes
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
///     let file_id_input = base_db::FileIdInput::new(self, file_id);
///     hir_def::item_tree_query(self, file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 512)]
pub fn item_tree_query<'db>(
    db: &'db dyn base_db::RootQueryDb,
    file_id_input: base_db::FileIdInput<'db>,
) -> Arc<ItemTree> {
    let _span = tracing::info_span!("item_tree", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    lower_file(db, file_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_tree_default() {
        let tree = ItemTree::default();
        assert_eq!(tree.top_level_items().len(), 0);
    }

    #[test]
    fn test_procedure_creation() {
        let proc = Procedure {
            name: Name::new("ТестоваяПроцедура"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: TextRange::new(0.into(), 10.into()),
            name_range: TextRange::new(0.into(), 10.into()),
        };

        assert_eq!(proc.name.as_str(), "ТестоваяПроцедура");
        assert!(proc.is_export);
        assert_eq!(proc.params.len(), 0);
    }

    #[test]
    fn test_function_with_params() {
        let func = Function {
            name: Name::new("ТестоваяФункция"),
            is_export: false,
            params: Box::new([
                Param { name: Name::new("Параметр1"), is_val: true, has_default: false },
                Param { name: Name::new("Параметр2"), is_val: false, has_default: true },
            ]),
            annotations: Box::new([]),
            source_range: TextRange::new(0.into(), 10.into()),
            name_range: TextRange::new(0.into(), 10.into()),
        };

        assert_eq!(func.params.len(), 2);
        assert!(func.params[0].is_val);
        assert!(!func.params[0].has_default);
        assert!(!func.params[1].is_val);
        assert!(func.params[1].has_default);
    }

    #[test]
    fn test_annotation_kinds() {
        let ann1 = Annotation { kind: AnnotationKind::AtClient };
        let ann2 = Annotation { kind: AnnotationKind::AtServer };

        assert_ne!(ann1.kind, ann2.kind);
    }

    #[test]
    fn test_item_tree_procedures() {
        let mut tree = ItemTree::default();

        let proc_idx = tree.procedures.alloc(Procedure {
            name: Name::new("Процедура1"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: TextRange::new(0.into(), 10.into()),
            name_range: TextRange::new(0.into(), 10.into()),
        });

        tree.top_level.push(ModItem::Procedure(proc_idx));

        assert_eq!(tree.top_level_items().len(), 1);
        assert!(matches!(tree.top_level_items()[0], ModItem::Procedure(_)));

        let proc = tree.procedure(proc_idx);
        assert_eq!(proc.name.as_str(), "Процедура1");
    }
}
