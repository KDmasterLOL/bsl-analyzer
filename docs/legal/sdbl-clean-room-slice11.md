# SDBL Slice 11 — Clean-Room Attestation

**Status:** complete (2026-04-26).

This document attests the clean-room authorship of the Slice 11
material of the SDBL parser — the **clauses-after-FROM family**
(WHERE / GROUP BY / HAVING / ORDER BY / AUTOORDER / TOTALS BY /
FOR UPDATE / INDEX BY) plus the two clause-tail dispatchers and
the `is_clause_keyword` predicate — per the staged migration
plan in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md).

## Scope

The paths claimed as clean-room Slice 11 authorship are:

- 12 functions in
  `crates/parser/src/grammar/sdbl/select.rs` under the new
  `CLEAN-ROOM Slice 11 — clauses after FROM` banner:

  - `select_tail_clauses` — post-`query` AUTOORDER / ORDER BY /
    TOTALS BY tail-clause loop (any-order acceptance, each
    clause at most once per loop invocation);
  - `query_body_clauses` — body-clause dispatcher inside `query`
    (FROM → WHERE → GROUP → HAVING → FOR UPDATE → INDEX BY →
    ORDER BY ordering);
  - `where_clause` — `(WHERE|ГДЕ) logical-expression`;
  - `is_clause_keyword` — `pub(super) fn(&Parser) -> bool`
    predicate for the union of clauses-after-FROM starters
    plus SELECT-prefix starters plus `ON/ПО` plus delegation
    to Slice 9 `is_join_keyword`;
  - `group_by_clause` — `(GROUP|СГРУППИРОВАТЬ) (BY|ПО)
    expression (',' expression)*`;
  - `order_by_clause` — `(ORDER|УПОРЯДОЧИТЬ) (BY|ПО)
    order-item (',' order-item)*`;
  - `order_by_item` — `expression [ASC|DESC|ВОЗР|УБЫВ]
    [HIERARCHY|ИЕРАРХИЯ]` helper (no per-item wrapper);
  - `having_clause` — `(HAVING|ИМЕЮЩИЕ) expression` (calls
    `expression(p)`, NOT `logical_expression(p)` — preserved
    pre-refactor entry-point asymmetry);
  - `for_update_clause` — `(FOR|ДЛЯ) (UPDATE|ИЗМЕНЕНИЯ)
    [mdo-ref]` with greedy MDO chain;
  - `index_by_clause` — `(INDEX|ИНДЕКСИРОВАТЬ) (BY|ПО)
    expression (',' expression)*`;
  - `autoorder_clause` — `AUTOORDER|АВТОУПОРЯДОЧИВАНИЕ` bare
    keyword;
  - `totals_by_clause` — `(TOTALS|ИТОГИ) totals-aggregate-list?
    (BY|ПО) totals-group-list` (narrowed flat-list shape per
    Slice 11 plan codex Round-1 finding 3; structured
    ONLY/HIERARCHY-in-TOTALS/PERIODS modifier promotion
    deferred to Slice 12).

- The clean-room banner block at the top of the same file's
  Slice 11 section (replacing the previous `LEGACY (Slices 5,
  11 pending)` banner; the residual block now reads
  `LEGACY (Slice 5 + SELECT limitation helpers pending)` and
  contains `virtual_table_args_legacy` plus
  `is_identifier_token` / `is_limitation_keyword` /
  `limitations` / `top_clause`).

- The §WHERE / §GROUP BY / §HAVING / §ORDER BY / §AUTOORDER /
  §TOTALS BY / §FOR UPDATE / §INDEX BY full-body sections, the
  §IDE-recovery allowances block (4 entries), the §ITS coverage
  verification table extension, and the §Non-consultation
  statement (Slice 11 reaffirmation) of
  `docs/legal/sdbl-select-mini-spec.md` (added in C0a as
  extensions of the previously-authored mini-spec; ITS
  verification rows filled in C2 from direct ITS dump reads).

- The 14 Bucket-A gap-test functions in
  `crates/parser/tests/sdbl_parser_tests.rs` added in C0b plus
  the spec-driven acceptance tests in
  `crates/parser/tests/sdbl_slice11_clauses.rs` added in C3.

**8 NodeKinds preserved bit-for-bit through the rewrite** (no
variant rename, no addition, no removal, no reorder in
`crates/syntax/src/syntax_kind.rs`):

`SdblWhereClause`, `SdblGroupClause`, `SdblOrderClause`,
`SdblHavingClause`, `SdblForUpdate`, `SdblIndexBy`,
`SdblAutoorder`, `SdblTotalsBy`.

**Function → NodeKind attribution map:**

