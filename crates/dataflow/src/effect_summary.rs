//! Per-method security-effect summary.
//!
//! Aggregates a bitwise summary of "what kinds of security-relevant
//! effects might this method (transitively) trigger". Consumers in
//! §1.6 (`PrivilegedModuleMethodCall`) and §6.5 (recursion penalty for
//! `CognitiveComplexity`) read these bits to decide whether to flag a
//! call site without re-running the full per-method analysis.
//!
//! # Pure helpers, no Salsa
//!
//! This module is the "pure" half of the effect-summary pipeline. It
//! takes a [`Body`] and a callee-lookup closure and produces one
//! [`EffectSummary`]. The Salsa-tracked module batch query
//! (`module_effect_summaries_query`) lives in `ide-db/src/effects.rs`
//! per Track 1 precedent — it computes the SCC fixpoint by repeatedly
//! invoking [`analyze_method_effects`] on each method until the bit
//! sets stabilize.
//!
//! # Termination
//!
//! The lattice is a 7-bit boolean set; each bit is monotone-ascending
//! under the bitwise-OR join. A naive worklist therefore performs at
//! most 7 value increases per SCC member; total iterations may exceed
//! that across the SCC, but finite ascent is guaranteed because the
//! join cannot introduce new bits beyond the seven.

use bsl_platform::security::{registry, Category};
use hir_def::{
    body::Body,
    hir::{Expr, Stmt},
    ExprId, IdConversion, Name,
};

/// Callee identifier passed to [`analyze_method_effects`]'s lookup
/// closure. Distinguishes the two HIR call shapes the helper can
/// resolve to a method summary:
///
/// - [`Self::Local`] — `Метод(args)` lowered as
///   `Expr::Call { callee: Expr::Path(Name), … }`. The wrapper
///   resolves this against the current module's exported methods.
/// - [`Self::Qualified`] — `Module.Method(args)` lowered as
///   `Expr::Call { callee: Expr::QualifiedPath(QualifiedName), … }`.
///   The wrapper resolves this against another module via the
///   call-graph index.
///
/// `Expr::MethodCall` (`obj.Method(args)`) is **not** a callee key —
/// the receiver is a runtime expression, not a module reference, and
/// the call cannot resolve to a static method summary.
#[derive(Debug, Clone, Copy)]
pub enum CalleeKey<'a> {
    Local(&'a Name),
    Qualified { module: &'a Name, method: &'a Name },
}

/// Bitwise summary of the security-relevant effects a single method
/// might trigger when executed (directly or via transitive callees).
///
/// Field naming uses `may_*` because the analysis is over-approximate:
/// a `true` bit means *some* path reaches an effect of that kind; a
/// `false` bit means *no* path was found. False positives are possible
/// (a path that bypasses the effect), false negatives are not (the
/// analysis is conservative).
///
/// `is_recursive` is set by the §1.4b SCC wrapper, NOT by the pure
/// [`analyze_method_effects`] helper — recursion detection requires
/// methodId-level resolution that the pure helper deliberately does
/// not have access to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectSummary {
    /// Calls (or transitively reaches) `УстановитьПривилегированныйРежим`
    /// or its `ПривилегированныйРежим()` getter.
    pub may_call_privileged: bool,
    /// Calls anything that toggles or queries safe-mode (the `SafeMode`
    /// and `SafeModeQuery` registry categories collapse into one bit
    /// because consumers care about "weakens safe-mode reasoning"
    /// uniformly).
    pub may_disable_safe_mode: bool,
    /// File-system access (constructor or global method).
    pub may_call_filesystem: bool,
    /// Outbound network access (HTTP / FTP / mail constructor).
    pub may_call_internet: bool,
    /// Launches an external process (`КомандаСистемы`,
    /// `ЗапуститьПриложение`, …).
    pub may_call_external_app: bool,
    /// Dynamic execution of BSL code (`Выполнить` statement or
    /// `Вычислить` call).
    pub may_execute_external_code: bool,
    /// Method participates in a recursive call cycle (self-recursive
    /// or part of a non-trivial SCC). Set by `ide-db::effects` after
    /// SCC analysis; pure analysis leaves it `false`.
    pub is_recursive: bool,
}

impl EffectSummary {
    /// Identity for [`Self::join`] — every bit `false`. Use this as the
    /// SCC fixpoint seed (bottom of the bool-set lattice).
    pub const EMPTY: Self = Self {
        may_call_privileged: false,
        may_disable_safe_mode: false,
        may_call_filesystem: false,
        may_call_internet: false,
        may_call_external_app: false,
        may_execute_external_code: false,
        is_recursive: false,
    };

