use bsl_platform::security::{registry, Category};
use hir_def::{
    body::Body,
    hir::{Expr, Stmt},
    ExprId, IdConversion, Name,
};
use stdx::case::CaseExt;

#[derive(Debug, Clone, Copy)]
pub enum CalleeKey<'a> {
    Local(&'a Name),
    Qualified { module: &'a Name, method: &'a Name },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectSummary {
    pub may_call_privileged: bool,
    pub may_disable_safe_mode: bool,
    pub may_call_filesystem: bool,
    pub may_call_internet: bool,
    pub may_call_external_app: bool,
    pub may_execute_external_code: bool,
    pub is_recursive: bool,
}

impl EffectSummary {
    pub const EMPTY: Self = Self {
        may_call_privileged: false,
        may_disable_safe_mode: false,
        may_call_filesystem: false,
        may_call_internet: false,
        may_call_external_app: false,
        may_execute_external_code: false,
        is_recursive: false,
    };

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

    pub fn join_in_place(&mut self, other: &Self) {
        *self = self.join(other);
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::EMPTY
    }

    pub fn classify_global_call(name: &str) -> Self {
        classify_global_call_lc(&name.fold_lower())
    }

    pub fn classify_constructor(type_name: &str) -> Self {
        classify_constructor_lc(&type_name.fold_lower())
    }
}

fn classify_global_call_lc(lc_name: &str) -> EffectSummary {
    let Some(entry) = registry().lookup_global_lc(lc_name) else {
        return EffectSummary::EMPTY;
    };
    bits_for_category(entry.category)
}

fn classify_constructor_lc(lc_type_name: &str) -> EffectSummary {
    let Some(entry) = registry().lookup_constructor_lc(lc_type_name) else {
        return EffectSummary::EMPTY;
    };
    bits_for_category(entry.category)
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
        Category::OsUsers | Category::Logging | Category::Transaction => {}
    }
    s
}

pub fn analyze_method_effects<F>(body: &Body, mut callee_lookup: F) -> EffectSummary
where
    F: FnMut(CalleeKey<'_>) -> Option<EffectSummary>,
{
    let mut summary = EffectSummary::EMPTY;

    for (_, stmt) in body.stmts_iter() {
        if matches!(stmt, Stmt::Execute { .. }) {
            summary.may_execute_external_code = true;
        }
    }

    for (_, expr) in body.exprs_iter() {
        match expr {
            Expr::Call { callee, .. } => {
                let callee_expr = body.expr(ExprId::from_idx(*callee));
                let key = match callee_expr {
                    Expr::Path(name) => {
                        let lc_name = name.as_str().fold_lower();
                        let direct = classify_global_call_lc(&lc_name);
                        if !direct.is_empty() {
                            summary.join_in_place(&direct);
                            None
                        } else {
                            Some(CalleeKey::Local(name))
                        }
                    }
                    Expr::Field { base, field } => match body.expr(ExprId::from_idx(*base)) {
                        Expr::Path(module) => Some(CalleeKey::Qualified { module, method: field }),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(key) = key {
                    if let Some(callee_sum) = callee_lookup(key) {
                        let mut sanitised = callee_sum;
                        sanitised.is_recursive = false;
                        summary.join_in_place(&sanitised);
                    }
                }
            }
            Expr::New { type_name: Some(name), .. } => {
                let lc_name = name.as_str().fold_lower();
                let bits = classify_constructor_lc(&lc_name);
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
        let s = EffectSummary::classify_global_call("ЗаписьЖурналаРегистрации");
        assert_eq!(s, EffectSummary::EMPTY);
    }

    #[test]
    fn classify_global_os_users_is_empty() {
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

    #[test]
    fn join_propagates_is_recursive_but_pure_helper_strips_it() {
        let mut callee = EffectSummary::EMPTY;
        callee.may_call_privileged = true;
        callee.is_recursive = true;

        let merged_via_join = EffectSummary::EMPTY.join(&callee);
        assert!(merged_via_join.is_recursive);
        assert!(merged_via_join.may_call_privileged);

        let mut sanitised = callee;
        sanitised.is_recursive = false;
        let merged_via_helper = EffectSummary::EMPTY.join(&sanitised);
        assert!(!merged_via_helper.is_recursive);
        assert!(merged_via_helper.may_call_privileged);
    }
}
