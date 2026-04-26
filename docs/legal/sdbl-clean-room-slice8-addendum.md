# SDBL clean-room — Slice 8-addendum (virtual-table arguments)

## Status

Landed 2026-04-26 on the local branch
`legal/sdbl-slice8-addendum-clean-room`. Not yet pushed to
`origin/develop`.

This slice retires the LEGACY banner block in
`crates/parser/src/grammar/sdbl/select.rs` entirely. After Slice
8-addendum lands the parser-side `select.rs` carries zero LEGACY
content; the file becomes one continuous sequence of clean-room
banners (Slice 6 → Slice 7 → Slice 8 → Slice 9 → Slice 11 →
Slice 7-addendum → Slice 8-addendum).

## Scope

Two functions in
`crates/parser/src/grammar/sdbl/select.rs` are promoted into a new
`CLEAN-ROOM Slice 8-addendum — virtual-table arguments` banner:

1. `virtual_table_args` — parses the trailing `'(' [vt-arg-list]
   ')'` after a `table_ref` MDO chain. Renamed from
   `virtual_table_args_legacy` in C1; clean-room rewrite from the
   SELECT mini-spec §Virtual table argument behavior in C2. Sole
   call site: `table_ref` in the same file.
2. `recover_to_delimiter_vt` — paren-depth-tracking
   spurious-token recovery helper for the VT-args context. Sole
   caller: `virtual_table_args` (two call sites inside its body).
   Relocated in C1 from a stranded position above the Slice 6
   banner; clean-room rewrite from the same mini-spec section in
   C2.

**NodeKinds locked in place by Slice 8-addendum** (zero new
variants):

- `SdblMissingArg` — empty-arg sibling marker (pre-existing,
  emitted by `virtual_table_args`).
- `Error` — recovery sub-node (pre-existing, emitted by
  `recover_to_delimiter_vt`).
- `SdblTableRef` — parent node that hosts the VT-args flat
  direct children (Slice 8 territory, unchanged here).

**Function → NodeKind attribution:**

| Function | Emits |
|---|---|
| `virtual_table_args` | (no NodeKind — children attach to the parent `SdblTableRef` as flat siblings: `LParen` token, expression-NodeKind result of `expression(p)`, `SdblMissingArg` markers, `Comma` tokens, `Error` sub-nodes from the recovery helper, `RParen` token) |
| `recover_to_delimiter_vt` | `Error` |

**Per-function provenance source map (Tier classification):**

