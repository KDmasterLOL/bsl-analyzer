# Track 2 — Closure document

Closure record for Track 2 «Семейственные переделки». The implementation
plan was carried in a private workspace plan file rather than checked
into the repo; this document is the canonical in-repo summary of what
was delivered.

## Status

- **Phases A / B / C / D**: closed.
- **Phase E**: E1 (per-card closure annotations) and E2 (this document)
  closed; **E3 (corpus-level perf measurements) deferred** to a separate
  session — Track 1 baseline rebuild + corpus access required.

## Summary

Track 2 collapsed five structural diagnostic clusters that were solving
the same problem in different places, into one source of truth per
fact:

- **Security registry + lattices** (Phase A): single curated registry
  in `bsl-platform/src/security/`, saturating-counter privilege/safe-mode
  lifetime lattice, intra-method value-state overlay, transitive
  effect-summary with Salsa cycle handling, guard-predicate detector.
  Ten security handlers migrated.
- **Complexity metrics** (Phase B §6): single-pass HIR visitor for
  cognitive / max-nesting / if-condition / size / params metrics;
  cyclomatic moved to CFG-based formula (`V(G) = E - N + 2*P`); seven
  complexity handlers migrated.
- **Doc-comments** (Phase B §5): `VariableDocs` joined `MethodDocs` as
  SymbolTree-owned data; quality checks for parameter/return-value
  descriptions tightened.
- **Module structure** (Phase C §3): canonical-alias / standard-regions
  policy / significant-statement predicates moved to single owners in
  `hir-def/module_structure`; `RegionTree` got a module-level filter and
  `module_level_regions_query` was removed.
- **SDBL per-rule quality** (Phase C §4): four targeted slices
  (minPathDepth config, UNION coverage, aggregation exemption,
  LikeUsage schema merge); the remaining 13 Track-A rules were
  audit-confirmed without changes.
- **Transactions** (Phase D §2): catch-body classifier
  (Empty/RaisesOnly/LogsOnly/Mixed/Silent/RollbackOnly) replaced the
  empty-only heuristic in `MissingCodeTryCatchEx`; preprocessor-aware
  Begin/Try matching documented as Track 6 deferral with an `#[ignore]`'d
  regression test.

## Phase A — Security registry + lattices

| Slice | Description | Commit |
|---|---|---|
| §1.1 / §1.2 / §1.3 / §1.4a | Foundation: security registry + value/security/effect lattices (pure helpers in `dataflow`) | `b51c38b8` |
| §1.4b/c | Salsa wrappers (`module_effect_summaries_query`, `method_effect_summary_query`, `module_security_state_query`) + `AnalysisProvider` extension | `72b7c3eb` |
| §1.5 | Guard-predicate detector (`crates/dataflow/src/guard_predicates.rs`) | `3a847187` |
| §1.6 Group A | Registry-driven recognizers: `FileSystemAccess`, `InternetAccess`, `ExternalAppStarting`, `OSUsersMethod` | `4a9a9290` |
| §1.6 Group B | Registry-driven recognizers: `ExecuteExternalCode`, `ExecuteExternalCodeInCommonModule`, `UnsafeSafeModeMethodCall` | `9588c13e` |
| §1.6 Group C | Lattice-driven `SetPrivilegedMode` + `DisableSafeMode` (HIR-side detection removed) | `f0c617e1` |
| §1.6 Group D | `PrivilegedModuleMethodCall` via `effect_summary` + guard-predicate suppression | `0862379b` |
| §1.7-A | E2E PrivilegedModuleMethodCall fixture | `f0a45e29` |

## Phase B — Complexity metrics + Doc-comments

### Complexity (§6)

| Slice | Description | Commit |
|---|---|---|
| §6.1 | `HirMethodMetrics` single-pass visitor in `hir-def` | `80fe948c` |
| §6.2 | CFG-based cyclomatic in `cfg` (pure) | `bfeabca8` |
| §6.3 | Salsa wrappers + `AnalysisProvider` (metrics + cyclomatic) | `a72ad08c` |
| §6.4 | `CognitiveComplexity` migration | `f4dd106b` |
| §6.4 | `CyclomaticComplexity` migration to CFG-based formula | `501203b8` |
| §6.4 | `NestedStatements` migration | `66308e02` |
| §6.4 | `IfConditionComplexity` migration | `ea375773` |
| §6.4 | `MethodSize` / `NumberOfParams` / `NumberOfOptionalParams` migration | `68f65fcd` |
| §6.4 cleanup | Drop retired `cognitive_complexity.rs` / `cyclomatic_complexity.rs` modules | `71c22eab` |
| §6.4 follow-up | Deterministic ordering for the older 3 handlers | `cd100d3b` |
| §6.5 | Cognitive recursion penalty via `effect_summary.is_recursive` | `a532468d` |
| §6.5 | Cyclomatic numerical alignment to SonarQube | `f7472c9e` |

### Doc-comments (§5)

