//! Type-narrowing guard recognition — pure syntactic layer.
//!
//! Recognizes the ADR-01 MUST-grammar guard shapes inside an `Expr`
//! tree and returns a [`Guard`] describing the effect the guard has on
//! the true / false successor of a conditional. Consumed by
//! [`NarrowingAnalysis`](crate::narrow) (Task 6.2) through the
//! branch-aware [`dataflow::Transfer::transfer_edge`] hook added in
//! Task 6.0.
//!
//! # Scope (ADR-01 MUST-grammar)
//!
//! - `ТипЗнч(X) = Тип("Строка")` — narrows `X` to the string-named
//!   platform type on the true branch; `Union \ Ty` on the false
//!   branch (the latter is Task 6.3's concern).
//! - `X = Неопределено` / `X <> Неопределено` — flips between
//!   `Ty::Undefined` and its complement over the union.
//! - `ЗначениеЗаполнено(X)` — strips `Undefined` / `Null` from a union
//!   on the true branch; keeps only `Undefined` / `Null` on the false
//!   branch.
//! - Symmetric orientations — `Тип("…") = ТипЗнч(X)`,
//!   `Неопределено <> X` — are accepted verbatim (BSL's `=` is not
//!   orientation-sensitive and authors routinely flip the sides).
//!
//! # Deferred (ADR-01 Q1 — not this task)
//!
//! - `ИЛИ`-composition (`ТипЗнч(X) = Тип("Строка") ИЛИ ТипЗнч(X) = Тип("Число")`).
//! - Negation (`Не ТипЗнч(X) = Тип("…")` or `Не ЗначениеЗаполнено(X)`).
//! - Nested guards (`Если A И B Тогда`).
//! - `X Есть Справочник`.
//! - Narrowing on non-`Path(Name)` receivers (fields, indexes, qualified paths).
//!
//! Anything outside the MUST-grammar returns [`None`] — the caller is
//! expected to propagate state unchanged across the branch, which is
//! exactly the default [`dataflow::Transfer::transfer_edge`] behaviour.
//!
//! # Purity
//!
//! This module does not touch the Salsa database, [`InferenceContext`],
//! or [`TyLoweringContext`]. Guards are recognized purely from
//! `Expr` / `Literal` shape plus case-insensitive name matches on
//! builtin function names. Resolution of the `Тип("…")` string literal
//! to a concrete [`Ty`] lives in the caller (Task 6.2), so this module
//! stays trivially testable and reusable (e.g. diagnostics that only
//! need to know "this is a type guard" without caring about the
//! target [`Ty`]).
//!
//! [`InferenceContext`]: crate::InferenceContext
//! [`TyLoweringContext`]: crate::TyLoweringContext
//! [`Ty`]: hir_def::ty::Ty

use hir_def::body::Body;
use hir_def::hir::{BinaryOp, Expr, Literal};
use hir_def::Name;
use la_arena::Idx;

type ExprIdx = Idx<Expr>;

/// One recognized guard shape and the variable it constrains.
///
/// Each variant describes the guard's effect implicitly via its name;
/// the mapping `Guard → (Ty_true, Ty_false)` lives in the narrowing
/// analysis (Task 6.2) because it needs the pre-guard union to compute
/// the false-branch complement (Task 6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    /// `ТипЗнч(X) = Тип("SomeType")` (or reversed orientation).
    ///
    /// On the true branch, `X` narrows to the type named by
    /// `type_name`; on the false branch, to `Union \ Ty` if `X` was a
    /// union, else `Unknown`.
    ///
    /// `type_name` is kept as the raw string literal — the caller maps
    /// it through [`hir_def::ty::Ty::from_type_name`].
    TypeCheck { var: Name, type_name: String },

    /// `X = Неопределено` (or `Неопределено = X`).
    ///
    /// True branch: `X` is `Ty::Undefined`.
    /// False branch: `X` is `Union \ Undefined` (or `Unknown`).
    IsUndefined { var: Name },

    /// `X <> Неопределено` (or `Неопределено <> X`).
    ///
    /// Mirror of [`Guard::IsUndefined`] — branches swap.
    IsNotUndefined { var: Name },

    /// `ЗначениеЗаполнено(X)`.
    ///
    /// True branch: `X` loses `Undefined` / `Null` from its union.
    /// False branch: `X` is constrained to `Undefined` / `Null` /
    /// empty-value subset. The caller decides precision (Task 6.3).
    ValueFilled { var: Name },
}

