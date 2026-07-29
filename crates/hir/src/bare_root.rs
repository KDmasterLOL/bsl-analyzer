//! Who owns a bare receiver name at a position.
//!
//! The decision itself is [`bare_global_name_claim`](crate::bare_global_name_claim) —
//! this module only binds a position to the enclosing body so the predicate can
//! be asked positionally. Every consumer that types a receiver from its
//! identifier text must go through here; a consumer with its own guard set
//! drifts from the diagnostic's verdict, which is how the shadowed-global
//! suggestions kept coming back.

use hir_def::item_tree::{ItemTree, ModItem};
use hir_ty::db::HirDatabase;
use syntax::{SyntaxNode, TextSize};
use vfs::FileId;

use crate::{
    bare_global_name_claim, infer_owner, BareGlobalClaim, BodyShadowScope, DefWithBodyId, ExprId,
    ModuleId, Name, Resolver, TypeId,
};

/// The top-level method whose source range contains `offset`. Inclusive end:
/// while an unfinished method is being typed at the end of the file, the
/// cursor commonly sits exactly at the method's current end — it still
/// belongs to that method, not to module code. The returned index is the
/// module-wide top-level item index — the same numbering `ModuleBodies` uses
/// for its per-method lower results.
pub fn method_item_at(item_tree: &ItemTree, offset: TextSize) -> Option<(u32, &ModItem)> {
    item_tree
        .top_level_items()
        .iter()
        .enumerate()
        .find(|(_, item)| {
            let range = match item {
                ModItem::Procedure(idx) => item_tree.procedure(*idx).source_range,
                ModItem::Function(idx) => item_tree.function(*idx).source_range,
                ModItem::Variable(_) => return false,
            };
            range.contains(offset) || range.end() == offset
        })
        .map(|(local_id, item)| (local_id as u32, item))
}

/// The user symbol claiming `name` at this read, plus the body owner needed to
/// type its reaching assignment. The read position is the receiver's own start,
/// so only assignments sequential inference has already completed claim the name.
pub fn claim_at<DB: HirDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
    name: &Name,
    read_offset: TextSize,
) -> (Option<BareGlobalClaim>, DefWithBodyId) {
    let module_id = ModuleId::new(file_id);
    let item_tree = db.item_tree(file_id);
    let module_bodies = db.module_bodies_ref(module_id);
    let (owner, lower_result) = match method_item_at(&item_tree, offset) {
        Some((local_id, _)) => {
            (DefWithBodyId::Method(local_id), module_bodies.lower_result(local_id))
        }
        None => (DefWithBodyId::ModuleCode, module_bodies.module_code_result()),
    };
    let scope = lower_result.map(|r| BodyShadowScope {
        body: &r.body,
        source_map: &r.source_map,
        read_offset,
    });
    let resolver = Resolver::with_builtins_and_workspace(module_id);
    (bare_global_name_claim(db, &resolver, scope.as_ref(), name), owner)
}

/// Same, keyed off a receiver node: the read position is the node's own start.
pub fn claim_at_node<DB: HirDatabase>(
    db: &DB,
    file_id: FileId,
    offset: TextSize,
    name: &Name,
    root: &SyntaxNode,
) -> (Option<BareGlobalClaim>, DefWithBodyId) {
    claim_at(db, file_id, offset, name, root.text_range().start())
}

/// The inferred type of a reaching assignment's value — the claiming local's
/// type at the read. `None` when inference has nothing for the expression.
pub fn reaching_value_ty<DB: HirDatabase>(
    db: &DB,
    file_id: FileId,
    owner: DefWithBodyId,
    value_id: ExprId,
) -> Option<TypeId> {
    infer_owner(db, file_id, owner).type_id_of_expr(value_id)
}
