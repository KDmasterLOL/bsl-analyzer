# Parser Architecture Map

> **Layer map still accurate; risk commentary is stale.** The architectural
> separation described here holds. The open question it poses — how much of the
> grammar-expression layer still reflects upstream work — has since been
> answered for SDBL in `sdbl-provenance-2026-07-audit.md`.

## Purpose

This note separates the `crates/parser` codebase into architectural layers for
licensing and provenance analysis.

The key point is simple:

- `parser` is **not** one uniform risk blob;
- the local event-parser architecture looks materially original;
- the grammar-expression layer remains the main likely derivative-risk area.

## Layer 1: local parser infrastructure

These files implement the parser framework rather than the language grammar
itself.

| File | Lines | Role | Current assessment |
|---|---:|---|---|
| `crates/parser/src/event.rs` | 147 | event stream and node kinds | strongly local |
| `crates/parser/src/parser.rs` | 559 | parser state machine, markers, forward parents, loop guards | strongly local |
| `crates/parser/src/parser/input.rs` | 624 | значимые токены входа, карта переводов строк, алфавит грамматики `Sig` | strongly local |
| `crates/parser/src/sink.rs` | 259 | Rowan sink from events to syntax tree | strongly local |
| `crates/parser/src/parser/token_set.rs` | 151 | recovery/token-set utility | strongly local |
| `crates/parser/src/syntax_kind.rs` | 264 | mapping to syntax kinds | local, but tied to parser output model |
| `crates/parser/src/lib.rs` | 194 | public entry points and glue | local glue; depends on grammar layer |

### Why this layer looks original

- event-based parser design is local and rust-analyzer-style;
- marker / completed-marker / forward-parent machinery is local parser
  infrastructure;
- Rowan sink integration is specific to this codebase;
- infinite-loop guards and progress tracking are local implementation choices.

### Licensing implication

This layer is **not** where the main copyleft risk currently lives.

Even if some design inspiration came from `rust-analyzer`, that does not create
the same problem as grammar derivation from `bsl-parser`, because
`rust-analyzer` is already permissively licensed.

## Layer 2: BSL grammar layer

These files define the hand-written BSL grammar and recovery behavior.

| File | Lines | Role | Current assessment |
|---|---:|---|---|
| `crates/parser/src/grammar.rs` | 424 | top-level source file grammar + preprocessor blocks | high provenance risk |
| `crates/parser/src/grammar/items.rs` | 290 | procedures, functions, variables, params | high provenance risk |
| `crates/parser/src/grammar/statements.rs` | 469 | statements and block structure | high provenance risk |
| `crates/parser/src/grammar/expressions.rs` | 414 | BSL expressions | high provenance risk |

### Why this layer is risky

- earlier project planning explicitly cited `bsl-parser` grammar files as source
  material;
- these files express the language grammar directly, which is where derivative
  structure matters most;
- even with an original event-parser architecture, the grammar decomposition may
  still follow upstream ANTLR design closely.

### Favorable nuance

This layer likely also contains real local value:

- recovery behavior;
- node shaping for diagnostics/IDE use;
- compromises made for lossless parsing and incremental tooling.

That means a future clean-room rewrite should try to preserve the architecture
and recovery ideas while rewriting the grammar expression itself from primary
sources.

## Layer 3: SDBL grammar layer

These files are the most sensitive grammar-related area.

| File | Lines | Role | Current assessment |
|---|---:|---|---|
| `crates/parser/src/grammar/sdbl.rs` | 117 | SDBL entry points | high provenance risk |
| `crates/parser/src/grammar/sdbl/select.rs` | 1430 | SELECT/FROM/JOIN/ORDER/TOTALS grammar | highest-risk file |
| `crates/parser/src/grammar/sdbl/expressions.rs` | 1108 | SDBL expressions and predicates | highest-risk file |

### Why this layer is the hardest blocker

- SDBL work was explicitly tied to `bsl-parser` in early history;
- current comments still mention ANTLR grammar assumptions;
- these files are large grammar-expression modules, where structural derivation
  is most plausible;
- many downstream diagnostics depend on this layer.

### Practical conclusion

If the repository wants a gradual path to `MIT OR Apache-2.0`, this layer is a
better target for clean-room rewrite than almost anything else in the workspace.

## Layer 4: SDBL token adaptation

| File | Lines | Role | Current assessment |
|---|---:|---|---|
| `crates/parser/src/sdbl_token_converter.rs` | 216 | adapter from SDBL lexer tokens to parser tokens | medium risk |

This file is mostly local adapter code, but it mirrors the SDBL token universe
coming from the lexer. So it is not the primary blocker, yet it cannot be
considered fully clean until the lexer/token inventory is cleaned up.

## Layer 5: tests and fixtures

| File | Lines | Role | Current assessment |
|---|---:|---|---|
| `crates/parser/tests/integration_tests.rs` | 752 | broad BSL parser coverage | mixed |
| `crates/parser/tests/sdbl_parser_tests.rs` | 2160 | dense SDBL acceptance corpus | high audit value |
| `crates/parser/tests/fixtures/Module.bsl` | 15156 | large external fixture | favorable, externally licensed |
| `crates/parser/tests/fixtures/user_query_with_highlighting_issue.sdbl` | 132 | small SDBL fixture | likely local, still reviewable |

### Key distinction

Tests are not automatically “safer” than parser code. In this crate, the SDBL
test corpus is large enough that copied or translated acceptance examples would
still matter for provenance.

## Licensing takeaway by layer

### Best future permissive candidates

- `event.rs`
- `parser.rs`
- `sink.rs`
- `token_set.rs`
- much of `lib.rs`

### Likely needs dedicated rewrite or proof

- `grammar.rs`
- `grammar/items.rs`
- `grammar/statements.rs`
- `grammar/expressions.rs`
- `grammar/sdbl.rs`
- `grammar/sdbl/select.rs`
- `grammar/sdbl/expressions.rs`
- `lexer/src/sdbl.rs` (outside this crate, but directly relevant)

### Depends on surrounding cleanup

- `sdbl_token_converter.rs`
- syntax-kind glue
- parser tests, especially SDBL-heavy ones

## Recommended next move

For parser licensing work, the most efficient path is:

1. treat parser infrastructure as a likely-local asset;
2. treat grammar-expression files as the real audit/rewrite target;
3. keep focusing first on SDBL grammar and lexer support;
4. only after that, audit or rewrite BSL grammar files.

## Bottom line

The parser crate already contains a lot of local implementation value.

The licensing problem is not “hand-written parser bad”; it is much narrower:
the main unresolved question is how much of the current grammar-expression layer
still reflects upstream `bsl-parser` grammar work.