| Function | Emits |
|---|---|
| `select_tail_clauses` | (dispatcher, no NodeKind — delegates to `autoorder_clause` / `order_by_clause` / `totals_by_clause`) |
| `query_body_clauses` | (dispatcher, no NodeKind — delegates to `from_clause` / `where_clause` / `group_by_clause` / `having_clause` / `for_update_clause` / `index_by_clause` / `order_by_clause`) |
| `where_clause` | `SdblWhereClause` |
| `is_clause_keyword` | (predicate, no NodeKind) |
| `group_by_clause` | `SdblGroupClause` |
| `order_by_clause` | `SdblOrderClause` |
| `order_by_item` | (helper, no NodeKind — emits inside `SdblOrderClause`) |
| `having_clause` | `SdblHavingClause` |
| `for_update_clause` | `SdblForUpdate` |
| `index_by_clause` | `SdblIndexBy` |
| `autoorder_clause` | `SdblAutoorder` |
| `totals_by_clause` | `SdblTotalsBy` |

**Per-function provenance Tier classification** (per Slice 9
precedent — A1 = ITS canonical listing, A2 = ITS prose-note,
B = lexer Slice 2 attested keyword pair, C = SELECT mini-spec,
D = local IDE-recovery allowance):

| # | Function | Tier source map |
|---|---|---|
| 1 | `select_tail_clauses` | C (mini-spec §SELECT query / §AUTOORDER / §ORDER BY / §TOTALS BY) + A1 (ITS pubqlang/16, /17, /27, /39) + D (any-order looping local convention) |
| 2 | `query_body_clauses` | C (mini-spec §SELECT query / §clause-tail dispatcher) + A1 (ITS pubqlang/12 §Структура запроса) |
| 3 | `where_clause` | A1 (ITS pubqlang/22 §Условие отбора + chapter 23 LIKE+WHERE + chapter 24 WHERE+parameters) + B (KwWhere/ГДЕ Slice 2-attested) + C (mini-spec §WHERE) |
| 4 | `is_clause_keyword` | C (mini-spec §SELECT query — union of clauses-after-FROM starters) + B (every keyword pair Slice 2-attested or Slice 2 LEGACY-attested) |
| 5 | `group_by_clause` | A1 (ITS pubqlang/34 §Группировка результата запроса + /35 §Расчет агрегатов) + B (KwGroup/СГРУППИРОВАТЬ + KwOnOrBy/BY/ПО) + C (mini-spec §GROUP BY) |
| 6 | `order_by_clause` | A1 (ITS pubqlang/16 §Сортировка результата запроса + /17 §Сортировка по реквизитам) + B (KwOrder/УПОРЯДОЧИТЬ Slice 2-attested) + C (mini-spec §ORDER BY) |
| 7 | `order_by_item` | A1 (ITS pubqlang/16 §Сортировка — ASC/DESC modifier per `chapter_016.html:37, 49, 63, 64`) + **A2 (ITS pubqlang/27 §Иерархическая упорядоченная выборка — HIERARCHY/ИЕРАРХИЯ modifier per `chapter_027.html:39, 51` — `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ`; C2 MANDATORY FIX)** + B (KwAsc/ВОЗР, KwDesc/УБЫВ, KwHierarchy/ИЕРАРХИЯ Slice 2 LEGACY-attested) |
| 8 | `having_clause` | A1 (ITS pubqlang/35 §Условие на агрегаты — `chapter_035.html:49`) + B (KwHaving/ИМЕЮЩИЕ Slice 2-attested) + C (mini-spec §HAVING) |
| 9 | `for_update_clause` | D (local IDE-recovery allowance — verified-no in dumped ITS chapters 16–39 by direct `rg`; lexer Slice 2 LEGACY KwFor + KwUpdate provide bilingual support) + C (mini-spec §FOR UPDATE) |
| 10 | `index_by_clause` | D (local IDE-recovery allowance — verified-no in dumped ITS chapters 16–39 by direct `rg`; lexer Slice 2 LEGACY KwIndex + KwOnOrBy provide bilingual support) + C (mini-spec §INDEX BY) |
| 11 | `autoorder_clause` | A1 (ITS pubqlang/17 §АВТОУПОРЯДОЧИВАНИЕ canonical example at `chapter_017.html:17, 32, 52`) + B (KwAutoOrder Slice 2 LEGACY-attested) + C (mini-spec §AUTOORDER) |
| 12 | `totals_by_clause` | A1 (ITS pubqlang/39 §Расчет общих итогов canonical `ИТОГИ ПО ОБЩИЕ` at `chapter_039.html:13, 25, 29, 48, 49, 51`) + B (KwTotals/ИТОГИ Slice 2-attested + KwOverall/ОБЩИЕ Slice 2 LEGACY-attested) + C (mini-spec §TOTALS BY) + D (parser-side flat-list shape; structured-modifier promotion deferred to Slice 12) |

**Child-attachment invariants** carried by Slice 11 that
downstream consumers depend on:

1. `SdblWhereClause` direct child = single condition expression
   node (one of 9 expression NodeKinds); consumer at
   `crates/sdbl-hir/src/lower/clauses.rs:28-41` reads the first
   matching kind.

