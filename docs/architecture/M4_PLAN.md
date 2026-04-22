# M4 Plan — Type System Completion

**Status:** approved 2026-04-22 after Codex pair-review.
**Milestone:** M4 of the type-system rollout.
**Supersedes:** the "Оставлено для M4" section of [`TYPE_SYSTEM.md`](TYPE_SYSTEM.md) — this document is the concrete task breakdown.
**Related:** [`adr/ADR-01-narrowing.md`](adr/ADR-01-narrowing.md).

## Dependency graph

```
Task 1: InferenceDiagnostic → ide-diagnostics channel  ─┐
Task 4: AttributeType → TypeRef consumer migration     ─┘  week 1 (parallel)
Task 2: Register FieldLookup (per-part variants)       ─┐
Task 2b: ExchangePlan / ChartOfAccounts MetadataKind   ─┤  week 2
Task 3: Predefined items / enum values on ObjectManager ┘
Task 5: Ty::ThisObject { owner } + coercion            ─┐
Task 6.0: branch-aware Transfer trait in dataflow      ─┘  week 3
Task 6: Narrowing (ADR-01 Option A)                    ──  weeks 4–6
Task 7: hir::Type::is_assignable_to                 ✅  week 7
```

Total: ~7 weeks.

## Tasks

### Task 1 — InferenceDiagnostic → ide-diagnostics channel (M, 2–3 days)

**Gap:** `InferenceDiagnostic::{UnresolvedMethodCall, MismatchedArgCount, TypeMismatch}` is already populated by `hir-ty`, but `ide-diagnostics` has no collector that reads them. Invariant #6 is 🟡 because the channel itself is missing, not because variants are missing.

**Work:**
1. New `crates/ide-diagnostics/src/hir_inference_diagnostics.rs` — sibling of `hir_dispatch.rs`.
2. `ExprId → TextRange` via `BodySourceMap` (pattern already used elsewhere in `ide-diagnostics`).
3. Wire the 3 existing variants (`UnresolvedMethodCall`, `MismatchedArgCount`, `TypeMismatch`) as `BSL-TY-*` codes.
4. Add `InferenceDiagnostic::UnresolvedField { expr, receiver_ty, field_name }` — emitted from `infer_expr::Expr::Field` when `FieldLookup::lookup_field` returns `None` **and** receiver is not `Unknown`/`Union`.
5. Handler `unresolved_field.rs` + `BSL-TY-UNRESOLVED-FIELD` (warn).

**Closes:** invariant #6 🟡 → ✅.

### Task 2 — Register FieldLookup, per-part variants (M, 2–3 days)

**Decision (Codex):** generic `RegisterPartRef { parent, part }` rejected — `register.rs` storage already diverges (`dimensions()/resources()/attributes()` have different element types), and `Движения` lives elsewhere entirely. Per-part variants age better.

**Work:**
1. New `MetadataKind` variants: `RegisterDimension { parent: MdoType }`, `RegisterResource { parent }`, `RegisterAttribute { parent }`. `parent` is the register flavour (`InformationRegister`, `AccumulationRegister`, `AccountingRegister`, `CalculationRegister`).
2. `field_lookup::lookup_on_register` — reads `Configuration.registers`, dispatches into the 3 parts.
3. `.Движения.ДобавитьРасход()` deferred to a follow-up PR after clarifying storage shape in `bsl-metadata`.

**Regression:** new `infer_register_fields.rs`.

### Task 2b — ExchangePlan / ChartOfAccounts MetadataKind (S, ½ day)

**Gap:** `bsl-metadata` already has ExchangePlan/ChartOfAccounts types (7 files), but `MetadataKind` has no variants — "планы" from the TYPE_SYSTEM.md M4 bullet is literally stale.

**Work:**
1. Add `ExchangePlanRef`, `ExchangePlanObject`, `ChartOfAccountsRef`, `ChartOfAccountsObject` to `MetadataKind`.
2. Extend `mdo_type_for_kind` in `field_lookup` + `type_facade` + `metadata_kind_from_prefix` in `hir-ty::lower`.
3. Existing MDO attribute lookup picks them up automatically (same shape as Catalog/Document).

### Task 3 — Predefined items / enum values on ObjectManager (M, 2 days)

**Gap:** `Перечисления.Состояния.Активен`, `Справочники.Валюты.Доллар` currently resolve to `Ty::Unknown`.

**Work:**
1. New `hir_ty::manager_lookup::lookup_predefined` — `(Ty::ObjectManager, member) → Option<Ty::MetadataRef>`.
2. Reads `mdo.enum_values` for `Enum` flavour, `mdo.predefined_items` for `Catalog`/`ChartOf*`.
3. `Expr::Field` on `Ty::ObjectManager` branch: second arm after the existing (trivial) fall-through.

### Task 4 — AttributeType consumer migration (S, 1 day)