/// Recognize an ADR-01 MUST-grammar guard at `expr`. Returns [`None`]
/// for any shape outside the grammar.
///
/// This is a pure syntactic match on `body.expr_idx(expr)` — no
/// inference context, no database. Case-insensitive comparisons use
/// [`Name::eq_ignore_case`] so Russian (`ТипЗнч`) and English
/// (`TypeOf`) spellings are treated uniformly.
pub fn recognize_guard(expr: ExprIdx, body: &Body) -> Option<Guard> {
    match body.expr_idx(expr) {
        Expr::BinaryOp { lhs, rhs, op } => match op {
            BinaryOp::Eq => recognize_eq_guard(*lhs, *rhs, body, /*negated=*/ false),
            BinaryOp::Neq => recognize_eq_guard(*lhs, *rhs, body, /*negated=*/ true),
            _ => None,
        },
        Expr::Call { callee, args } => {
            let callee_name = path_name(*callee, body)?;
            if callee_name.eq_ignore_case(&Name::new("ЗначениеЗаполнено"))
                || callee_name.eq_ignore_case(&Name::new("ValueIsFilled"))
                || callee_name.eq_ignore_case(&Name::new("ValueFilled"))
            {
                let var = single_path_arg(args, body)?;
                return Some(Guard::ValueFilled { var });
            }
            None
        }
        _ => None,
    }
}

/// Handle `=` / `<>` guards. When `negated` is true, true- and
/// false-branch semantics flip, which we express by swapping the
/// variant produced for `X = Неопределено` vs `X <> Неопределено`.
///
/// The `ТипЗнч(X) = Тип("…")` shape is *not* sensitive to `<>` in
/// ADR-01's MUST set — `Не ТипЗнч(X) = Тип("…")` is an explicit
/// non-goal, so `<>` on a type check falls through to [`None`] rather
/// than producing a "negated-type-check" variant we'd have to carry
/// through the analysis.
fn recognize_eq_guard(lhs: ExprIdx, rhs: ExprIdx, body: &Body, negated: bool) -> Option<Guard> {
    // Try each orientation: the BSL author can write either side first.
    if let Some(g) =
        try_type_check(lhs, rhs, body, negated).or_else(|| try_type_check(rhs, lhs, body, negated))
    {
        return Some(g);
    }

    if let Some(var) =
        try_undefined_compare(lhs, rhs, body).or_else(|| try_undefined_compare(rhs, lhs, body))
    {
        return Some(if negated {
            Guard::IsNotUndefined { var }
        } else {
            Guard::IsUndefined { var }
        });
    }

    None
}

/// Try to match `ТипЗнч(var) = Тип("…")` with `lhs = ТипЗнч(var)` and
/// `rhs = Тип("…")`. Returns `None` if either side's shape is wrong,
/// or if the comparison is `<>` (which is an explicit non-goal).
fn try_type_check(lhs: ExprIdx, rhs: ExprIdx, body: &Body, negated: bool) -> Option<Guard> {
    if negated {
        return None;
    }
    let var = type_of_arg(lhs, body)?;
    let type_name = type_literal_arg(rhs, body)?;
    Some(Guard::TypeCheck { var, type_name })
}

/// Return `Some(var)` if `(lhs, rhs)` is `(Path(var), Literal(Undefined))`.
fn try_undefined_compare(lhs: ExprIdx, rhs: ExprIdx, body: &Body) -> Option<Name> {
    let var = path_name(lhs, body)?;
    match body.expr_idx(rhs) {
        Expr::Literal(Literal::Undefined) => Some(var),
        _ => None,
    }
}

/// Extract the variable from `ТипЗнч(var)` / `TypeOf(var)` — accepts
/// exactly one `Path(Name)` argument. Anything else (non-`Path` arg,
/// zero or multiple args, wrong callee name) returns [`None`].
fn type_of_arg(expr: ExprIdx, body: &Body) -> Option<Name> {
    let (callee_name, args) = call_parts(expr, body)?;
    if !callee_name.eq_ignore_case(&Name::new("ТипЗнч"))
        && !callee_name.eq_ignore_case(&Name::new("TypeOf"))
    {
        return None;
    }
    single_path_arg(args, body)
}