2. `SdblWhereClause` KW_OR token reachability via recursive walk
   excluding subqueries; consumer at `clauses.rs:170-192`
   `collect_or_tokens_excluding_subqueries`. KW_OR is NOT a
   direct token child of `SdblWhereClause` — it sits inside the
   direct `SdblLogicalOrExpr` wrapper, and the recursive walk
   reaches it through that wrapper while skipping
   `SDBL_SUBQUERY` / `SDBL_SUBQUERY_EXPR` / `SDBL_SELECT_QUERY`
   descendants.

3. `SdblGroupClause` collects multiple direct expression-node
   children (one per group-item); consumer at `clauses.rs:74-82`
   filters direct children for one of 3 expression NodeKinds.
   No per-item wrapper.

4. `SdblOrderClause` interleaves expression nodes and ASC/DESC
   IDENT tokens (and post-Slice-11-C2: HIERARCHY/ИЕРАРХИЯ IDENT
   token) as flat siblings (no `SdblOrderByItem` wrapper);
   consumer at `clauses.rs:114-156` walks
   `children_with_tokens()` alternately picking expression-node
   children and IDENT direction tokens.

5. `SdblHavingClause` shape parallel to `SdblWhereClause` —
   single direct expression-node child. HIR reader is Slice 13
   territory (`hir.having = None` at
   `crates/sdbl-hir/src/lower/mod.rs:349`); Slice 11 must emit a
   shape that Slice 13 can read without parser changes.

6. `SdblTotalsBy` direct children: pre-BY aggregate-expression
   nodes + BY token + post-BY group-expression nodes (flat
   layout). OVERALL/ОБЩИЕ falls through `is_expression_start` as
   a bare Ident → consumed as `SdblColumnRef` by the post-BY
   `expression(p)` call. Slice 13 owns semantic interpretation.

7. `SdblForUpdate` direct children: FOR token, UPDATE token,
   optional MDO chain (Dot Ident token pairs at flat token
   level — no `SdblColumnRef` / `SdblTableRef` wrapper for the
   chain).

8. `SdblIndexBy` direct children: multiple expression nodes
   (parallel to `SdblGroupClause`).

9. `SdblAutoorder` is a standalone keyword node — direct
   children: just the AUTOORDER token.

10. `is_clause_keyword(p)` is `pub(super) fn` consumed
    cross-slice by Slice 7 (alias scan), Slice 8 (source /
    data-source scan), Slice 9 (JOIN delegation via
    `is_join_keyword`), Slice 10b (`column_or_function`
    clause-keyword recovery fix), Slice 11 (for_update_clause
    MDO-chain guard, totals_by_clause pre-BY loop guard). The
    keyword set IS the cross-slice clauses-after-FROM contract;
    adding/removing entries would break consumer call sites.

**AST-shape invariants** (operational contracts beyond
NodeKind identity):

1. `query_body_clauses` allows ORDER BY at the body-tail
   position AND `select_tail_clauses` ALSO accepts ORDER BY at
   the post-`query` tail position. Both accept points are
   reachable in the same query — preserved pre-refactor
   behaviour.

2. `select_tail_clauses` accepts AUTOORDER, ORDER, TOTALS in
   any order, each at most once per loop invocation (the three
   independent flags `parsed_autoorder` / `parsed_order_by` /
   `parsed_totals_by` gate the accepted starters).

3. `for_update_clause` does NOT mandate the optional MDO chain
   to terminate at `is_clause_keyword` via the inner `Dot Ident`
   loop — the OUTER guard
   `p.at(Ident) && !is_clause_keyword(p)` is the only protection
   against consuming clause keywords. Greediness preserved.

4. `totals_by_clause` pre-BY expression loop has a clause-
   keyword guard — without it, `ИТОГИ ИЗ T` would consume `ИЗ`
   as a pre-BY aggregate-expression.

5. Most clause functions return early on missing required
   keyword without consuming the leading keyword as ERROR:
   - `group_by_clause` missing-BY: emits bare `SdblGroupClause`
     containing only the leading keyword;
   - `order_by_clause` missing-BY: same shape with
     `SdblOrderClause`;
   - `index_by_clause` missing-BY: same shape with
     `SdblIndexBy`;
   - `totals_by_clause` missing-BY: emits `SdblTotalsBy`
     containing the leading TOTALS keyword PLUS any pre-BY
     aggregate expressions already consumed (TOTALS variant —
     pre-BY loop runs FIRST).

6. `order_by_item` does NOT emit a per-item wrapper despite the
   function name — no `m.start()` / `m.complete()` call. The
   expression node and modifier IDENT tokens end up as flat
   siblings of the parent `SdblOrderClause`.

7. `having_clause` calls `expression(p)`, NOT
   `logical_expression(p)`. Both entry points wrap their result
   in `SdblLogicalOrExpr` per Slice 10a §AST-shape #1, so the
   consumer-side filter at `clauses.rs:28-41` matches both
   shapes via the `SDBL_LOGICAL_OR_EXPR` arm.

