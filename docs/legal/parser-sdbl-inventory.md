# Parser / SDBL Inventory

## Purpose

This document is a concrete inventory of grammar-related files and parser/SDBL
test fixtures that matter for provenance and future relicensing.

It is intentionally operational: each entry includes a likely risk level and a
recommended next step.

## Inventory rules

Risk labels used here:

- `high`: likely derived-risk or needs line-by-line audit
- `medium`: structurally original, but depends on unaudited grammar/parser layer
- `low`: favorable provenance signal or already externally licensed

## Grammar-related source files

### Core parser grammar files

| File | Lines | Scope | Current risk | Notes | Recommended action |
|---|---:|---|---|---|---|
| `crates/parser/src/grammar/expressions.rs` | 414 | BSL expression grammar | `high` | Part of hand-written parser, but the whole BSL grammar layer was explicitly developed with `bsl-parser` as a reference source in early planning | Audit against `BSLParser.g4`, then classify as rewrite-needed or independently expressed |
| `crates/parser/src/grammar/items.rs` | 290 | BSL items: procedures, functions, params | `high` | Same as above; architecture is original, grammar provenance is not yet clean-room | Audit against `BSLParser.g4` |
| `crates/parser/src/grammar/statements.rs` | 469 | BSL statements and block structure | `high` | Same general parser-layer risk; likely mixed original recovery logic plus grammar-derived structure | Audit against `BSLParser.g4` |
| `crates/parser/src/grammar/sdbl.rs` | 117 | SDBL root grammar entry points | `high` | Earlier history explicitly referenced `SDBLParser.g4 from bsl-parser`; current file is short but provenance is explicit | Treat as grammar-derived until rewritten from spec |
| `crates/parser/src/grammar/sdbl/expressions.rs` | 1108 | SDBL expression grammar | `high` | Large grammar surface; almost certainly the most sensitive SDBL grammar file after `select.rs` | Line-by-line audit vs `SDBLParser.g4` |
| `crates/parser/src/grammar/sdbl/select.rs` | 1430 | SDBL SELECT/FROM/JOIN/GROUP/ORDER grammar | `high` | Contains explicit ANTLR references in comments and mirrors broad SDBL grammar behavior | Highest-priority grammar rewrite candidate |

### Lexer / token layer

| File | Lines | Scope | Current risk | Notes | Recommended action |
|---|---:|---|---|---|---|
| `crates/lexer/src/sdbl.rs` | 990 | SDBL token inventory and regex-based lexer | `high` | Large bilingual token catalog likely tracks `SDBLLexer.g4`; even if keywords themselves are uncopyrightable, token selection and organization likely came from upstream grammar work | Audit token inventory, then plan clean-room rebuild from 1C docs/spec |
| `crates/parser/src/sdbl_token_converter.rs` | 203 | Adapter from SDBL lexer tokens to parser tokens | `medium` | Mostly local adapter logic; depends directly on unaudited SDBL token set | Keep coupled to lexer audit; lower priority than lexer/grammar files |
| `crates/parser/src/lib.rs` | 188 | Parse entry points for BSL and SDBL | `medium` | Glue/integration layer is local, but SDBL entry points sit on top of grammar-derived code | No urgent rewrite; revisit after parser cleanup |
| `crates/parser/src/syntax_kind.rs` | 295+ | Syntax kind mapping for parser events/tokens | `medium` | Local Rowan mapping layer; structure follows local architecture, not ANTLR directly | Likely permissive candidate later, but blocked by parser audit |

### SDBL semantic layer

| File group | Scope | Current risk | Notes | Recommended action |
|---|---|---|---|---|
| `crates/sdbl-hir/src/lib.rs` | Public semantic API | `medium` | Local semantic/HIR abstractions; not obviously copied from `bsl-parser` | Candidate for future permissive bucket |
| `crates/sdbl-hir/src/lower/*.rs` | Lowering, scope, source map, diagnostics | `medium` | Looks materially more original than parser layer, but depends on parser/grammar provenance | Keep separate from parser in future licensing plan |
| `crates/sdbl-hir/src/standard_fields.rs`, `types.rs`, `scope.rs` | Semantic metadata model | `medium` | Mostly local semantic code | Review after parser layer is untangled |

## Upstream grammar reference files

These are the concrete upstream files currently most relevant for comparison:

