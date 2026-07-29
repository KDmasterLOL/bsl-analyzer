//! Filters completion candidates by the execution environments of the code at
//! the cursor: a candidate the availability diagnostics would immediately
//! underline (`UnavailableInEnvironment`, `ModuleAccessibility`) is not
//! offered. The same model drives both sides — module environments ∩
//! compilation directive ∩ enclosing `#Если` regions on the caller, the
//! syntax-helper "Доступность" markup on the candidate, and the configured
//! `checked_environments` mask — so narrowing the code with `#Если НЕ
//! ВебКлиент` or unchecking the web client in `bsl-analyzer.toml` restores
//! the corresponding suggestions.

use hir::execution_env::{self, EnvFlags};
use hir::{AnnotationKind, ModItem};
use ide_db::RootDatabase;
use syntax::TextSize;
use vfs::FileId;

pub(super) struct EnvFilter {
    body_env: EnvFlags,
    checked_env: EnvFlags,
}

impl EnvFilter {
    /// A filter that admits everything — for contexts where availability must
    /// not be judged (union receivers, tests).
    pub(super) fn permissive() -> Self {
        Self { body_env: EnvFlags::EMPTY, checked_env: EnvFlags::EMPTY }
    }

    pub(super) fn at<DB: RootDatabase>(db: &DB, file_id: FileId, offset: TextSize) -> Self {
        let opts = db.env_options();
        let metadata = db.module_metadata(hir::ModuleId::new(file_id));
        let item_tree = db.item_tree(file_id);
        let method_at_cursor = method_item_at(&item_tree, offset);
        // A weaving interceptor's effective directive comes from the method it
        // intercepts, unknown here — never judge availability inside one
        // (mirrors inference disabling the checks for weaving bodies).
        if let Some((_, item)) = method_at_cursor {
            let annotations = match item {
                ModItem::Procedure(idx) => &item_tree.procedure(*idx).annotations,
                ModItem::Function(idx) => &item_tree.function(*idx).annotations,
                ModItem::Variable(_) => unreachable!("variables are filtered out above"),
            };
            let weaving = annotations.iter().any(|a| {
                matches!(
                    a.kind,
                    AnnotationKind::Before
                        | AnnotationKind::After
                        | AnnotationKind::Instead
                        | AnnotationKind::ChangeAndValidate
                )
            });
            if weaving {
                return Self { body_env: EnvFlags::EMPTY, checked_env: opts.checked_environments };
            }
        }
        let mut body_env = match method_at_cursor {
            Some((local_id, _)) => {
                execution_env::method_env(&item_tree, local_id, &metadata, &opts)
            }
            None => execution_env::module_code_env(&metadata, &opts),
        };
        if !body_env.is_empty() {
            let conditionals = db.conditional_tree(file_id);
            if !conditionals.is_empty() {
                // One containment walk covers both statement-level `#Если`
                // around the cursor and module-level regions around the method.
                body_env = body_env & execution_env::conditional_env_at(&conditionals, offset);
            }
        }
        Self { body_env, checked_env: opts.checked_environments }
    }

    /// Whether a member with availability `member_env` should be offered:
    /// exactly the complement of the diagnostic's verdict, so completion
    /// never suggests what the analyzer would underline. Unknown sides
    /// (empty masks) admit everything.
    pub(super) fn admits(&self, member_env: EnvFlags) -> bool {
        if self.body_env.is_empty() || member_env.is_empty() {
            return true;
        }
        (self.body_env.without(member_env) & self.checked_env).is_empty()
    }

    pub(super) fn admits_context(
        &self,
        context: Option<&bsl_platform::ContextAvailability>,
    ) -> bool {
        self.admits(EnvFlags::from_platform_context(context))
    }

    /// Whether a same-module method should be offered — the completion mirror
    /// of the local `ModuleAccessibility` rule: only the server side is ever a
    /// violation, a client-side caller reaching a server method is the form's
    /// regular remote call.
    pub(super) fn admits_local_method(&self, callee_env: EnvFlags) -> bool {
        if self.body_env.is_empty() || callee_env.is_empty() {
            return true;
        }
        (self.body_env.without(callee_env) & self.checked_env & EnvFlags::SERVER_SIDE).is_empty()
    }

    /// Whether the common module `name` is callable from the cursor's
    /// environments — the completion mirror of `ModuleAccessibility`:
    /// `ВызовСервера` modules stay visible to every client environment.
    /// Judged on the caller-visible flags; an extension that NARROWS an
    /// adopted module hides it here while the diagnostic tolerates that
    /// corner as a miss — the caller-visible model is what completion sells.
    pub(super) fn admits_common_module<DB: RootDatabase>(
        &self,
        db: &DB,
        file_id: FileId,
        name: &str,
    ) -> bool {
        if self.body_env.is_empty() {
            return true;
        }
        let Some(module) = db.resolve_common_module(file_id, name) else {
            return true;
        };
        let opts = db.env_options();
        let module_env = execution_env::common_module_env(&module, &opts);
        if module_env.is_empty() {
            return true;
        }
        let mut missing = self.body_env.without(module_env) & self.checked_env;
        if module.is_server_call() {
            missing = missing.without(EnvFlags::ALL_CLIENTS);
        }
        missing.is_empty()
    }
}

/// The top-level method whose source range contains `offset`. Inclusive end:
/// while an unfinished method is being typed at the end of the file, the
/// cursor commonly sits exactly at the method's current end — it still
/// belongs to that method, not to module code. The returned index is the
/// module-wide top-level item index — the same numbering `ModuleBodies` uses
/// for its per-method lower results.
pub(crate) fn method_item_at(
    item_tree: &hir::ItemTree,
    offset: TextSize,
) -> Option<(u32, &ModItem)> {
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