8. `autoorder_clause` is one statement long — `eat_sdbl_keyword`
   + `m.complete`. No expression-children pickup.

9. `is_clause_keyword` includes `JOIN` family via delegation to
   `is_join_keyword` — Slice 9 attestation contract.

10. `select_tail_clauses` skip_trivia BEFORE each keyword
    check — without this, trailing whitespace would confuse the
    `at_sdbl_keyword` lookahead.

## Sources consulted

The clean-room authorship of the 12 functions, the C0a-extended
mini-spec sections, and the C3 acceptance suite was based on
direct readings of:

- ITS pubqlang chapter regions, accessed via the local dump at
  `<ITS pubqlang dump>/html/`:
  - chapter 16 (`chapter_016.html:19, 31, 33, 37, 49, 63, 64,
    75-76`) — `УПОРЯДОЧИТЬ ПО ... ВОЗР` canonical sort form,
    multi-field `УПОРЯДОЧИТЬ ПО Период УБЫВ, ...`, prose
    reference to HIERARCHY at line 63;
  - chapter 17 (`chapter_017.html:17, 29, 32, 49, 52`) —
    AUTOORDER / АВТОУПОРЯДОЧИВАНИЕ canonical example, sort by
    ссылочное-поле variant;
  - chapter 22 (`chapter_022.html:15, 26, 35`) — `ГДЕ` primary
    `Условие отбора данных из таблицы`;
  - chapter 23 (`chapter_023.html:13, 25-27, 45-46`) — `ГДЕ ...
    ПОДОБНО "%Иван%"` LIKE+WHERE integration, `НЕ ... ПОДОБНО`
    form;
  - chapter 24 (`chapter_024.html:15, 16`) — `&Клиент` parameter
    substitution in WHERE conditions;
  - chapter 27 (`chapter_027.html:39, 51`) — `УПОРЯДОЧИТЬ ПО
    Наименование ИЕРАРХИЯ` canonical hierarchical-ordering
    syntax (Slice 11 C2 MANDATORY FIX source);
  - chapter 34 (`chapter_034.html:14, 33, 44, 46, 51, 52`) —
    `СГРУППИРОВАТЬ ПО` canonical with агрегатные функции
    СУММА/МИНИМУМ/МАКСИМУМ/СРЕДНЕЕ/КОЛИЧЕСТВО;
  - chapter 35 (`chapter_035.html:23, 29, 41, 44, 45, 49`) —
    multi-field `СГРУППИРОВАТЬ ПО` example, `ИМЕЮЩИЕ` HAVING
    primary explanation;
  - chapter 39 (`chapter_039.html:13, 25, 29, 48, 49, 51`) —
    canonical `ИТОГИ ... ПО ОБЩИЕ` example.

- The C0a-extended SELECT mini-spec at
  `docs/legal/sdbl-select-mini-spec.md`, specifically the
  §WHERE / §GROUP BY / §HAVING / §ORDER BY / §AUTOORDER /
  §TOTALS BY / §FOR UPDATE / §INDEX BY clause-body sections,
  the §IDE-recovery allowances block (4 entries), and the §ITS
  coverage verification table.

- The lexer Slice 2 clean-room attestation
  `docs/legal/sdbl-clean-room-slice2.md` for bilingual EN/RU
  keyword pairs (`WHERE/ГДЕ`, `GROUP/СГРУППИРОВАТЬ`,
  `HAVING/ИМЕЮЩИЕ`, `ORDER/УПОРЯДОЧИТЬ`, `BY/ПО`, `FOR/ДЛЯ`,
  `UPDATE/ИЗМЕНЕНИЯ`, `INDEX/ИНДЕКСИРОВАТЬ`,
  `AUTOORDER/АВТОУПОРЯДОЧИВАНИЕ`, `TOTALS/ИТОГИ`,
  `OVERALL/ОБЩИЕ`, `ASC/ВОЗР`, `DESC/УБЫВ`,
  `HIERARCHY/ИЕРАРХИЯ`).

- The Slice 1, 2, 6, 7, 8, 9, 10a, and 10b clean-room
  attestations for event-parser conventions, the
  `at_sdbl_keyword` / `eat_sdbl_keyword` / `is_join_keyword`
  helper contracts, and the AST-shape invariants of upstream
  slices.

- The HIR consumer code at
  `crates/sdbl-hir/src/lower/clauses.rs` (read-only
  documentation of the consumer-side AST-shape requirements
  for `SdblWhereClause`, `SdblGroupClause`, `SdblOrderClause`,
  including the `LogicalOrInWhere` recursive-walk reachability
  invariant at lines 170-192).

## Non-consultation statement

The author of the Slice 11 clean-room work asserts:

- `../bsl-parser/*` was not consulted at any point during C0a /
  C0b / C1 / C2 / C3 authoring of the 12 Slice 11 function
  bodies, the per-function provenance comments, the C0a
  mini-spec extension, the C0b Bucket-A gap tests, or the C3
  acceptance suite.