| # | Function | Tier source map |
|---|---|---|
| 1 | `virtual_table_args` | A1 (v8327doc Глава 8.2 «Виртуальные таблицы» + Глава 8.3 «Виртуальные и обычные поля» canonical example `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )` at `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`; pubqlang chapter 104 `Обороты` with date-helper nested function call + named-condition trailing arg, chapter 116 `Обороты()` parameter-order prose, chapter 152 no-args `Остатки()` + leading-empty `Остатки( , cond)`, chapter 156 IN-subquery as VT param structural form, peripheral chapter 9 `СрезПоследних` prose intro); C (SELECT mini-spec §Virtual table argument behavior — Grammar EBNF, AST-shape contract, IDE-recovery allowances #1–#6, Tier classification); D (parser-internal `SdblMissingArg` marker shape and clause-keyword fall-through allowance #6) |
| 2 | `recover_to_delimiter_vt` | D (parser-internal recovery utility for VT-args context; functionally equivalent to `recover_to_delimiter` in `expressions.rs` — both share paren-depth tracking, comma / semicolon stop, `is_clause_keyword` stop, and unconditional `Error` emit; the helpers differ only in module location) |

## Child-attachment invariants

1. **`SdblTableRef` direct-child sequence after the MDO chain
   accepts the VT-args layout.** When `table_ref` ends with a
   `(`, the following children attach **inline** (not in a
   sub-node):
   - `LParen` token (consumed at the top of `virtual_table_args`);
   - 0+ argument bodies — each one of: an expression NodeKind
     (one of the 9 expression NodeKinds enumerated in the SELECT
     mini-spec §AST-shape contract), an `SdblMissingArg` marker,
     or (for malformed input) an expression NodeKind followed
     by an `Error` sub-node from `recover_to_delimiter_vt`;
   - `Comma` tokens between args;
   - `RParen` token (consumed by `p.expect(RParen)`).

   No new wrapper. The HIR consumer at
   `crates/sdbl-hir/src/lower/from_clause.rs:246-371` walks
   `SdblTableRef.syntax().children()` directly and lowers each
   expression-NodeKind into the
   `virtual_table_params: Vec<ExprHir>` field declared at
   `crates/sdbl-hir/src/hir.rs:172`. The clean-room rewrite in
   C2 preserves the parser's flat direct-child layout so the
   existing HIR walker continues working unchanged.

2. **`SdblMissingArg` is a bare marker.** The empty-arg sub-node
   carries no children — consumers may count `SdblMissingArg`
   siblings to compute "user supplied a default for slot N"
   semantics.

3. **`recover_to_delimiter_vt` unconditionally emits an `Error`
   marker.** The marker is opened at the top of the function
   and completed unconditionally after the recovery loop exits,
   regardless of how many tokens were consumed. The marker's
   token children depend on what the loop ate before
   terminating; whether the marker is non-empty is incidental.

4. **Paren-depth tracking absorbs nested `(...)` calls.** A
   spurious `Q` between a parenthesised expression and the next
   comma — as in `Остатки(СУММА(A) Q, B)` — is consumed inside
   the `Error` marker without the helper mistaking the inner
   `СУММА(A)` close-paren for the outer VT-args terminator.

## AST-shape invariants (operational contracts)

1. **The outer `if !p.at(LParen) { return; }` guard makes the
   call site in `table_ref` unconditional** — `table_ref`
   invokes `virtual_table_args(p)` without checking for `(`
   itself. When the next token is not `(` the function is a
   pure no-op.

2. **Trailing comma `(...,,)` produces N `SdblMissingArg`
   siblings, NOT an error.** The empty-trailing-arg form is
   the 1C idiom (canonical v8327doc shape `(, , Авто, , )` has
   two trailing empty slots followed by `)`).

3. **`expression(p)` is called even for ill-formed args.** When
   an expression is followed by an unexpected non-comma /
   non-`)` token, the partial expression node emits first and
   the recovery helper wraps the trailing junk in an `Error`
   sub-node — the expr + Error pair appears as flat siblings
   inside `SdblTableRef`.

4. **`p.check_iteration_limit()` guards both the comma loop in
   `virtual_table_args` and the recovery loop in
   `recover_to_delimiter_vt`.** Both locations have
   malformed-input infinite-loop risk, so both must check.

5. **`p.expect(TokenKind::RParen)` is mandatory at the end of
   `virtual_table_args`, including on recovery paths.** If the
   recovery helper exits at EOF without finding the closing
   `)`, the parser emits a missing-`)` diagnostic at the
   `expect` call. The `RParen` is the unique sync point that
   exits the function.

## Sources consulted

The C2 clean-room rewrite was authored from:

- v8.3.27 Developer's Reference Глава 8 «Работа с запросами» —
  `https://its.1c.ru/db/v8327doc#bookmark:dev:TI000000453`. The
  primary SDBL grammar specification. Cited at C2 for the
  Глава 8.2 «Виртуальные таблицы» VT-introduction prose, Глава
  8.3 «Виртуальные и обычные поля» canonical example
  `РегистрНакопления.УчетНоменклатуры.ОстаткиИОбороты(, , Авто, , )`,
  and the 4–5 sibling examples in the Глава 8.3 listing.
- the ITS pubqlang dump (textbook companion) — chapter 9
  `СрезПоследних` virtual-table intro prose (peripheral),
  chapter 104 `Обороты` with date-helper nested function call +
  named-condition trailing arg `(&НачалоПериода,
  КОНЕЦПЕРИОДА(&КонецПериода, ДЕНЬ), , Номенклатура = &Товар)`,
  chapter 116 §Параметры виртуальной таблицы оборотов
  (parameter-order prose: `НачалоПериода`, `КонецПериода`,
  `Периодичность`, `Условие`, `КорСубконто`, ...), chapter 152
  no-args `Остатки()` + leading-empty
  `Остатки( , Номенклатура = &Номенклатура)`, chapter 156
  multi-line VT call with `IN (ВЫБРАТЬ ...)` subquery as a VT
  param;
- the C0a-extended SELECT mini-spec at
  `docs/legal/sdbl-select-mini-spec.md` §Virtual table argument
  behavior — Grammar EBNF, AST-shape contract, IDE-recovery
  allowances #1–#6, Tier classification, §ITS coverage
  verification rows;
- the lexer Slice 2 attestation
  (`docs/legal/sdbl-clean-room-slice2.md`) for cross-checking
  that VT-args use `Ident` pass-through (`Авто`, `Остатки`,
  `Обороты`, `СрезПоследних`, `ОстаткиИОбороты` are regular
  identifiers, not lexer keywords);
- the Slice 1 / 2 / 6 / 7 / 8 / 9 / 10a / 10b / 11 / 7-addendum
  clean-room attestations for event-parser conventions and
  AST-shape contracts (in particular the Slice 8 attestation
  for the `table_ref` MDO chain that hosts the
  `virtual_table_args` call, the Slice 10a / 10b attestations
  for the 9-NodeKind expression backbone that produces the
  expression-arg children, and the Slice 11 attestation for
  `is_clause_keyword`);
- the HIR consumer at
  `crates/sdbl-hir/src/lower/from_clause.rs:246-371` for
  read-only documentation of consumer-side AST-shape
  requirements (the existing
  `virtual_table_params: Vec<ExprHir>` lowering walks
  `SdblTableRef.syntax().children()` directly; the clean-room
  rewrite preserves the flat direct-child layout the HIR
  walker reads).

Per the user's citation-policy directive, this §Sources
consulted block cites only public URLs (the v8327doc bookmark
URL above) and pubqlang chapter identifiers (with optional
stable line numbers per §D8a in the SELECT mini-spec). This
convention applies prospectively to Slice 8-addendum and
beyond. Prior slices retain their pre-policy citation form.

## Non-consultation statement

The author did **not** consult `../bsl-parser/*` grammar text
during the C0a / C0b / C1 / C2 / C3 authoring. The C2
clean-room rewrite was authored from the spec contracts (mini-
spec EBNF + AST-shape contract + IDE-recovery allowances)
plus the cited ITS sources. The author did not read the
pre-C1 function bodies of `virtual_table_args` (then named
`virtual_table_args_legacy`) or `recover_to_delimiter_vt` as
working text during C2 authoring; the C1 placeholder bodies
were physically present in the working tree (pure refactor)
but were not consulted as working text — the rewrite derives
from the spec.

## Preserved pre-refactor behaviours

The two functions emit syntax trees with the same observable
shape as the pre-C1 implementation, modulo the §Behaviour
change section below (which is empty by default).

1. **No wrapper NodeKind for VT-args** — `virtual_table_args`
   attaches children to the parent `SdblTableRef` directly.
   Pinned by `test_slice8adn_empty_paren_pair_ru` /
   `_en` (the empty-paren pair forces direct-child layout
   inspection without any per-arg content) and by
   `test_slice8adn_canonical_v8327doc_ru` (which asserts the
   exact direct-child interleaved sequence for the
   v8327doc 5-arg canonical shape).

2. **Empty `()` accepted (no args at all)** — VT methods that
   take no parameters parse cleanly. Pinned by
   `test_slice8adn_empty_paren_pair_ru` /
   `test_slice8adn_empty_paren_pair_en`.

3. **`recover_to_delimiter_vt` always emits an `Error`
   marker.** The unconditional emit is the shared contract
   with the sibling `recover_to_delimiter` in
   `expressions.rs:182-233`. Pinned by
   `test_slice8adn_recover_always_emits_error`.

4. **Clean `IN (subquery)` and nested function calls are
   consumed inside `expression(p)`** — the recovery helper is
   a safety net for malformed input only, not the primary path
   for nested forms. Pinned by
   `test_slice8adn_paren_balanced_subquery_arg_ru` and
   `test_slice8adn_nested_function_call_arg`.

5. **`SdblMissingArg` markers persist through HIR.** The HIR
   walker at
   `crates/sdbl-hir/src/lower/from_clause.rs:246-371` already
   handles them as siblings of expression NodeKinds inside
   `SdblTableRef`. The clean-room rewrite keeps the bare-
   marker shape so the existing HIR walker continues working.

## Behaviour change

**One regression-free fix landed in the post-C3 close-out:**
`recover_to_delimiter_vt` now treats clause keywords as
recovery terminators at **any** paren depth, not just at
`paren_depth == 0` as the pre-rewrite parser did. The
practical impact: when an unterminated nested `(...)` inside a
VT-args list is followed by a clause keyword like
`ГДЕ` / `СГРУППИРОВАТЬ` / `УПОРЯДОЧИТЬ` / `ИЗ`, the recovery
helper now stops at the keyword instead of gobbling it (and
any following tokens) into the `Error` sub-node. This is a
recovery-quality improvement: the unterminated VT-args list
no longer destructively consumes content that belongs to the
outer query. Pinned by the regression test
`test_slice8adn_recovery_stops_on_clause_keyword_at_any_depth`
in `sdbl_slice8_addendum_virtual_table_args.rs`.

Comma and Semicolon, in contrast, remain depth-0-only
terminators — a comma inside a nested function-call argument
list (e.g. `СУММА(A, B)`) is part of the nested call's
grammar and must not terminate recovery.

Note that the parallel helper `recover_to_delimiter` in
`crates/parser/src/grammar/sdbl/expressions.rs:182-233`
(Slice 10a territory) retains the original depth-0-only
clause-keyword guard. Aligning the two helpers — by applying
the same depth-any clause-keyword fix to
`recover_to_delimiter` — is deferred to Slice 12 (IDE
recovery / allowances), which owns cross-helper recovery
hardening across the parser. This Slice 8-addendum fix is
scoped to the slice's own `recover_to_delimiter_vt`.

Caveat — out of scope for this slice: the deeper recovery
weakness where `Parser::expect(RParen)` at a clause keyword
falls through to `Parser::error()` and bumps the keyword into
its own `Error` sub-node remains. The §Behaviour change above
addresses only the `recover_to_delimiter_vt` helper's
gobbling; broader missing-RParen recovery quality is Slice 12
territory. The Slice 8-addendum regression test pins the
narrow contract by asserting that no single `Error` sub-node
under `SdblTableRef` contains BOTH `(` AND `ГДЕ` (which
would indicate the recovery helper crossed the clause
keyword), without requiring a downstream `SdblWhereClause` to
materialise.

## Verification recipe

Run end-to-end at C3 commit time. All must pass:

1. `cargo test -p parser --test sdbl_parser_tests` — 204
   tests (197 base + 7 Slice 8-addendum gap tests added in
   C0b).
2. `cargo test -p parser --test sdbl_slice6_package` — 26.
3. `cargo test -p parser --test sdbl_slice7_fields` — 26.
4. `cargo test -p parser --test sdbl_slice7_addendum_limitations`
   — 13.
5. `cargo test -p parser --test sdbl_slice8_sources` — 28.
6. `cargo test -p parser --test sdbl_slice8_addendum_virtual_table_args`
   — 16 (new in C3).
7. `cargo test -p parser --test sdbl_slice9_joins` — 17.
8. `cargo test -p parser --test sdbl_slice10a_backbone` — 28.
9. `cargo test -p parser --test sdbl_slice10b_predicates` — 43.
10. `cargo test -p parser --test sdbl_slice11_clauses` — 35.
11. `cargo test -p lexer --test sdbl_slice2_keywords` — 30.
12. `cargo test -p lexer --test sdbl_golden_corpus` — 1.
13. `cargo test -p lexer --test sdbl_slice1_core` — 34.
14. `cargo test -p parser` — full parser suite green.
15. `cargo test -p sdbl-hir` — 204+ green.
16. `cargo test -p ide-diagnostics` — full suite green
    (`JoinWithVirtualTable` regression gate included).
17. `cargo test -p mcp-server` — 65.
18. `cargo build --workspace --all-targets` clean.
19. `cargo clippy -p parser --all-targets --all-features -- -D
    warnings` clean.

## Commit trail

- **C0a** `a8e262f4` 2026-04-26 — extend SELECT mini-spec
  §Virtual table argument behavior with full AST-shape
  contract, IDE-recovery allowances #1–#6, Tier classification,
  and seven §ITS coverage verification rows (TODO at C2);
  add §Non-consultation statement (Slice 8-addendum
  reaffirmation).
- **C0b** `dd7b4b02` 2026-04-26 — add 7 Bucket-A audit-gate
  tests at the end of `sdbl_parser_tests.rs` covering empty
  `()`, single trailing-comma, canonical v8327doc 5-arg
  shape, paren-balanced subquery as VT param, mid-arg
  recovery, normal nested function call, VT-args followed by
  clause keyword without alias. Test count 197 → 204.
- **C1** `228db0b2` 2026-04-26 — relocate
  `virtual_table_args_legacy` (renamed to `virtual_table_args`)
  and `recover_to_delimiter_vt` into the new Slice 8-addendum
  banner in `select.rs`; remove the LEGACY banner block
  entirely; update the module-level Provenance docstring in
  `sdbl.rs` (Slice 5 entry now points at the lexer-side
  Slice-2 LEGACY block as the remaining Slice 5 scope); update
  test-side comments.
- **C2** `9267b29e` 2026-04-26 — clean-room rewrite of the
  two functions from the C0a-extended mini-spec + ITS
  sources; per-function provenance comments cite only public
  URL + pubqlang chapter identifiers; fill in the seven §ITS
  coverage verification rows in the mini-spec.
- **C3** `7425e1ea` 2026-04-26 — this
  attestation, the new acceptance-test file
  `sdbl_slice8_addendum_virtual_table_args.rs`, the master-doc
  splice (preserve Slice 8 file list, replace the
  legacy-helper suffix with a Slice 8-addendum cross-
  reference; add a new §Slice 8-addendum block), and the
  flip of the `sdbl.rs` Provenance docstring Slice 8-addendum
  bullet from "rewrite in progress" to "complete".

The C3 SHA placeholder above is filled by the optional
Anti-Hilbert close-out commit (or left as the placeholder if
the close-out is skipped per Slice 11 / Slice 7-addendum
precedent).

## Licensing note

Slice 8-addendum is the LAST parser-side LEGACY-banner closure.
After it lands, `crates/parser/src/grammar/sdbl/select.rs`
carries zero LEGACY content and the file becomes one
continuous sequence of clean-room banners. Tier A promotion
for the parser crate remains deferred — Slice 12 (IDE recovery
hardening) and Slice 13 (sdbl-hir reattachment) are the
remaining parser-side prerequisites for the LGPL→MIT relicense
flip.

## Author attestation

Authored by Claude Opus 4.7 (1M context) on 2026-04-26 at the
direction of the user. The C2 clean-room rewrite was authored
from the cited public ITS sources and the C0a-extended SELECT
mini-spec; pre-C1 function bodies, `../bsl-parser/*`, and any
third-party SDBL grammar text were not consulted as working
text. Per-function provenance comments inside `select.rs` cite
only public ITS URLs and pubqlang chapter identifiers.
