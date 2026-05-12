# Track 6.3 — Closure document

Closure record for Track 6.3 «Preprocessor source-of-truth»
(architectural mini-track inside ROADMAP §Track 6).

## Status

- **Status:** CLOSED.
- **Date:** 2026-05-12.
- **Scope:** Typed AST wrappers for preproc directives; shared
  `PreprocIfStmt::branches()` iterator consumed by CFG builder and
  AST-walking diagnostics; coverage validation for 3 priority cards
  (`AllFunctionPathMustHaveReturn`, `DuplicatedInsertionIntoCollection`,
  `BeginTransactionBeforeTryCatch`); 11 follow-up cards taxonomized.
- **Plan rounds:** Codex adversarial-review round 1
  (verdict: NEEDS REWORK, 2 BLOCKER + 7 MAJOR + 4 MINOR + 1 NIT —
  applied inline в v2 плана, see §10.2 of plan). No round 2 — slice-level
  adversarial-review per slice enforced quality.

## Summary

Track 6.3 introduced per-branch preprocessor analysis discipline as
the **single source of truth** for both syntactic and semantic
consumers, without introducing active-symbol pruning:

- **Typed AST surface** (Phase A): `PreSymbol`, `PreExpr`, `PreIfDir`,
  `PreElsIfClause`, `PreElseClause` wrappers — mirror `PreRegionDir`
  pattern. Iterators return concrete `impl Iterator` (no `Box<dyn>`).
  Nested-preproc isolation verified.
- **Shared HIR iterator** (Phase B): `PreprocIfStmt::branches()`
  collapses two independent per-branch enumerations (CFG builder,
  DuplicatedInsertion handler) into one source of truth.
- **3 priority cards coverage validation** (Phase C):
  - AFPMR — 4 preproc fixtures pass against existing CFG/dataflow
    infrastructure, no code change.
  - DuplicatedInsertion — 3 intra-branch fixtures cover the existing
    branch-isolated handler logic.
  - BeginTransactionBeforeTryCatch — `pre_if_all_branches_open_with_try`
    helper in lowering walker recognizes BSL-safe Begin/preproc/Try
    pattern; previously-ignored regtest re-enabled.

## Active-symbol pruning: evaluated and rejected

Per Track 6.3 design discovery (recorded in plan §Context):

1. **bsl-language-server reference impl validation**: the Java analyzer
   used in production for 5+ years does NOT do active-symbol pruning.
   Uses a hardcoded whitelist for `UnknownPreprocessorSymbol` and
   per-branch CFG sub-graphs — exactly our shape.

2. **Runtime semantics**: `#Если Сервер` is meaningful precisely because
   the same source compiles in multiple contexts. A CommonModule with
   `Server=true, ClientManagedApplication=true` compiles twice, and
   **both** branches reach runtime — just in different copies of the
   process. Pruning either branch as "inactive" is the wrong semantics
   for the most common use case.

3. **In-process server invocation matrix**: thick client in file mode,
   mobile application, external connection can all execute server-side
   code (`ObjectModule`, `ManagerModule`) in-process. Even "Server-only"
   modules accept calls from `ТолстыйКлиентОбычноеПриложение`,
   `МобильноеПриложениеСервер`, etc. Module-type → symbol-set mapping
   is genuinely multi-dimensional, and any naive active-symbol set
   trivially false-positives on these scenarios.

Conclusion: **per-branch analysis discipline** (analyze each branch
independently) is the right primary win. **Active-symbol pruning** is
dropped from the Track 6 roadmap entirely — not deferred.

Negative regression guard: `git grep -nE 'active_symbols|PreprocessorConfig|preproc_config' -- 'crates/' ':(exclude)crates/*/tests/*'` returns 0 hits as of closure.

## Phase status

| Phase | Plan section | Description | Commits | Status |
|---|---|---|---|---|
| A | §2 | Typed AST wrappers | `673f8958`, `9bc2925f`, `261daf1e` | DONE |
| B | §3 | Shared `PreprocIfStmt::branches()` + consumer refactor | `a6a2d18b`, `d4ab7303` | DONE |
| C §4.1 | AFPMR | preproc fixtures audit (no handler change) | `6d641b79`, `f3786ea9` | DONE |
| C §4.2 | DuplicatedInsertion | intra-branch fixtures + comment fix | `91d7871b`, `6d0519a9` | DONE |
| C §4.3 | BeginTransaction | preproc-aware Begin/Try pattern + regtest re-enable | `bccbb3c7`, `b4e6c755` | DONE |
| D | §5 | Closure docs + ROADMAP update | this commit | DONE |

## Slice → commit table

| Phase / Slice | Plan section | Commit |
|---|---|---|
| A.1 — PreSymbol + PreExpr | §2.1 | `673f8958` |
| A.2 — PreIfDir + PreElsIfClause + PreElseClause | §2.2 | `9bc2925f` |
| A.3 — nested-preproc isolation test | §2.3 | `261daf1e` |
| B.1 — `PreprocIfStmt::branches()` iterator | §3.1 | `a6a2d18b` |
| B.2 — CFG + DuplInsertion refactor | §3.2 | `d4ab7303` |
| C.1.1 — AFPMR preproc fixtures | §4.1 Slice 1 | `6d641b79` |
| C.1.2 — AFPMR card docs closure | §4.1 Slice 3 | `f3786ea9` |
| C.2.1 — DuplInsertion intra-branch fixtures | §4.2 Slice 1 | `91d7871b` |
| C.2.2 — DuplInsertion card docs closure | §4.2 Slice 3 | `6d0519a9` |
| C.3.1 — BeginTxn preproc-aware pattern | §4.3 Slices 1–2 | `bccbb3c7` |
| C.3.2 — BeginTxn card docs + no-else regression | §4.3 Slices 3–4 | `b4e6c755` |