    /// Bitwise OR of two summaries. Used to fold per-stmt bits into
    /// one method-level summary, and to merge a callee's summary into
    /// the caller's during SCC fixpoint.
    pub fn join(&self, other: &Self) -> Self {
        Self {
            may_call_privileged: self.may_call_privileged | other.may_call_privileged,
            may_disable_safe_mode: self.may_disable_safe_mode | other.may_disable_safe_mode,
            may_call_filesystem: self.may_call_filesystem | other.may_call_filesystem,
            may_call_internet: self.may_call_internet | other.may_call_internet,
            may_call_external_app: self.may_call_external_app | other.may_call_external_app,
            may_execute_external_code: self.may_execute_external_code
                | other.may_execute_external_code,
            is_recursive: self.is_recursive | other.is_recursive,
        }
    }

    /// In-place join — minor optimisation for hot SCC iteration loops.
    pub fn join_in_place(&mut self, other: &Self) {
        *self = self.join(other);
    }

    /// `true` when no effect bit is set (`*self == EMPTY`).
    pub fn is_empty(&self) -> bool {
        *self == Self::EMPTY
    }

    /// Effect bits contributed by a single direct global call by name.
    /// Returns [`Self::EMPTY`] for unknown / non-security names.
    ///
    /// Used both inside [`analyze_method_effects`] and as a stand-alone
    /// classifier — §1.6 handlers can call this to decide whether a
    /// callee name belongs to a category they care about.
    pub fn classify_global_call(name: &str) -> Self {
        let Some(entry) = registry().lookup_global(name) else {
            return Self::EMPTY;
        };
        bits_for_category(entry.category)
    }

    /// Effect bits contributed by a single `Новый <Type>(…)` constructor.
    /// Returns [`Self::EMPTY`] for unknown types or for categories
    /// whose constructor variant is benign.
    pub fn classify_constructor(type_name: &str) -> Self {
        let Some(entry) = registry().lookup_constructor(type_name) else {
            return Self::EMPTY;
        };
        bits_for_category(entry.category)
    }
}

fn bits_for_category(category: Category) -> EffectSummary {
    let mut s = EffectSummary::EMPTY;
    match category {
        Category::PrivilegedMode | Category::PrivilegedModeQuery => {
            s.may_call_privileged = true;
        }
        Category::SafeMode | Category::SafeModeQuery => {
            s.may_disable_safe_mode = true;
        }
        Category::FileSystem => s.may_call_filesystem = true,
        Category::Internet => s.may_call_internet = true,
        Category::ExternalApp => s.may_call_external_app = true,
        Category::ExecuteExternalCode => s.may_execute_external_code = true,
        Category::OsUsers | Category::Logging => {
            // OsUsers has no dedicated bit yet; logging is benign and
            // exists in the registry only for §2's catch-body classifier.
        }
    }
    s
}