- The pre-C1 textual transcription of the 12 Slice 11 function
  bodies — which lived under the previous `LEGACY (Slices 5,
  11 pending)` banner before C1 physically relocated them
  under the new `CLEAN-ROOM Slice 11 — clauses after FROM`
  banner — was not used as working text for the C2 rewrite.
  The C2 rewrite was authored from the four sources listed
  above (ITS chapters, mini-spec, lexer Slice 2 attestation,
  upstream Slice 1/2/6/7/8/9/10a/10b attestations) and from
  the HIR consumer code at `clauses.rs` for child-attachment
  contracts. The Slice 11 plan itself, at
  `<engineering scratch plans>/serialized-moseying-orbit.md`,
  served as the high-level routing document for which
  function emits which NodeKind and which Tier classification
  applies to each provenance comment.

The Slice 11 plan (codex pair-reviewed across 14 findings over
6 review rounds before approval) and the C0a / C0b / C1 / C2
codex pair-review rounds (5 / 6 / 1 / 4 rounds respectively,
all converging to APPROVE) form the audit trail for the
clean-room authorship.

## Preserved pre-refactor behaviours

The 12 functions emit syntax trees with the same observable
shape as the pre-C1 implementation, except for the one
behaviour change documented in the next section.

1. **`query_body_clauses` allows ORDER BY at the body-tail
   position AND `select_tail_clauses` accepts ORDER BY at the
   post-`query` tail position.** Both call sites preserved.
   Pinned by `test_slice11_body_order_by_vs_tail_order_by` in
   the C3 acceptance suite.

2. **`select_tail_clauses` accepts AUTOORDER / ORDER / TOTALS
   in any order, each at most once per loop invocation.**
   Pinned by `test_slice11_tail_any_order_no_cross_query_leak`
   in C0b and `test_slice11_tail_any_order_autoorder_after_totals`
   in C3.

3. **`order_by_item` does NOT emit a per-item wrapper despite
   the function name.** Pinned by
   `test_slice11_order_by_flat_children_no_wrapper` in C0b and
   `test_slice11_order_by_flat_children` in C3.

4. **`for_update_clause` greedy MDO chain consumes all
   subsequent `Dot Ident` pairs until the
   `is_clause_keyword(p)` outer guard or the post-Dot lookahead
   fails.** Pinned by `test_slice11_for_update_deep_mdo_chain`
   in C0b and `test_slice11_for_update_deep_mdo_chain` in C3.

5. **`totals_by_clause` does NOT enforce the OVERALL keyword
   shape — it falls through `is_expression_start` and the
   OVERALL Ident is consumed as a bare `SdblColumnRef`.**
   §IDE-recovery allowance #1. Pinned by
   `test_slice11_totals_overall_fallthrough_shape` in C0b and
   `test_slice11_totals_overall_fallthrough_ru` in C3.

6. **`index_by_clause` does NOT enforce that indexed
   expressions correspond to selected fields.** Pure syntactic
   — semantic checking is Slice 13 territory.

7. **`having_clause` calls `expression(p)`, NOT
   `logical_expression(p)`.** Slice 10a wraps both entry points
   in `SdblLogicalOrExpr`. Pinned by
   `test_slice11_having_logical_expression_wrapping` in C0b and
   `test_slice11_having_logical_expression_wrapping` in C3.

8. **GROUP / ORDER / INDEX missing-BY recovery: bare-keyword
   shape.** Leading clause keyword consumed before BY check;
   early-return emits a bare clause with no direct child nodes
   and no non-trivia tokens beyond the leading keyword. Pinned
   by `test_slice11_{group,order,index}_missing_by_recovery` in
   C0b and `test_slice11_{group,order,index}_missing_by_recovery`
   in C3.

9. **TOTALS missing-BY recovery: TOTALS-variant shape.** Pre-BY
   aggregate-expression loop runs FIRST, so `ИТОГИ A` produces
   `SdblTotalsBy` with TOTALS+A expression child (NOT a bare-
   keyword node). Pinned by
   `test_slice11_totals_missing_by_recovery` in C0b and
   `test_slice11_totals_missing_by_recovery` in C3.

10. **`is_clause_keyword` includes the JOIN family via
    delegation to `is_join_keyword`.** Pinned by
    `test_slice11_is_clause_keyword_join_delegation` in C0b and
    `test_slice11_is_clause_keyword_join_delegation` in C3.

## Behaviour change

### Pre-C2 behaviour

The pre-C2 `order_by_item` at
`crates/parser/src/grammar/sdbl/select.rs:1235-1246` consumed
only the optional `ASC/ВОЗР/DESC/УБЫВ` modifier and left
HIERARCHY / ИЕРАРХИЯ in the token stream. Input
`УПОРЯДОЧИТЬ ПО Поле ИЕРАРХИЯ` therefore broke the
`select_tail_clauses` loop at the unconsumed `ИЕРАРХИЯ` token.

