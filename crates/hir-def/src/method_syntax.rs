//! A method's syntax tree, cut loose from its file.
//!
//! Every per-method query below this one must be recomputed only when the
//! method itself changed. The file tree cannot say that: its nodes carry file
//! offsets, so after an edit anywhere before a method every node of the method
//! is a different node. The green tree does not know offsets — it is the same
//! value for the same method text — so it is what a body query depends on.
//! When only some other method changed, this query re-runs (the parse is new)
//! but produces an equal value, and salsa lets everything below it stand.
//!
//! Equality is structural, not textual: a method inside a module-level
//! `#Если` parses with a different boundary stack than the same text outside
//! one, so equal text does not guarantee an equal tree.

use syntax::GreenNode;
use syntax::SyntaxNode;

use crate::{DefDatabase, MethodIdInput};

#[derive(Debug, Clone, Eq)]
pub struct MethodSyntax {
    green: GreenNode,
    is_function: bool,
}

/// Структурное равенство с замыканием по указателю: после переразбора одного
/// метода поддеревья остальных — те же самые узлы, и бэкдейт их мемо стоит
/// одно сравнение адресов, а не обход.
impl PartialEq for MethodSyntax {
    fn eq(&self, other: &Self) -> bool {
        self.is_function == other.is_function && syntax::green_eq(&self.green, &other.green)
    }
}

impl MethodSyntax {
    /// A tree rooted at the method: offsets in it are method-relative.
    pub fn detached_root(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn is_function(&self) -> bool {
        self.is_function
    }

    pub fn green(&self) -> &GreenNode {
        &self.green
    }
}

/// A method node of the file tree, re-rooted at itself. The green tree is
/// shared with the file tree, so this is a refcount bump, not a copy.
pub fn detach(node: &SyntaxNode) -> SyntaxNode {
    SyntaxNode::new_root(node.green().into_owned())
}

/// Bytes of a green tree are not observable; the text length under it is a
/// stable proxy of the same order (tokens store their text inline).
fn method_syntax_heap(v: &Option<MethodSyntax>) -> usize {
    v.as_ref().map_or(0, |s| u32::from(s.green.text_len()) as usize * 2)
}

// Retained per method so that a body memo evicted below it does not drag the
// file's parse back in; see the retention rule on `method_lower_query`.
#[salsa::tracked(lru = 8192, heap_size = method_syntax_heap, returns(ref))]
pub fn method_syntax_query<'db>(
    db: &'db dyn DefDatabase,
    method: MethodIdInput<'db>,
) -> Option<MethodSyntax> {
    let mid = method.method_id(db);
    let file_id = mid.module.file_id;
    let _span = tracing::info_span!("method_syntax", file_id = file_id.0, local_id = ?mid.local_id)
        .entered();

    let item_tree = db.item_tree_ref(file_id);
    let (range, is_function) = item_tree.method_at(mid.local_id)?;
    let parse = db.parse_ref(file_id);
    let node = crate::symbol_tree::method_node_at(parse, range, is_function)?;
    Some(MethodSyntax { green: node.green().into_owned(), is_function })
}

/// Retention cap of `method_syntax_query`; see `set_lowering_lru_sweep_mode`.
pub(crate) fn set_lru_capacity(db: &mut dyn DefDatabase, cap: usize) {
    method_syntax_query::set_lru_capacity(db, cap);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_green(code: &str, name: &str) -> (GreenNode, u32) {
        let parse = parser::parse_with_shared_cache(code);
        let item_tree = crate::item_tree::ItemTree::from_parse(&parse);
        let (range, is_function) =
            item_tree.method_at(crate::MethodKey::first(name)).expect("a method there");
        let node = crate::symbol_tree::method_node_at(&parse, range, is_function).unwrap();
        (node.green().into_owned(), u32::from(range.start()))
    }

    #[test]
    fn the_green_tree_ignores_where_the_method_sits() {
        let second = "Процедура Б()\n\tХ = 1;\nКонецПроцедуры\n";
        let before = format!("Процедура А()\nКонецПроцедуры\n\n{second}");
        let after = format!("Процедура А()\n\tДобавлено = 2;\nКонецПроцедуры\n\n{second}");
        let (g1, start1) = method_green(&before, "Б");
        let (g2, start2) = method_green(&after, "Б");
        assert_ne!(start1, start2, "the edit must move the second method");
        assert_eq!(g1, g2);
    }

    #[test]
    fn an_edit_inside_the_method_changes_the_green_tree() {
        let a = "Процедура А()\nКонецПроцедуры\n\nПроцедура Б()\n\tХ = 1;\nКонецПроцедуры\n";
        let b = "Процедура А()\nКонецПроцедуры\n\nПроцедура Б()\n\tХ = 2;\nКонецПроцедуры\n";
        assert_ne!(method_green(a, "Б").0, method_green(b, "Б").0);
    }

    #[test]
    fn a_detached_root_starts_at_zero_and_keeps_the_text() {
        let code = "Процедура А()\nКонецПроцедуры\n\nФункция Б()\n\tВозврат 1;\nКонецФункции\n";
        let parse = parser::parse_with_shared_cache(code);
        let item_tree = crate::item_tree::ItemTree::from_parse(&parse);
        let (range, is_function) = item_tree.method_at(crate::MethodKey::first("Б")).unwrap();
        let node = crate::symbol_tree::method_node_at(&parse, range, is_function).unwrap();
        let root = detach(&node);
        assert_eq!(u32::from(root.text_range().start()), 0);
        assert_eq!(root.text_range().len(), range.len());
        assert_eq!(root.text().to_string(), node.text().to_string());
        assert_eq!(root.kind(), syntax::SyntaxKind::FUNCTION_DEF);
    }
}