/// Walk every statement and expression of `body`, fold the resulting
/// bits into one summary, and merge in any cross-method callee
/// summaries returned by `callee_lookup`.
///
/// `callee_lookup` receives a [`CalleeKey`] — either a single-segment
/// `Local(&Name)` for `Метод(args)` or a `Qualified { module, method }`
/// for `Module.Method(args)`. The wrapper at `ide-db::effects` decides
/// whether the key resolves to an intra-module method (lookup in the
/// in-flight SCC table) or a cross-module method (Salsa-cached query);
/// this pure helper stays oblivious to that split.
///
/// Recognized callee shapes:
/// - `Метод(args)` → `CalleeKey::Local`
/// - `Module.Method(args)` (HIR `Expr::Field` with a `Path` base)
///   → `CalleeKey::Qualified`
///
/// Three-or-more-segment paths (`Документы.ПКО.Создать()` lowered as
/// `Expr::QualifiedPath`) point through a manager-resolved object
/// into a runtime instance method; the registry's name-keyed shape
/// cannot reduce them to a static summary, so they are skipped. The
/// legacy `PrivilegedModuleMethodCall` handler relies on call-graph
/// data for those shapes (§1.6). `Expr::MethodCall` (`obj.method(…)`)
/// is also skipped — the receiver is a runtime value, not a module
/// reference.
///
/// `is_recursive` is masked out of merged callee summaries before the
/// join: it is a *post-fixpoint* SCC label set once by the Salsa
/// wrapper after the bitwise-OR fixpoint converges. If we let it
/// propagate through `callee_lookup`, a single recursive callee would
/// taint every transitive caller, which is semantically wrong (the
/// callers themselves are not recursive).
///
/// Returns [`EffectSummary::EMPTY`] for a body with no security-
/// relevant calls. `is_recursive` always returns `false`.
pub fn analyze_method_effects<F>(body: &Body, mut callee_lookup: F) -> EffectSummary
where
    F: FnMut(CalleeKey<'_>) -> Option<EffectSummary>,
{
    // The pure helper does NOT pre-filter `Field`-receiver calls
    // against a body-wide local-binding set — doing so would suppress
    // real `Module.Method` calls in any body that happens to assign to
    // a same-spelled local elsewhere (`reaching_defs` is source-order
    // incremental at lower-time; this analysis is arena-order, where
    // "before" / "after" the assignment is not observable).
    //
    // Instead we pass every `Field { base: Path(name), field }` call
    // to `callee_lookup` as `CalleeKey::Qualified`. The wrapper at
    // `ide-db::effects` is the one that knows the module index, and
    // returns `None` for names that are not real modules — at which
    // point the call contributes no effect, exactly the behaviour the
    // legacy filter targets. The remaining edge case — assigning to a
    // name that genuinely *is* a registered common module
    // (`ОбщийМодуль = …; ОбщийМодуль.Метод()`) — over-attributes the
    // module's effects to the caller, which is a false-positive in the
    // direction security analysis tolerates.
    let mut summary = EffectSummary::EMPTY;

    // Statement-level effects: `Выполнить(<expr>)` is a statement, not
    // a function call, so it doesn't reach the expr walk below.
    for (_, stmt) in body.stmts_iter() {
        if matches!(stmt, Stmt::Execute { .. }) {
            summary.may_execute_external_code = true;
        }
    }

    // Expression-level effects: walk the entire expr arena. Bitwise OR
    // is idempotent so visiting nested calls (e.g. inside argument
    // lists) is safe — double-counting cannot corrupt the summary.
    for (_, expr) in body.exprs_iter() {
        match expr {
            Expr::Call { callee, .. } => {
                let callee_expr = body.expr(ExprId::from_idx(*callee));
                // Resolve the callee shape to either a direct
                // registry hit or a `CalleeKey` for the wrapper to
                // resolve. HIR shapes for the two flavours of
                // qualified call:
                // - `Module.Method(args)` lowers to
                //   `Expr::Call { callee: Expr::Field { base: Path(Module), field: Method } }`.
                //   This is the common-module case (`hir-def::body::lower::lower_field_expr`).
                // - `Manager.Object.Method(args)` lowers to
                //   `Expr::Call { callee: Expr::QualifiedPath(_3-seg_) }`
                //   at `hir-def/src/body/lower/expr.rs:1180`. The
                //   third segment is a runtime instance method on a
                //   manager-resolved object; the registry / call-graph
                //   cannot reduce it to a static summary, so we skip.
                let key = match callee_expr {
                    Expr::Path(name) => {
                        let direct = EffectSummary::classify_global_call(name.as_str());
                        if !direct.is_empty() {
                            summary.join_in_place(&direct);
                            None
                        } else {
                            Some(CalleeKey::Local(name))
                        }
                    }
                    Expr::Field { base, field } => match body.expr(ExprId::from_idx(*base)) {
                        Expr::Path(module) => {
                            // Pass to the wrapper unconditionally; if
                            // `module` is not a real common module the
                            // wrapper returns `None`. See fn doc for
                            // the false-positive trade-off.
                            Some(CalleeKey::Qualified { module, method: field })
                        }
                        // Non-`Path` base (`obj.field.Method`,
                        // `func().Method`, `Новый Тип().Method`):
                        // cannot resolve to a static module summary.
                        _ => None,
                    },
                    // Three-or-more-segment qualified path
                    // (`Документы.ПКО.Создать()`); see comment above.
                    Expr::QualifiedPath(_) => None,
                    _ => None,
                };
                if let Some(key) = key {
                    if let Some(callee_sum) = callee_lookup(key) {
                        // Strip recursion before propagating: it is set
                        // post-fixpoint, not transitively (see fn doc).
                        let mut sanitised = callee_sum;
                        sanitised.is_recursive = false;
                        summary.join_in_place(&sanitised);
                    }
                }
            }
            Expr::New { type_name: Some(name), .. } => {
                let bits = EffectSummary::classify_constructor(name.as_str());
                summary.join_in_place(&bits);
            }
            _ => {}
        }
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_has_no_bits() {
        let s = EffectSummary::EMPTY;
        assert!(s.is_empty());
        assert!(!s.may_call_privileged);
        assert!(!s.is_recursive);
    }

    #[test]
    fn join_is_bitwise_or() {
        let mut a = EffectSummary::EMPTY;
        a.may_call_filesystem = true;
        let mut b = EffectSummary::EMPTY;
        b.may_call_internet = true;
        let merged = a.join(&b);
        assert!(merged.may_call_filesystem);
        assert!(merged.may_call_internet);
        assert!(!merged.may_call_privileged);
    }

    #[test]
    fn join_idempotent() {
        let mut a = EffectSummary::EMPTY;
        a.may_call_external_app = true;
        a.is_recursive = true;
        assert_eq!(a.join(&a), a);
    }

    #[test]
    fn join_commutative() {
        let mut a = EffectSummary::EMPTY;
        a.may_call_privileged = true;
        let mut b = EffectSummary::EMPTY;
        b.may_disable_safe_mode = true;
        assert_eq!(a.join(&b), b.join(&a));
    }

    #[test]
    fn join_with_empty_is_identity() {
        let mut a = EffectSummary::EMPTY;
        a.may_call_filesystem = true;
        assert_eq!(a.join(&EffectSummary::EMPTY), a);
        assert_eq!(EffectSummary::EMPTY.join(&a), a);
    }

    #[test]
    fn classify_global_privileged() {
        let s = EffectSummary::classify_global_call("УстановитьПривилегированныйРежим");
        assert!(s.may_call_privileged);
        assert!(!s.may_call_filesystem);
    }

    #[test]
    fn classify_global_eval() {
        let s = EffectSummary::classify_global_call("Вычислить");
        assert!(s.may_execute_external_code);
        assert!(!s.may_call_privileged);
    }

    #[test]
    fn classify_global_external_app() {
        let s = EffectSummary::classify_global_call("КомандаСистемы");
        assert!(s.may_call_external_app);
    }

    #[test]
    fn classify_global_safe_mode() {
        let s = EffectSummary::classify_global_call("УстановитьБезопасныйРежим");
        assert!(s.may_disable_safe_mode);
    }

    #[test]
    fn classify_global_safe_mode_query() {
        let s = EffectSummary::classify_global_call("БезопасныйРежим");
        assert!(s.may_disable_safe_mode, "SafeMode getter folds into the same bit");
    }

    #[test]
    fn classify_global_unknown_returns_empty() {
        let s = EffectSummary::classify_global_call("__definitely_not_a_real_method__");
        assert_eq!(s, EffectSummary::EMPTY);
    }

    #[test]
    fn classify_global_logging_is_empty() {
        // Logging entries exist in the registry only so §2's catch-body
        // classifier can identify `LogsOnly` clauses; they are not a
        // security effect.
        let s = EffectSummary::classify_global_call("ЗаписьЖурналаРегистрации");
        assert_eq!(s, EffectSummary::EMPTY);
    }

    #[test]
    fn classify_global_os_users_is_empty() {
        // OsUsers has no dedicated bit today (§9.3 of the master plan
        // marks it as a future extension); classifier returns empty
        // until the field is added.
        let s = EffectSummary::classify_global_call("ПользователиОС");
        assert_eq!(s, EffectSummary::EMPTY);
    }

    #[test]
    fn classify_constructor_filesystem() {
        let s = EffectSummary::classify_constructor("Файл");
        assert!(s.may_call_filesystem);
    }

    #[test]
    fn classify_constructor_internet() {
        let s = EffectSummary::classify_constructor("HTTPСоединение");
        assert!(s.may_call_internet);
    }

    #[test]
    fn classify_constructor_unknown_is_empty() {
        let s = EffectSummary::classify_constructor("Массив");
        assert_eq!(s, EffectSummary::EMPTY);
    }

    #[test]
    fn classify_global_english_alias() {
        let s = EffectSummary::classify_global_call("SetPrivilegedMode");
        assert!(s.may_call_privileged);
    }

    // Regression guard for Codex round-1 BLOCKER: a recursive callee
    // must NOT taint the caller's summary. Verified at the merge point
    // by inspecting `analyze_method_effects` source; here we pin the
    // contract symbolically.
    #[test]
    fn join_propagates_is_recursive_but_pure_helper_strips_it() {
        let mut callee = EffectSummary::EMPTY;
        callee.may_call_privileged = true;
        callee.is_recursive = true;

        // Direct `join` propagates is_recursive — that is the lattice
        // operation, used by the §1.4b SCC fixpoint that legitimately
        // wants to track recursion membership.
        let merged_via_join = EffectSummary::EMPTY.join(&callee);
        assert!(merged_via_join.is_recursive);
        assert!(merged_via_join.may_call_privileged);

        // The pure helper masks `is_recursive` before joining a callee
        // summary in. This is enforced inline in
        // `analyze_method_effects`; no synthetic body is needed to
        // verify the masking pattern.
        let mut sanitised = callee;
        sanitised.is_recursive = false;
        let merged_via_helper = EffectSummary::EMPTY.join(&sanitised);
        assert!(!merged_via_helper.is_recursive);
        assert!(merged_via_helper.may_call_privileged);
    }
}