### Root cause

The Slice 11 plan codex Round-1 finding 2 audit identified
that ITS chapter 27 (`chapter_027.html:39, 51`) explicitly
documents `УПОРЯДОЧИТЬ ПО Наименование ИЕРАРХИЯ` as
canonical hierarchical-ordering syntax — placing HIERARCHY on
ORDER BY items at **ITS Tier A2** (ITS prose-attested), NOT a
local IDE-recovery allowance. The pre-C2 parser's failure to
consume HIERARCHY was therefore a documented-syntax-loss bug,
not a recovery-quality issue.

### C2 fix

Slice 11 C2 extends `order_by_item` to consume the optional
HIERARCHY/ИЕРАРХИЯ modifier as a third position after the
optional ASC/DESC modifier:

```rust
if p.at_keyword("HIERARCHY") || p.at_keyword("ИЕРАРХИЯ") {
    p.bump();
    p.skip_trivia();
}
```

The fix preserves the flat-sibling layout — the HIERARCHY
IDENT token sits next to the expression node and the
optional ASC/DESC token, all flat at the
`SdblOrderClause` level. No per-item wrapper. No new
NodeKind. Mechanically tight: 4 lines added.

### Post-C2 behaviour

Input `УПОРЯДОЧИТЬ ПО Поле ИЕРАРХИЯ` now parses cleanly: the
`SdblOrderClause` direct children include the `Поле`
expression node AND the `ИЕРАРХИЯ` IDENT token as a flat
sibling. The `select_tail_clauses` loop continues past the
ORDER BY clause without breaking. The `assert_clean_parse`
helper used by the C0b regression-gate test (g) passes,
proving zero ERROR descendants.

### HIR semantic-interpretation scope

The HIERARCHY consumption is **parser-only acceptance**. The
HIR consumer at
`crates/sdbl-hir/src/lower/clauses.rs:114-156` and the
`OrderByItem` struct at `crates/sdbl-hir/src/hir.rs` do NOT
yet recognise the HIERARCHY/ИЕРАРХИЯ token (`OrderByItem`
has no hierarchy field; `SortDirection` lowering only reads
`ASC/ВОЗР/DESC/УБЫВ`). Therefore `ORDER BY A ИЕРАРХИЯ`
lowers identically to `ORDER BY A` from the IDE / semantic
layer's perspective.

Extending HIR is **out of Slice 11 scope** — the plan
§Constraints declares `crates/sdbl-hir/**` read-only — and
is owned by **Slice 13** (sdbl-hir reattachment), which will
add a hierarchy field to `OrderByItem` and a HIR regression
test. Slice 11's contribution is the syntax-tree contract:
HIERARCHY is reachable from `SdblOrderClause` as a flat
sibling IDENT token, so Slice 13's reader can pick it up
without further parser changes.

### Regression gates

- **C0b regression-gate test (g)**:
  `test_slice11_order_by_hierarchy_consumed` in
  `crates/parser/tests/sdbl_parser_tests.rs`. Landed
  `#[ignore]`-ed in C0b; unignored atomically with the C2
  fix. Now an active gate that PASSES.

- **C3 acceptance test**:
  `test_slice11_order_by_hierarchy_canonical_ru` in
  `crates/parser/tests/sdbl_slice11_clauses.rs`, citing the
  ITS chapter 27 canonical example.

The remaining audit-gate candidates from the Slice 11 plan
§Pre-existing bug audit defaulted to **Option B PRESERVE**
per Slice 9 pattern: the candidates are local-recovery-
quality issues, Slice 11 inherits Slice 10b's clause-keyword
fix transitively through every `expression(p)` /
`logical_expression(p)` call site, and Slice 12's scope owns
IDE-recovery hardening.

## Verification recipe

The acceptance run for Slice 11 C3 is, in order:

1. `cargo test -p parser --test sdbl_parser_tests` — 192
   passed (was 178 + 14 added in C0b; test (g) flipped from
   ignored to passing in C2). ZERO ignored.
2. `cargo test -p parser --test sdbl_slice11_clauses` — new in
   C3; the spec-driven acceptance suite.
3. `cargo test -p parser --test sdbl_slice6_package` — 26 tests
   (unchanged).
4. `cargo test -p parser --test sdbl_slice7_fields` — 26 tests
   (unchanged).
5. `cargo test -p parser --test sdbl_slice8_sources` — 28 tests
   (unchanged).
6. `cargo test -p parser --test sdbl_slice9_joins` — 17 tests
   (unchanged).
7. `cargo test -p parser --test sdbl_slice10a_backbone` — 28
   tests (unchanged).
8. `cargo test -p parser --test sdbl_slice10b_predicates` — 43
   tests (unchanged).
9. `cargo test -p parser --test sdbl_slice2_keywords` — 45
   tests (unchanged).
10. `cargo test -p parser --test sdbl_golden_corpus` — 23 tests
    (unchanged).
