# ADR-01 — Type narrowing (M4 scope)

**Status:** draft / stub.
**Milestone:** M4 of the type-system rollout.
**Supersedes:** nothing.
**Related:** [`TYPE_SYSTEM.md`](../TYPE_SYSTEM.md), [`DATAFLOW.md`](../DATAFLOW.md).

## Context

M3 shipped `Ty::Union(Arc<[Ty]>)` as a canonical, `Eq`-stable
representation of "this expression could be one of N types" (e.g.
`ОписаниеТипов(...)`, XML `Composite`, JSDoc `Число, Строка`). The
union type is load-bearing on paper — XML `AttributeType::Composite`
and JSDoc comma-lists both lower into it — but at runtime every
consumer currently treats `Ty::Union` as opaque: method lookup,
field lookup, and completion all return `None` / empty when the
receiver is a union.

Narrowing is what makes unions useful. In BSL the idiomatic guard
shapes are:

```bsl
Если ТипЗнч(Х) = Тип("Массив") Тогда
    // here Х must be treated as Ty::Array
КонецЕсли

Если Х <> Неопределено Тогда
    // Ty::Union([Ty::CatalogRef.Х, Ty::Undefined]) → Ty::CatalogRef.Х
КонецЕсли

Если ТипЗнч(Х) = Тип("Строка") ИЛИ ТипЗнч(Х) = Тип("Число") Тогда
    // Х narrows to Ty::Union([String, Number]) inside the block
КонецЕсли
```

The M3 plan intentionally left narrowing out of scope because the
current `InferenceContext` walks statements linearly: a single
`var_types: FxHashMap<String, Ty>` per body, updated in-order, with
no notion of "this binding has different types on different paths".
Narrowing requires flow-sensitive inference — the `Ty` of a variable
depends on which block we're currently in, not just on the latest
assignment.

## Decision drivers

- **User-visible signal.** Narrowing is the single biggest typing
  improvement a BSL programmer would notice — it's the difference
  between "hover on `Х` inside an `Если`-block shows `Union`" and
  "hover shows the concrete branch type". Without it, the M3 union
  machinery is plumbing with no daylight.
- **Existing CFG infrastructure.** `cfg` + `dataflow` crates already
  model control flow with `Lattice` / `Transfer` / `DataflowSolver`
  (used by reaching definitions and liveness). Narrowing is another
  dataflow analysis with a custom transfer function; the framework
  is not greenfield.
- **Model mismatch with current inference.** `InferenceContext` is
  linear. Narrowing needs a merge-on-branch operator (`join`) and a
  scope that distinguishes "before `Если`" from "inside the
  `then`-branch". This is a **smoothed smaller refactor** of
  inference, not a feature toggle.

## Options considered

**A. Full flow-sensitive rewrite of `InferenceContext`.**
Replace the linear walk with a CFG-driven fixed-point solver (like
reaching-defs). Each program point carries a `FxHashMap<String, Ty>`
and narrowing is a transfer function on `Если` / `Если Тогда` /
`КонецЕсли` edges.

Pros: clean, reuses the existing dataflow solver.
Cons: large refactor; high risk of breaking the 30+ M2 / M3 behavioural
tests that rely on the linear model.

**B. Targeted overlay: keep linear inference, add a
"narrowing overlay" layer.**
Linear inference produces the base `var_types`. A second pass walks
`Если` blocks, collects type-guards from conditions, and emits
narrowed types into a shadow map keyed by `(BlockId, VarName)`.
`Semantics::type_of_expr` consults the overlay before the base map.

Pros: zero blast radius on existing inference; incremental.
Cons: cannot correctly handle nested narrowing (`Если A И B` where
both narrow the same variable), loses invalidation precision.

**C. Scope-tree narrowing via `ExprScopes`.**
`hir-def`'s `ExprScopes` already models lexical scopes. Extend it
with "narrowing facts" — per-scope (not per-statement) `Map<Name, Ty>`
attached to specific scopes (e.g. the body of an `Если`-branch).
Inference queries `(scope, name)` and merges with the base type.

Pros: uses existing scope tree; lexical model matches BSL semantics.
Cons: block-level scopes don't always correspond to what the user
reads — e.g., a variable narrowed in the `then`-branch but the user
hovers on the `Иначе`-branch should NOT see the narrowing.

## Recommendation (pending M4 decision)

Option A (full flow-sensitive rewrite) is the architecturally correct
answer, but option B (overlay) may ship faster if the narrowing
semantics can be defined narrowly (`ТипЗнч(X) = Тип(...)` only; no
`ИЛИ` composition; no nested guards). A decision is blocked on
implementation cost estimate and whether the user-visible improvement
justifies the refactor.

## Open questions

1. **Guard grammar.** Which shapes narrow?
   - Must: `ТипЗнч(Х) = Тип("Массив")`, `Х = Неопределено`, `Х <> Неопределено`, `ЗначениеЗаполнено(Х)`.
   - Maybe: `ТипЗнч(Х) = Тип("…") ИЛИ ТипЗнч(Х) = Тип("…")` (union narrowing).
   - Explicitly deferred: `Х Есть Справочник`, Cast-via-assignment.
2. **Merge semantics on `Иначе` / fall-through.** If `Х: Union(A, B)`
   and the `Если`-branch narrows to `A`, the `Иначе` branch sees
   `B` (type-difference). Do we compute `Union \ Narrowed` precisely
   or degrade to `Ty::Unknown`?
3. **Interaction with reassignment.** `Х = СоздатьМассив()` inside
   a narrowed block overrides the narrowing; does it extend outside
   the block? (BSL is dynamically typed; user intent is ambiguous.)
4. **Hover on the guard expression itself.** Do we show the *pre-narrow*
   or *post-narrow* type on the receiver of `ТипЗнч(…)`?
5. **Performance.** Flow-sensitive inference increases complexity by
   roughly a factor of the average block count per body. Do we need
   a new Salsa query key (per-body flow fact) or can we fold into
   `infer_query`?
6. **`is_assignable_to`.** ✅ Resolved in M4 Task 7: shipped in
   `hir::Type::is_assignable_to` with reflexive / `Unknown` / `Null ≤ ref` /
   union-left / union-right / `ThisObject` coercion rules. Narrowing
   awareness enters via callers that build the [`Type`] from
   `Semantics::type_of_expr` — the method itself is pure on [`Ty`].

## Acceptance criteria for M4

- Hover on a variable inside an `Если ТипЗнч(Х) = Тип("Массив") Тогда`
  block shows `Array`, not `Union(Array, String, …)`.
- `Х.Добавить()` resolves to `Ty::Undefined` (Array method) inside
  the narrowed block, `Ty::Unknown` outside.
- No regression on any M2 / M3 behavioural test when narrowing is
  disabled (feature flag or empty guard set).
- `hir::Type::is_assignable_to` lands with narrowing-aware semantics. ✅ (Task 7)

## Non-goals

- Full type-system dependent-types (refinements, index-based narrowing).
- Narrowing across function call boundaries (callers see parameters
  pre-narrowed; callees don't inherit caller-side narrowing).
- Narrowing on `Попытка` / `Исключение` boundaries — would need
  an escape analysis not yet present.