| Upstream file | Role | License signal |
|---|---|---|
| `../bsl-parser/src/main/antlr/BSLLexer.g4` | BSL lexer grammar | `LGPL-3.0-or-later` |
| `../bsl-parser/src/main/antlr/BSLParser.g4` | BSL parser grammar | `LGPL-3.0-or-later` |
| `../bsl-parser/src/main/antlr/SDBLLexer.g4` | SDBL lexer grammar | `LGPL-3.0-or-later` |
| `../bsl-parser/src/main/antlr/SDBLParser.g4` | SDBL parser grammar | `LGPL-3.0-or-later` |

## Test fixtures on disk

### Real fixture files

| File | Lines | Used by | Current risk | Notes | Recommended action |
|---|---:|---|---|---|---|
| `crates/parser/tests/fixtures/Module.bsl` | 15156 | `crates/parser/src/lib.rs`, `crates/parser/tests/integration_tests.rs` | `low` | Contains explicit `CC BY 4.0` header from `ООО 1С-Софт`; provenance is external and visible in the file itself | Keep, but record external-license provenance if reused elsewhere |
| `crates/parser/tests/fixtures/user_query_with_highlighting_issue.sdbl` | 132 | `crates/parser/tests/sdbl_parser_tests.rs` | `medium` | Introduced later in local history; not found in local `../bsl-parser` tree during this audit; likely local or independently sourced, but still worth manual review | Manual provenance check and optional rewrite if any upstream match appears later |

### Test files with inline fixtures

These files are not “fixture files” in the filesystem sense, but they contain a
large amount of embedded test data and therefore must be part of the inventory.

| File | Lines | Fixture style | Current risk | Notes | Recommended action |
|---|---:|---|---|---|---|
| `crates/parser/tests/integration_tests.rs` | 663 | Mostly inline BSL snippets + `Module.bsl` includes | `medium` | General parser coverage; many small examples likely local, but should be sampled for upstream overlap | Sample audit first, then classify |
| `crates/parser/tests/sdbl_parser_tests.rs` | 2160 | Heavy inline SDBL fixtures + one included `.sdbl` file | `high` | This is the densest SDBL acceptance corpus; even if not copied verbatim, it is the most likely place where upstream grammar examples shaped current tests | Highest-priority test audit target |
| `crates/sdbl-hir/src/lower/tests.rs` | 2765 | Heavy inline SDBL fixtures | `medium` | More semantic than grammatical; likely more original than parser tests, but still depends on parser behavior and may contain inherited examples | Audit after `sdbl_parser_tests.rs` |
| `crates/parser/src/grammar/sdbl/select.rs` tests | embedded | Inline parser recovery fixtures | `high` | Grammar-adjacent tests live beside parser code and are tightly coupled to the highest-risk file | Audit together with `select.rs` |

## Provenance signals already found

### Strong signals of upstream dependence

- Initial planning document explicitly lists `bsl-parser` grammar files as source
  material for parser implementation.
- Early `crates/parser/src/grammar/sdbl.rs` explicitly stated
  `Grammar reference: SDBLParser.g4 from bsl-parser`.
- Current parser code still contains comments such as
  `ANTLR grammar has...`.

### Favorable signals

- Parser architecture is local and event-based, not ANTLR-generated.
- `sdbl-hir` is a semantic layer with local abstractions, not a parse-tree wrapper.
- `Module.bsl` has a visible external license notice.
- `user_query_with_highlighting_issue.sdbl` was not found in the sibling
  `../bsl-parser` tree during this audit.

## Suggested audit order

1. `crates/parser/src/grammar/sdbl/select.rs`
2. `crates/parser/src/grammar/sdbl/expressions.rs`
3. `crates/lexer/src/sdbl.rs`
4. `crates/parser/tests/sdbl_parser_tests.rs`
5. `crates/sdbl-hir/src/lower/tests.rs`
6. BSL parser files:
   - `crates/parser/src/grammar/expressions.rs`
   - `crates/parser/src/grammar/statements.rs`
   - `crates/parser/src/grammar/items.rs`

## Working conclusion

At this stage, the inventory supports the following split:

- parser grammar files and SDBL lexer/token inventory:
  likely still in the copyleft-risk bucket
- parser entrypoint/glue files:
  secondary risk, dependent on grammar audit
- `sdbl-hir` semantic files:
  substantially better candidates for future permissive licensing
- disk fixtures:
  mixed provenance, with `Module.bsl` favorable and `sdbl_parser_tests.rs` the
  highest-value audit target