## 3 priority cards closure

| Card | Outcome | Action |
|---|---|---|
| `AllFunctionPathMustHaveReturn` | Existing CFG/dataflow infrastructure (`walk_preproc_if_statement_hir` + `path_terminates`) already handles `#Если` correctly. | 4 preproc fixtures added; no handler or CFG/dataflow code change. |
| `DuplicatedInsertionIntoCollection` | Handler already isolated per-branch via `Stmt::PreprocIf` match arm; refactored to consume shared `PreprocIfStmt::branches()` iterator in Phase B. | 3 intra-branch fixtures added; stale "HIR does not lower preprocessor" comment fixed. |
| `BeginTransactionBeforeTryCatch` | Lowering walker previously emitted false positive on `Begin; #Если ... Try ... #Иначе Try ... #КонецЕсли`. | `pre_if_all_branches_open_with_try` helper added; pending `НачатьТранзакцию` consumed when every branch opens with `Попытка`, emitted otherwise. Regtest `begin_in_preproc_then_try_outside` re-enabled. |

## 11 follow-up cards — taxonomy

Cards previously listed in plan §5 as preproc-touching but **not**
prioritized in Track 6.3:

### Foundation-only (open card-level matrix follow-ups remain, but the
shared iterator + typed AST + per-branch CFG infrastructure is enough
for any future handler-level fix):

1. `CodeAfterAsyncCall` — per-branch async semantics may want fixtures,
   but no current handler bug identified.
2. `CodeOutOfRegion` — preproc + region interaction already covered in
   Track 2 Phase C §3; no further preproc gap.
3. `CodeBlockBeforeSub` — code-before-method-ordering rule is per-source,
   per-branch consideration is forward-looking only.
4. `CommentedCode` — heuristic operates per-token; no preproc-specific
   gap blocking closure.
5. `CanonicalSpellingKeywords` — orthogonal to branch activity; per-branch
   matrix not load-bearing.
6. `IdenticalExpressions` — AST-fallback exists; can adopt typed AST
   wrappers / shared iterator in a follow-up matrix slice.
7. `OneStatementPerLine` — semantics already preproc-aware.
8. `SeveralCompilerDirectives` — annotation/directive interaction
   orthogonal to per-branch analysis.
9. `UnknownPreprocessorSymbol` — handler unchanged in Track 6.3; hardcoded
   whitelist remains source of truth (matches bsl-LS reference impl).

### Forward-tracked (genuine card-level work owned by other tracks):

10. `UnreachableCode` — uses CFG with `PreprocCondition` vertex; can be
    extended with preproc-aware lineage if future per-card audit asks
    for it. Currently `unreachable_code.rs:327` is the only consumer of
    `PreprocCondition` and behaves correctly per existing tests.
11. `CommitTransactionOutsideTryCatch` — similar story to
    BeginTransactionBeforeTryCatch but with different end-of-list state
    machine. If a similar false-positive on `Commit; #Если ... #КонецЕсли`
    pattern surfaces, the same `pre_if_all_branches_open_with_try`-style
    helper can be applied. Not blocking closure.

## Acceptance gate matrix

Plan §7 enumerated 12 gates; all satisfied at closure:

| # | Gate | Result |
|---|---|---|
| 1 | `cargo test --workspace` green; `#[ignore]` baseline −1 | green; `begin_in_preproc_then_try_outside` re-enabled |
| 2 | `cargo clippy --all-targets --all-features -- -D warnings` clean | green |
| 3 | Typed AST coverage: 5 wrappers in `ast.rs` | 5 hits (PreSymbol, PreExpr, PreIfDir, PreElsIfClause, PreElseClause) |
| 4 | `PreprocIfStmt::branches()` exists in `hir.rs` | 1 hit |
| 5 | Shared iterator call-sites: ≥ 1 in CFG builder, ≥ 1 in DuplInsertion | 1 hit each |
| 6 | 3 priority cards each carry `## Закрыто Track 6.3` | done (AFPMR, DuplInsertion, BeginTxn) |
| 7 | Per-card fixture count: AFPMR ≥ 4 preproc; DuplInsertion ≥ 3; BeginTxn re-enable + ≥ 2 new | 4 / 3 / 1 re-enable + 3 new (asymmetric, inside, no-else) |
| 8 | Foundation self-tests: `syntax preproc_wrappers_tests`, `hir-def preproc_if_stmt_branches_tests` | both green |
| 9 | `TRACK_6_3_CLOSURE.md` created + ROADMAP marker | this commit |
| 10 | Push origin only | done |
| 11 | No active-symbol code paths (negative gate) | 0 hits |
| 12 | No `Box<dyn Iterator>` in new APIs | 0 hits |

## Known limitations forwarded

None block closure. Future work:

- If `UnreachableCode` audit surfaces a card-level preproc gap,
  per-branch CFG sub-graph traversal already exists via
  `PreprocConditionVertex`. Owner: Track 6 follow-up.
- If `CommitTransactionOutsideTryCatch` exposes a Commit/preproc/Try
  symmetry false positive, mirror the `pre_if_all_branches_open_with_try`
  helper for the commit-side state machine. Owner: Track 6 follow-up.
- All other 9 cards: foundation-only, no scheduled work.
