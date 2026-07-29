//! Who owns a bare receiver name at the cursor.
//!
//! The decision itself belongs to `hir` (`bare_global_name_claim`) — this module
//! only binds a cursor position to the enclosing body so the predicate can be
//! asked positionally. Every completion source that types a receiver from its
//! identifier text must go through here; a source with its own guard set drifts
//! from the diagnostic's verdict, which is how the shadowed-global suggestions
//! kept coming back.

use hir::Name;
use ide_db::RootDatabase;
use syntax::SyntaxNode;

use crate::completion::env_filter::method_item_at;

/// The user symbol claiming `name` at this read, plus the body owner needed to
/// type its reaching assignment. The read position is the receiver's own start,
/// so only assignments sequential inference has already completed claim the name.
pub(crate) fn claim_at<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    name: &Name,
    read_offset: syntax::TextSize,
) -> (Option<hir::BareGlobalClaim>, hir::DefWithBodyId) {
    let module_id = hir::ModuleId::new(file_id);
    let item_tree = db.item_tree(file_id);
    let module_bodies = db.module_bodies_ref(module_id);
    let (owner, lower_result) = match method_item_at(&item_tree, offset) {
        Some((local_id, _)) => {
            (hir::DefWithBodyId::Method(local_id), module_bodies.lower_result(local_id))
        }
        None => (hir::DefWithBodyId::ModuleCode, module_bodies.module_code_result()),
    };
    let scope = lower_result.map(|r| hir::BodyShadowScope {
        body: &r.body,
        source_map: &r.source_map,
        read_offset,
    });
    let resolver = hir::Resolver::with_builtins_and_workspace(module_id);
    (hir::bare_global_name_claim(db, &resolver, scope.as_ref(), name), owner)
}

/// Same, keyed off a receiver node: the read position is the node's own start.
pub(crate) fn claim_at_node<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    name: &Name,
    root: &SyntaxNode,
) -> (Option<hir::BareGlobalClaim>, hir::DefWithBodyId) {
    claim_at(db, file_id, offset, name, root.text_range().start())
}

/// The inferred type of a reaching assignment's value — the claiming local's
/// type at the read. `None` when inference has nothing for the expression.
pub(crate) fn reaching_value_ty<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    owner: hir::DefWithBodyId,
    value_id: hir::ExprId,
) -> Option<hir::TypeId> {
    hir::infer_owner(db, file_id, owner).type_id_of_expr(value_id)
}