**Work:** mechanical sweep of ~19 direct `bsl_metadata::AttributeType` consumers → `TypeRef`. Whitelist in `scripts/check-invariants.sh`: `bsl-metadata/**` + `hir-def/src/type_ref.rs` (the bridge). All semantic consumers go through `TyLoweringContext`.

### Task 5 — Ty::ThisObject { owner } + coercion (M, 2 days)

**Decision (Codex):** keep the dedicated `Ty::ThisObject` variant from the original TYPE_SYSTEM.md design. Mapping `ЭтотОбъект` directly to `Ty::MetadataRef { CatalogObject, … }` loses provenance — `BodyDiagnostic::RedundantAccessToObject` already wants to know "this is explicitly `ЭтотОбъект`", and future rename/refactor features will too.

**Work:**
1. New `Ty::ThisObject { owner: (MdoType, Name) }`.
2. `Resolver::resolve_this_object` — reads module kind (already known in hir-def).
3. `Expr::Path("ЭтотОбъект" | "ThisObject")` → resolved via builtin.
4. FieldLookup/MethodLookup: single-match coercion to `MetadataRef { *Object, name }` at the entry of each adapter. Downstream logic unchanged.

### Task 6.0 — Branch-aware Transfer trait in dataflow (S, 1 day, prerequisite for Task 6)

**Gap:** `dataflow::Transfer::transfer_block(block_idx, in_state)` is branch-blind. Narrowing needs the transfer to see whether the edge into the successor came from the `True` or `False` side of a conditional.

**Work:**
1. Extend `Transfer` with `transfer_edge(edge_kind: EdgeKind, in_state) → L` with default impl = identity.
2. Reaching-defs and liveness are bit-vector lattices — edge-kind irrelevant, default impl works. Zero-regression migration.
3. Dataflow README updated to document the new hook.

**Splits the risk off Task 6's estimate — done standalone before Task 6 begins.**

### Task 6 — Narrowing (ADR-01 Option A, L, 2–3 weeks after 6.0)

**Decision (Codex-confirmed):** Option A (full CFG-driven fixed-point) over Option B (overlay). Option B doesn't handle nested guards per ADR-01's own analysis.

**Scope (ADR-01 MUST-grammar only):**
- `ТипЗнч(Х) = Тип("…")` — narrows `X` to the specified type.
- `Х = Неопределено` / `Х <> Неопределено` — narrows via `Ty::Undefined`.
- `ЗначениеЗаполнено(Х)` — strips `Undefined`/`Null` from unions.
- `ИЛИ`-composition: **deferred** (ADR-01 Q1).

**Subtasks:**
1. `hir-ty::narrow::Guard` — pure `Expr → Option<Guard>`.
2. `hir-ty::narrow::NarrowingAnalysis: Lattice<Name → Ty>` on top of existing `cfg` + `dataflow` (uses 6.0 branch-aware transfer).
3. Else-branch (ADR-01 Q2): `Union \ Narrowed` via smart-constructor; non-`Union` receiver → `Unknown` on false-branch.
4. Reassignment inside narrowed block (Q3): local to the block, doesn't leak.
5. Hover on guard-receiver (Q4): **pre-narrow** type.
6. Salsa (Q5): separate `narrow_query(file_id, owner)`; merged at `Semantics::type_of_expr` time.
7. Feature-flag `type_narrowing` in `bsl-analyzer.toml` for rollback.

**Acceptance criteria:** verbatim from [`ADR-01 § Acceptance criteria for M4`](adr/ADR-01-narrowing.md#acceptance-criteria-for-m4).

**Creep risks (Codex):** branch-sensitive solver plumbing; precise `Union \ Narrowed` edge cases; reassignment; CFG TODOs around `break`/`continue`/`goto`.

### Task 7 — hir::Type::is_assignable_to (S, 1 day after Task 6)

**Rules:**
- `A ≤ A`, `A ≤ Unknown`, `Null ≤ ref-type`.
- `A ≤ Union(…, A, …)`.
- `Union(A, B) ≤ T  ↔  A ≤ T ∧ B ≤ T`.
- Narrowing-aware: consult `Semantics::type_of_expr` for the actual narrowed type at the expression, not just the declared type.

**Closes:** Codex Q4 MEDIUM from M3.

## Milestones

| Milestone | Week | Deliverables |
|---|---|---|
| **M4.1** | 2 | Diagnostics channel ✅, invariant #6 ✅, AttributeType sweep ✅ |
| **M4.2** | 3 | FieldLookup covers all Configuration (+registers, +ExchangePlan, +ChartOfAccounts, +predefined items); ThisObject ✅ |
| **M4.3** | 7 | Narrowing ✅ + is_assignable_to ✅ — type system done |

## Out of scope (deferred past M4)

- `Движения.ДобавитьРасход()` — register record-set method tables.
- `ИЛИ`-composition in narrowing guards.
- Narrowing across function call boundaries.
- `Попытка`/`Исключение` narrowing (needs escape analysis).
- Full type-system dependent types (refinements).