11. `cargo test -p parser --test sdbl_slice1_core` — 4 passed
    + ignored (unchanged).
12. `cargo test -p parser` — full parser suite (integration +
    inline `mod tests`).
13. `cargo test -p sdbl-hir` — 204+ HIR lowering tests (HIR
    consumers continue working unchanged because Slice 11
    preserves all child-attachment shapes; HIERARCHY token is
    silently ignored by the existing reader, parser-only
    acceptance per documented scope).
14. `cargo test -p lexer` — Slices 1 + 2 regression gate.
15. `cargo test -p ide-db` — SDBL validation tests including
    `parse_sdbl` path.
16. `cargo test -p ide --test sdbl_completion_integration_test`
    — subquery-in-expression + UNION scenarios.
17. `cargo test -p ide` — full IDE test suite.
18. `cargo test -p ide-diagnostics` — 1572+ tests including
    the `LogicalOrInWhere` producer-side regression gate
    (`logical_or_in_the_where_section_of_query.rs`); Slice 11
    preserves the recursive-walk reachability of KW_OR tokens
    via the SdblLogicalOrExpr wrapper.
19. `cargo test -p mcp-server` — 72 tests.
20. `cargo build --workspace --all-targets` — workspace build.
21. `cargo clippy -p parser --all-targets --all-features --
    -D warnings` — parser clippy clean.
22. `cargo clippy -p lexer --all-targets --all-features --
    -D warnings` — lexer clippy clean (Slice 11 does not touch
    the lexer).
23. `git log --follow crates/parser/src/grammar/sdbl/select.rs`
    — shows C1, C2 as separate commits; C0a, C0b, and C3 do
    not touch this file.
24. `git diff develop..HEAD --stat` — exactly the 7 files
    listed in the §Commit trail below.

## Commit trail

Five anchor commits on the `legal/sdbl-slice11-clean-room`
feature branch off local `develop`:

- **C0a** `81bd6db6` (2026-04-26) — extend
  `docs/legal/sdbl-select-mini-spec.md` with full §WHERE /
  §GROUP BY / §HAVING / §ORDER BY / §AUTOORDER / §TOTALS BY /
  §FOR UPDATE / §INDEX BY clause-body sections, §IDE-recovery
  allowances block (4 entries), §ITS coverage verification
  table extension (rows for chapters 16, 17, 22, 23, 24, 27,
  34, 35, 39 with 3 verified-yes at C0a + remaining TODO at
  C2), §Non-consultation statement (Slice 11 reaffirmation).
  Pre-C2 authoring deliverable — no parser-code, test, or
  other-doc edits. Codex pair-review: 5 rounds, 6 findings
  resolved (R1 OVERALL allowance preamble; R2 HIERARCHY BNF
  framing + non-consultation overclaim; R3 TOTALS missing-BY
  recovery doc + chapter 17 split; R4 §AUTOORDER coexistence
  whole-query overreach; R5 APPROVE). +367/-12 LOC.

- **C0b** `6d02c07d` (2026-04-26) — add 14 Bucket-A gap-test
  functions (a)–(n) in
  `crates/parser/tests/sdbl_parser_tests.rs`. Test (g)
  `test_slice11_order_by_hierarchy_consumed` lands
  `#[ignore]`-ed as the regression-gate for the MANDATORY C2
  HIERARCHY consumption fix. The other 13 tests pin pre-C2
  parser behaviour for §IDE-recovery allowances, §AST-shape
  invariants, and the cross-slice JOIN delegation boundary.
  Test count 178 → 192 (191 passed + 1 ignored). Codex
  pair-review: 6 rounds, 12 findings resolved (R1 4 weak
  tests; R2 missing-BY recovery + cardinality; R3
  has_errors() on valid cases + fail-closed owner attribution;
  R4 owner identity by text_range + clean-parse on test (g);
  R5 disjoint-query enforcement; R6 APPROVE). +795 LOC.

- **C1** `c668ea5f` (2026-04-26) — physical relocation of the
  12 Slice 11 functions out of the `LEGACY (Slices 5, 11
  pending)` banner block in
  `crates/parser/src/grammar/sdbl/select.rs` into a new
  `CLEAN-ROOM Slice 11 — clauses after FROM` banner placed
  between the Slice 9 banner and the residual
  `LEGACY (Slice 5 + SELECT limitation helpers pending)`
  banner. Pure refactor — zero functional changes inside any
  of the 12 function bodies; each carries a `// C1 placeholder
  — clean-room rewrite in C2` marker. Slice 6 select_query
  body comment + Slice 7 query body comment updated to
  reference Slice 11 banner. `crates/parser/src/grammar/sdbl.rs`
  `## Provenance` docstring extended with a Slice 11 — clean-
  room (rewrite in progress) bullet (NO attestation citation
  per Slice 10b Round-7 precedent — citation lands at C3).
  Codex pair-review: 1 round, APPROVE. +203/-157 LOC across
  2 files.