/// Extract the string literal from `Тип("…")` / `Type("…")`. Returns
/// [`None`] if the callee is wrong, argument count is not 1, or the
/// argument is not a string literal.
fn type_literal_arg(expr: ExprIdx, body: &Body) -> Option<String> {
    let (callee_name, args) = call_parts(expr, body)?;
    if !callee_name.eq_ignore_case(&Name::new("Тип"))
        && !callee_name.eq_ignore_case(&Name::new("Type"))
    {
        return None;
    }
    if args.len() != 1 {
        return None;
    }
    match body.expr_idx(args[0]) {
        Expr::Literal(Literal::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// Return `(callee_name, args)` for a `Call { callee: Path(name), .. }`.
fn call_parts(expr: ExprIdx, body: &Body) -> Option<(Name, &[ExprIdx])> {
    match body.expr_idx(expr) {
        Expr::Call { callee, args } => {
            let name = path_name(*callee, body)?;
            Some((name, args.as_ref()))
        }
        _ => None,
    }
}

/// If `expr` is `Path(name)`, return `name`. Covers the variable-reference
/// case for guard receivers; `QualifiedPath`, `Field`, etc. are out of
/// scope for ADR-01's MUST-grammar.
fn path_name(expr: ExprIdx, body: &Body) -> Option<Name> {
    match body.expr_idx(expr) {
        Expr::Path(name) => Some(name.clone()),
        _ => None,
    }
}

/// Require a single `Path(name)` argument. Anything else (zero args,
/// multiple args, non-path arg) returns [`None`].
fn single_path_arg(args: &[ExprIdx], body: &Body) -> Option<Name> {
    if args.len() != 1 {
        return None;
    }
    path_name(args[0], body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir_def::hir::UnaryOp;

    /// Tiny builder that hand-rolls a `Body` with just enough expressions
    /// to exercise guard recognition. We never need statements or
    /// bindings for these tests — `recognize_guard` is pure on the
    /// expression arena.
    struct ExprBuilder {
        body: Body,
    }

    impl ExprBuilder {
        fn new() -> Self {
            Self { body: Body::new() }
        }

        fn alloc(&mut self, expr: Expr) -> ExprIdx {
            self.body.exprs_mut().alloc(expr)
        }

        fn path(&mut self, name: &str) -> ExprIdx {
            self.alloc(Expr::Path(Name::new(name)))
        }

        fn string_lit(&mut self, s: &str) -> ExprIdx {
            self.alloc(Expr::Literal(Literal::String(s.to_string())))
        }

        fn undefined(&mut self) -> ExprIdx {
            self.alloc(Expr::Literal(Literal::Undefined))
        }

        fn call(&mut self, callee: ExprIdx, args: Vec<ExprIdx>) -> ExprIdx {
            self.alloc(Expr::Call { callee, args: args.into_boxed_slice() })
        }

        fn bin(&mut self, lhs: ExprIdx, rhs: ExprIdx, op: BinaryOp) -> ExprIdx {
            self.alloc(Expr::BinaryOp { lhs, rhs, op })
        }
    }

    #[test]
    fn recognizes_type_check_direct() {
        // `ТипЗнч(Х) = Тип("Строка")` — canonical orientation.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let typznc_callee = b.path("ТипЗнч");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck { var: Name::new("Х"), type_name: "Строка".to_string() })
        );
    }

    #[test]
    fn recognizes_type_check_reversed() {
        // `Тип("Строка") = ТипЗнч(Х)` — flipped sides, same meaning.
        let mut b = ExprBuilder::new();
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Массив");
        let lhs = b.call(tip_callee, vec![s]);
        let x = b.path("Данные");
        let typznc_callee = b.path("ТипЗнч");
        let rhs = b.call(typznc_callee, vec![x]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck {
                var: Name::new("Данные"), type_name: "Массив".to_string()
            })
        );
    }

    #[test]
    fn recognizes_type_check_english_spelling() {
        // `TypeOf(X) = Type("String")` — English forms accepted verbatim
        // (BSL is bilingual and case-insensitive on builtin names).
        let mut b = ExprBuilder::new();
        let x = b.path("X");
        let typeof_callee = b.path("TypeOf");
        let lhs = b.call(typeof_callee, vec![x]);
        let type_callee = b.path("Type");
        let s = b.string_lit("String");
        let rhs = b.call(type_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::TypeCheck { var: Name::new("X"), type_name: "String".to_string() })
        );
    }

    #[test]
    fn recognizes_type_check_case_insensitive() {
        // Mixed case — `тИпЗнЧ` must still match, because identifier
        // comparison in BSL is fully case-insensitive.
        let mut b = ExprBuilder::new();
        let x = b.path("а");
        let typznc_callee = b.path("тИпЗнЧ");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("ТИП");
        let s = b.string_lit("Число");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert!(matches!(recognize_guard(guard, &b.body), Some(Guard::TypeCheck { .. })));
    }

    #[test]
    fn recognizes_is_undefined() {
        // `Х = Неопределено`
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.undefined();
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_undefined_reversed() {
        // `Неопределено = Х` — same guard, flipped sides.
        let mut b = ExprBuilder::new();
        let lhs = b.undefined();
        let rhs = b.path("Х");
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_not_undefined() {
        // `Х <> Неопределено`
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.undefined();
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsNotUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_is_not_undefined_reversed() {
        // `Неопределено <> Х`
        let mut b = ExprBuilder::new();
        let lhs = b.undefined();
        let rhs = b.path("Х");
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(
            recognize_guard(guard, &b.body),
            Some(Guard::IsNotUndefined { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_value_filled() {
        // `ЗначениеЗаполнено(Х)`
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![x]);

        assert_eq!(
            recognize_guard(call, &b.body),
            Some(Guard::ValueFilled { var: Name::new("Х") })
        );
    }

    #[test]
    fn recognizes_value_filled_english() {
        // `ValueIsFilled(X)` — documented English spelling.
        let mut b = ExprBuilder::new();
        let x = b.path("X");
        let callee = b.path("ValueIsFilled");
        let call = b.call(callee, vec![x]);

        assert_eq!(
            recognize_guard(call, &b.body),
            Some(Guard::ValueFilled { var: Name::new("X") })
        );
    }

    #[test]
    fn does_not_recognize_random_binary_op() {
        // `Х + 1` — arithmetic, not a guard. Must return `None` so the
        // solver leaves state unchanged.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let expr = b.bin(lhs, rhs, BinaryOp::Add);

        assert_eq!(recognize_guard(expr, &b.body), None);
    }

    #[test]
    fn does_not_recognize_negated_type_check() {
        // `ТипЗнч(Х) <> Тип("Строка")` is an explicit non-goal of
        // ADR-01 (see module doc). Falling through to `None` is the
        // contract — `Не` / `<>` on a type check does *not* synthesize
        // an inverted TypeCheck guard.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let typznc_callee = b.path("ТипЗнч");
        let lhs = b.call(typznc_callee, vec![x]);
        let tip_callee = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tip_callee, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Neq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_unary_not() {
        // `Не ЗначениеЗаполнено(Х)` — negation is deferred (ADR-01 Q1).
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![x]);
        let negated = b.alloc(Expr::UnaryOp { expr: call, op: UnaryOp::Not });

        assert_eq!(recognize_guard(negated, &b.body), None);
    }

    #[test]
    fn does_not_recognize_or_composition() {
        // `ТипЗнч(Х) = Тип("Строка") ИЛИ ТипЗнч(Х) = Тип("Число")` —
        // explicitly deferred. The top-level expression is BinaryOp::Or
        // so `recognize_guard` falls through to `None`.
        let mut b = ExprBuilder::new();

        let build_tc = |b: &mut ExprBuilder, type_lit: &str| {
            let x = b.path("Х");
            let tz = b.path("ТипЗнч");
            let lhs = b.call(tz, vec![x]);
            let tp = b.path("Тип");
            let s = b.string_lit(type_lit);
            let rhs = b.call(tp, vec![s]);
            b.bin(lhs, rhs, BinaryOp::Eq)
        };

        let left = build_tc(&mut b, "Строка");
        let right = build_tc(&mut b, "Число");
        let or_expr = b.bin(left, right, BinaryOp::Or);

        assert_eq!(recognize_guard(or_expr, &b.body), None);
    }

    #[test]
    fn does_not_recognize_type_check_with_non_string_arg() {
        // `ТипЗнч(Х) = Тип(СомеПеременная)` — the `Тип(…)` argument
        // must be a string literal. A dynamic argument is out of scope.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let tz = b.path("ТипЗнч");
        let lhs = b.call(tz, vec![x]);
        let tp = b.path("Тип");
        let dynamic_arg = b.path("СомеПеременная");
        let rhs = b.call(tp, vec![dynamic_arg]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_multi_arg_type_of() {
        // `ТипЗнч(Х, Y) = Тип("Строка")` — excess args.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let y = b.path("Y");
        let tz = b.path("ТипЗнч");
        let lhs = b.call(tz, vec![x, y]);
        let tp = b.path("Тип");
        let s = b.string_lit("Строка");
        let rhs = b.call(tp, vec![s]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_with_no_args() {
        // `ЗначениеЗаполнено()` — wrong arity.
        let mut b = ExprBuilder::new();
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_with_literal_arg() {
        // `ЗначениеЗаполнено("hi")` — only simple variable receivers
        // narrow, literals do not (ADR-01 scope).
        let mut b = ExprBuilder::new();
        let lit = b.string_lit("hi");
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![lit]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }

    #[test]
    fn does_not_recognize_field_receiver() {
        // `Объект.Поле = Неопределено` — narrowing on field receivers
        // is deferred (would need alias analysis). Must return `None`.
        let mut b = ExprBuilder::new();
        let obj = b.path("Объект");
        let field = b.alloc(Expr::Field { base: obj, field: Name::new("Поле") });
        let rhs = b.undefined();
        let guard = b.bin(field, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_missing_literal() {
        // `Х = 1` — `1` is not `Неопределено`, so no IsUndefined guard.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.alloc(Expr::Literal(Literal::Number(1.0.try_into().unwrap())));
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_path_eq_path() {
        // `Х = Y` — both sides are identifiers. Equality between two
        // variables is not a narrowing guard under ADR-01 (no known
        // type info to transfer). Pin the `None` so a future sloppy
        // refactor of `try_undefined_compare` can't accidentally
        // promote `Path == Path` into an IsUndefined-shaped guard.
        let mut b = ExprBuilder::new();
        let lhs = b.path("Х");
        let rhs = b.path("Y");
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_type_check_on_type_check() {
        // `ТипЗнч(Х) = ТипЗнч(Y)` — both sides look like `ТипЗнч(…)`;
        // neither side is a `Тип("…")` string literal. Must return
        // `None` — cross-variable type-equality is not an ADR-01
        // guard and narrowing both variables is out of scope.
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let tz1 = b.path("ТипЗнч");
        let lhs = b.call(tz1, vec![x]);
        let y = b.path("Y");
        let tz2 = b.path("ТипЗнч");
        let rhs = b.call(tz2, vec![y]);
        let guard = b.bin(lhs, rhs, BinaryOp::Eq);

        assert_eq!(recognize_guard(guard, &b.body), None);
    }

    #[test]
    fn does_not_recognize_ternary() {
        // `?(Х = Неопределено, А, Б)` — `recognize_guard` is called
        // on the `Ternary` node itself. It MUST NOT peek through and
        // return the guard hidden in `condition`: the solver asks
        // "what does THIS expression narrow?", not "what does its
        // condition narrow?". The latter is Task 6.2's job (the CFG
        // builder emits a `Conditional` vertex whose `condition`
        // expr-id is what `recognize_guard` gets called on — never
        // the Ternary).
        let mut b = ExprBuilder::new();
        let x = b.path("Х");
        let und = b.undefined();
        let condition = b.bin(x, und, BinaryOp::Eq);
        let then_expr = b.path("А");
        let else_expr = b.path("Б");
        let ternary = b.alloc(Expr::Ternary { condition, then_expr, else_expr });

        assert_eq!(recognize_guard(ternary, &b.body), None);
    }

    #[test]
    fn does_not_recognize_value_filled_on_field() {
        // `ЗначениеЗаполнено(Объект.Поле)` — the receiver is a
        // `Field`, not a `Path`. Field narrowing is deferred (it
        // needs alias analysis that ADR-01 leaves out of scope).
        // `single_path_arg` rejects via `path_name`, but pin the
        // reject path with a dedicated test so it can't silently
        // regress.
        let mut b = ExprBuilder::new();
        let obj = b.path("Объект");
        let field = b.alloc(Expr::Field { base: obj, field: Name::new("Поле") });
        let callee = b.path("ЗначениеЗаполнено");
        let call = b.call(callee, vec![field]);

        assert_eq!(recognize_guard(call, &b.body), None);
    }
}