| Slice | Description | Commit |
|---|---|---|
| §5.1 Slice A | `VariableDocs` parser + offset-based extractor (no consumer migration) | `55ce9dc0` |
| §5.2 Slice B | `VariableDocs` SymbolTree wiring + `MissingVariablesDescription` migration | `e289cec8` |
| §5.3 | `MissingParameterDescription` strict-mode content-quality knob | `bbaf2bde` |
| §5.3 | `MissingReturnedValueDescription` covers no-doc export when PMD disabled | `6d8a1eb2` |

## Phase C — Module structure + SDBL

### Module structure (§3)

| Slice | Description | Commit |
|---|---|---|
| §3 Slice 1 | `module_structure` single-owner facts (`canonical_alias`, `standard_regions`, significant predicates) + handler migrations (`DuplicateRegion`, `NonStandardRegion`, `CodeOutOfRegion`, `RegionTree::is_region_empty`) | `effab845` |
| §3.4 | `EmptyRegion` migration to `RegionTree` | `00f52b2f` |
| §3 Slice 2 | `RegionTree` module-level filter; removal of `module_level_regions_query` (master plan §8.6) | `d32f55d9` |

### SDBL (§4)

| Slice | Description | Commit |
|---|---|---|
| §4 audit pass | Track A/B/C confirmation across 20 SDBL handlers | task #84 |
| §4 Slice 1 | `QueryNestedFieldsByDot` `minPathDepth` config + schema | `329802ad` |
| §4 Slice 2 | `AssignAliasFieldsInQuery` covers UNION parts uniformly | `5efc5362` |
| §4 Slice 3 | `JoinWithSubQuery` exempts aggregating subqueries (function-call-positioned scan) | `c3cd1917` |
| §4 Slice 4 | Collapse `UsingLikeInQuery` + `IncorrectUseLikeInQuery` into `SdblDiagnostic::LikeUsage { kind }` | `f65d5446` |
| §4 delta-audit | `FieldsFromJoinsWithoutIsNull`, `LogicalOrInTheWhereSectionOfQuery` — closed without code work | — |
| §4 Track A | 13 cards confirmed stable: `BadWords`, `CreateQueryInCycle`, `JoinWithVirtualTable`, `LogicalOrInJoinQuerySection`, `MultilineStringInQuery`, `QueryParseError`, `QueryToMissingMetadata`, `SelectTopWithoutOrderBy`, `UnionAll`, `VirtualTableCallWithoutParameters`, `FullOuterJoinQuery`, `NestedConstructorsInStructureDeclaration`, `RefOveruse` (deferred to Track 6) | — |

## Phase D — Transactions

| Slice | Description | Commit |
|---|---|---|
| §2.1 + §2.2 | Catch-body classifier in `hir-def/catch_class.rs` + `MissingCodeTryCatchEx` migration to classifier-driven dispatch | `40403b45` |
| §2.3 mini-fix + closure annotations | `#[ignore]`'d preprocessor regression test + Track 2 closure annotations on transaction-cluster cards | `3d75ff2b` |
| §2.3 mini-fix follow-up | Fixture corrected to BSL-safe both-branches form (single-branch was a true positive, not a gap) | `0d009baf` |

## Phase E — Closure

| Slice | Description | Commit |
|---|---|---|
| E1 | Per-card Track 2 closure annotations on ~46 audit-cards | this commit |
| E2 | Master `TRACK_2_CLOSURE.md` (this document) | this commit |
| E3 | Corpus-level perf measurements (Track 1 baseline vs HEAD) | **pending separate session** |

## Acceptance gate (master plan §8)

| Gate | Status |
|---|---|
| `cargo test --workspace` зелёный без `#[ignore]` без обоснования | ✓ verified each commit (pre-commit hook). The single `#[ignore]`'d test (`begin_in_preproc_then_try_outside`) carries an explicit `Track 6 dep` rationale string. |
| `cargo clippy --all-targets --all-features -- -D warnings` чистый | ✓ pre-commit hook |
| ~30 §Context cards have «закрыто Track 2» markers | ✓ all §Context cards present in `docs/diagnostics-audit/` carry the marker (E1). `MissingFunctionDescription` / `MissingProcedureDescription` cards are absent from the audit-dir; nothing to annotate. The four cluster-adjacent transaction cards outside §Context (`CommitTransactionOutsideTryCatch`, `PairingBrokenTransaction`, `WrongUseOfRollbackTransactionMethod`, `TryNumber`) are also marked with explicit «out-of-scope» annotations so the transactions cluster reads consistently. |
| Single-source security registry (§8.4) | ✓ all 10 security handlers consume `bsl_platform::security::registry`. |
| Single-source complexity visitor (§8.5) | ✓ legacy `cognitive_complexity.rs` / `cyclomatic_complexity.rs` modules removed (`71c22eab`). |
| Single `RegionTree` access (§8.6) | ✓ `module_level_regions_query` removed (`d32f55d9`). |
| CFE-aware tests for security cluster (§8.7) | ✓ §1.7-A fixture (`f0a45e29`). |
| Performance budgets (§8.8) | **pending Phase E3.** |
| Documentation (§8.9) | partial: closure cards (E1) + this doc (E2) + module-doc on each new Salsa query are in repo. The plan file itself was kept in a private workspace path rather than copied to `docs/diagnostics-audit/roadmap/track-2.md` — the in-repo equivalent is this closure doc. |
| `// TODO(...)` markers in migrated handlers removed (§8.10) | ✓ implicit in migrations. |