- **C2** `b1a3455a` (2026-04-26) — clean-room rewrite of the
  12 Slice 11 function bodies, replacing the
  `// C1 placeholder` markers with tiered (A1/A2/B/C/D)
  per-function provenance comments. **MANDATORY HIERARCHY
  consumption fix** in `order_by_item` (4 lines added — see
  §Behaviour change above) atomic with unignoring the C0b
  regression-gate test (g) `test_slice11_order_by_hierarchy_consumed`.
  Test count 191+1ignored → 192 passed (zero ignored).
  `docs/legal/sdbl-select-mini-spec.md` §ITS coverage
  verification rows filled in from direct ITS dump reads
  (chapters 16, 17 sort-by-ссылочное-поле lines, 22, 23, 24,
  34, 35 verified-yes; FOR UPDATE / INDEX BY verified-no via
  direct `rg`); §ORDER BY HIERARCHY paragraph rewritten from
  "post-Slice-11-C2 target grammar" to "active Slice 11 C2
  parser grammar"; §HIR semantic-interpretation scope notes
  added to §ORDER BY and order_by_item docstring; §Non-
  consultation statement rewritten to distinguish C0a
  authoring inputs vs C2 verification pass.
  `crates/parser/src/grammar/sdbl.rs` Slice 11 bullet NOT
  touched in C2 (Slice 10b Round-7 precedent — citation lands
  at C3). Codex pair-review: 4 rounds, 4 findings resolved
  (R1 HIERARCHY parser-only acceptance scope; R2 stale §ORDER
  BY post-C2 wording; R3 §Non-consultation contradiction;
  R4 APPROVE). +355/-139 LOC across 3 files.

- **C3** `8ebe376b` (2026-04-26) — Slice 11 attestation
  (this file), spec-driven acceptance suite at
  `crates/parser/tests/sdbl_slice11_clauses.rs` (35 tests),
  master-doc status flip in
  `docs/legal/sdbl-clean-room-slices.md` (§Slice 11 →
  complete with full provenance summary),
  `crates/parser/src/grammar/sdbl.rs` Slice 11 bullet flipped
  from "rewrite in progress" to "complete (landed with C3
  2026-04-26)" with attestation citation
  `docs/legal/sdbl-clean-room-slice11.md`.

- **Anti-Hilbert close-out** `<CLOSE_OUT_COMMIT>` (2026-04-26)
  — replaces the `8ebe376b` placeholder above with the
  actual SHA of the C3 commit. This second commit lands
  immediately after the C3 commit on the same feature branch
  and is what makes the §Commit trail self-resolving: the
  close-out edits this file in-place to fill in the C3 SHA,
  then records its own SHA at `<CLOSE_OUT_COMMIT>` (which is
  the only remaining placeholder, and which is by construction
  not knowable until after the close-out lands — the same
  fixed-point pattern as Slice 9's `ef85b028` /
  `ecb26896` close-out pair and Slice 10b's two close-out
  commits).

**Anti-Hilbert disclosure.** This C3 commit is the last anchor
of the Slice 11 commit trail. Per the Slice 9 / Slice 10b
precedent, the C3 commit message and the attestation §Commit
trail entry both contain a `8ebe376b` placeholder for
the C3 SHA at land time. The Anti-Hilbert close-out commit
listed above lands IMMEDIATELY after C3 on the same feature
branch and replaces both `8ebe376b` placeholders with
the actual C3 SHA. The close-out commit's own SHA is filled
in by a one-line follow-up edit at the very end if needed (or
left as `<CLOSE_OUT_COMMIT>` if the close-out is the truly
absolute-last commit on this slice's branch — same fixed-
point convention as Slice 9 / Slice 10b).

## Licensing note

The clean-room status of `crates/parser` cannot be flipped to
the project-wide MIT license tier yet. Slice 5 (virtual-table
args clean-room) and the SELECT-prefix limitation helpers
(`is_limitation_keyword` / `limitations` / `top_clause`) are
still pending in the residual `LEGACY (Slice 5 + SELECT
limitation helpers pending)` banner. The lexer's vocabulary-
heavy domains (Slices 3 and 4) and the sdbl-hir reattachment
(Slice 13) remain. License-tier promotion is therefore
deferred to post-Slice-13.

## Author attestation

Authored 2026-04-26 by Кирилл Елишев (vlad.ugra@gmail.com)
with assistance from Claude Opus 4.7 (1M context) under the
clean-room discipline documented in
[`sdbl-clean-room-slices.md`](sdbl-clean-room-slices.md). The
C0a / C0b / C1 / C2 / C3 authoring rounds and the codex pair-
review feedback that shaped each commit are recorded above
under §Commit trail. The Slice 11 plan at
`<engineering scratch plans>/serialized-moseying-orbit.md`
served as the high-level routing document for this work; that
plan was itself codex pair-reviewed across 14 findings over 6
review rounds before approval.
