pub mod lower;

use crate::Name;
use base_db::RootQueryDb;
use la_arena::{Arena, Idx};
use std::sync::Arc;
use text_size::{TextRange, TextSize};
use vfs::FileId;

pub fn lower_file(db: &dyn RootQueryDb, file_id: FileId) -> Arc<ItemTree> {
    lower::Ctx::lower_file(db, file_id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemTree {
    pub(crate) top_level: Vec<ModItem>,

    pub(crate) procedures: Arena<Procedure>,

    pub(crate) functions: Arena<Function>,

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

    pub fn top_level_items(&self) -> &[ModItem] {
        &self.top_level
    }

    pub fn procedure(&self, idx: Idx<Procedure>) -> &Procedure {
        &self.procedures[idx]
    }

    pub fn function(&self, idx: Idx<Function>) -> &Function {
        &self.functions[idx]
    }

    pub fn variable(&self, idx: Idx<Variable>) -> &Variable {
        &self.variables[idx]
    }

    pub fn procedures(&self) -> impl Iterator<Item = (Idx<Procedure>, &Procedure)> {
        self.procedures.iter()
    }

    pub fn functions(&self) -> impl Iterator<Item = (Idx<Function>, &Function)> {
        self.functions.iter()
    }

    pub fn variables(&self) -> impl Iterator<Item = (Idx<Variable>, &Variable)> {
        self.variables.iter()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModItem {
    Procedure(Idx<Procedure>),
    Function(Idx<Function>),
    Variable(Idx<Variable>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Procedure {
    pub name: Name,
    pub is_export: bool,
    pub params: Box<[Param]>,
    pub annotations: Box<[Annotation]>,
    pub source_range: TextRange,
    pub name_range: TextRange,
    pub param_list_range: Option<TextRange>,
    /// End of the declaration header — the closing `)` of the parameter list, or
    /// the export keyword when present. Anchors the full (possibly multi-line)
    /// signature slice, distinct from `name_range` which is the name token alone.
    pub sig_end: TextSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: Name,
    pub is_export: bool,
    pub params: Box<[Param]>,
    pub annotations: Box<[Annotation]>,
    pub source_range: TextRange,
    pub name_range: TextRange,
    pub param_list_range: Option<TextRange>,
    /// End of the declaration header — the closing `)` of the parameter list, or
    /// the export keyword when present. Anchors the full (possibly multi-line)
    /// signature slice, distinct from `name_range` which is the name token alone.
    pub sig_end: TextSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    pub name: Name,
    pub is_export: bool,
    pub annotations: Box<[Annotation]>,
    pub source_range: TextRange,
    pub name_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Name,
    pub is_val: bool,
    pub has_default: bool,
    pub name_range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Annotation {
    pub kind: AnnotationKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    AtClient,
    AtServer,
    AtClientAtServer,
    AtClientAtServerNoContext,
    AtServerNoContext,
    Before,
    After,
    Instead,
    ChangeAndValidate,
}

// Condensed module item index (no green-tree pin): feeds symbol/name resolution
// across modules. High cap keeps it across chunk-boundary LRU trims so a later
// chunk doesn't re-lower it from a re-parse.
#[salsa::tracked(lru = 32768)]
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
            param_list_range: None,
            sig_end: 10.into(),
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
                Param {
                    name: Name::new("Параметр1"),
                    is_val: true,
                    has_default: false,
                    name_range: TextRange::new(0.into(), 10.into()),
                },
                Param {
                    name: Name::new("Параметр2"),
                    is_val: false,
                    has_default: true,
                    name_range: TextRange::new(12.into(), 22.into()),
                },
            ]),
            annotations: Box::new([]),
            source_range: TextRange::new(0.into(), 10.into()),
            name_range: TextRange::new(0.into(), 10.into()),
            param_list_range: Some(TextRange::new(0.into(), 10.into())),
            sig_end: 10.into(),
        };

        assert_eq!(func.params.len(), 2);
        assert!(func.params[0].is_val);
        assert!(!func.params[0].has_default);
        assert!(!func.params[1].is_val);
        assert!(func.params[1].has_default);
    }

    #[test]
    fn test_annotation_kinds() {
        let ann1 = Annotation {
            kind: AnnotationKind::AtClient,
            range: TextRange::new(0.into(), 10.into()),
        };
        let ann2 = Annotation {
            kind: AnnotationKind::AtServer,
            range: TextRange::new(0.into(), 10.into()),
        };

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
            param_list_range: None,
            sig_end: 10.into(),
        });

        tree.top_level.push(ModItem::Procedure(proc_idx));

        assert_eq!(tree.top_level_items().len(), 1);
        assert!(matches!(tree.top_level_items()[0], ModItem::Procedure(_)));

        let proc = tree.procedure(proc_idx);
        assert_eq!(proc.name.as_str(), "Процедура1");
    }
}