## Known limitations (forwarded)

### Track 4 — quick-fixes

- **PublicMethodsDescription** «export outside region — emit всё равно»
  (master plan §5.3 audit gap): not closed in Track 2; deferred to a
  Track 4 quick-fix slice once UX wording for region-violation messages
  is decided.

### Track 6 — preprocessor + cross-module + cascade-suppression

- **`BeginTransactionBeforeTryCatch`** preprocessor-aware Begin/Try
  matching: `НачатьТранзакцию()` outside, every active branch of an
  `#Если/#Иначе` block starting with `Попытка` — currently a false
  positive. Regression test `begin_in_preproc_then_try_outside` is
  `#[ignore]`'d with reason `Track 6 dep: preprocessor-aware Begin/Try
  matching`.
- **`RefOveruse`** reclassification: metadata-blocked stub remains in
  Track 2; full reclassification needs ground-truth on `Ссылка`
  semantics in JOIN/WHERE contexts.
- **`TempFilesDir` ↔ `FileSystemAccess` double-detection**: cascade-
  suppression scope; documented as known-limitation in card-level docs.
- **Custom logging-registry extension hook** for projects with their
  own logging wrappers: would loosen the conservative `Silent`
  classification in `MissingCodeTryCatchEx`.
- **Inter-procedural detector** for «user-defined method guarantees
  re-raise/log»: same area; would shrink the `Silent` set further.
- **Endpoint/method analysis** for `InternetAccess`; **argument/taint**
  analysis for `ExternalAppStarting` (registry already carries
  `Role::Cmd` / `Role::Path` / `Role::Url` for this future pipeline).
- **`OSUsersMethod`** contextual usage analysis: passing the result to
  ACL APIs, comparing with constants, etc. — currently the diagnostic
  fires on every call regardless of usage shape.
- **`SetPrivilegedMode`** inter-procedural transparency for calls
  through arbitrary variables without a known value: the intra-method
  `value_state` overlay (Phase A §1.3) handles the local-literal case;
  cross-call propagation through parameters / globals would require a
  full inter-procedural const-prop pipeline.

### Track 6 — cross-module SCC follow-ups (pre-existing)

- **Cross-module SCC walk for `is_recursive` flagging** in
  `effect_summary` (task #50) — **STILL OPEN**. Re-scoped after
  recon (2026-05-11): proper fix requires either a workspace-wide
  modules Salsa input or per-method DFS with custom cycle handling
  (~300-800 LOC), since Salsa 0.26's `cycle_fn` only fires on cycle
  head and the codebase has no workspace-aggregator precedent. The
  single downstream consumer is `cognitive_complexity.rs:141`
  (+1 bonus). Most-common false-negative (self-recursion via
  ЭтотОбъект.foo()) was eliminated by closing #51 below. Remaining
  gap is cross-module mutual recursion (A→B→A), rare in BSL.
- **`ЭтотОбъект.method()` normalization** in call-graph extractor
  (task #51) — **CLOSED 2026-05-11, commit `9b5b82aa`**.
  `field_callee_to_edge` now detects `ЭтотОбъект` / `ThisObject` in
  the qualifier and emits `DirectLocal` (or `ThisObjectMethod` if
  the local method isn't resolvable). The `is_recursive` known-
  limitation note in `crates/ide-db/src/effects.rs:46-56` was
  rewritten to reflect this fix.
- **Pre-lowercase registry hot-path lookups** (task #52) —
  **CLOSED 2026-05-11, commit `c41599f6`**. New `lookup_global_lc`
  / `lookup_constructor_lc` methods on `SecurityRegistry` accept
  caller-provided already-lowercase `&str`; internal map split into
  separate globals/constructors `FxHashMap<String, usize>` so
  `.get(&str)` uses `String: Borrow<str>` zero-alloc. Hot callers
  in `dataflow::effect_summary` and `dataflow::security_state`
  migrated. Convenience methods retained.

Originally all three were carried forward from before Track 2;
#51 and #52 are now closed. #50 remains open and is owned by
Track 6 (or a dedicated follow-up if a cross-module SCC need
materialises sooner).

## Pointers

- Per-card closure annotations: `## Закрыто Track 2` section at the end
  of each affected card in `docs/diagnostics-audit/`.
- Phase E3 perf scaffolding: `scripts/benchmark*.sh`.
