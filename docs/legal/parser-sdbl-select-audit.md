# Audit: `crates/parser/src/grammar/sdbl/select.rs`

> **Superseded on current state — retained as historical record.** Written in
> April 2026 by reasoning from rule names, without access to the upstream
> grammar. Its premise — that the file was written against the upstream grammar
> text — is now proven from this repository's own history. Its assessment of the
> file predates Slices 6 through 11; the rule bodies it describes as
> grammar-derived have since been rewritten and now differ structurally from
> upstream. See `sdbl-provenance-2026-07-audit.md`.

## Goal

This note evaluates `crates/parser/src/grammar/sdbl/select.rs` as a provenance
target and separates:

- parts that look strongly grammar-derived from `bsl-parser` / `SDBLParser.g4`
- parts that look materially original to this repository
- parts that are mixed and should be handled pragmatically

## High-level conclusion

`select.rs` is **not** a line-by-line port of the ANTLR file, but it is still a
high-risk grammar-derived file.

Reason:

- the function set and rule boundaries closely track `SDBLParser.g4`;
- many function names map almost one-to-one to upstream grammar rules;
- comments explicitly refer to ANTLR grammar behavior in multiple places;
- the file still expresses the grammar in the same semantic decomposition, even
  though the parser architecture is completely different.

At the same time, the file also contains a substantial amount of local,
repository-specific logic:

- event-based parsing structure;
- Rowan node production;
- explicit error recovery;
- incomplete-code handling for IDE scenarios;
- simplifications accepted for error recovery rather than exact grammar fidelity.

So the right conclusion is:

- **grammar structure: likely derived**
- **parser architecture and recovery behavior: materially original**

## Concrete rule/function mapping

The following parts in `select.rs` align closely with rule boundaries in
`../bsl-parser/src/main/antlr/SDBLParser.g4`:

| Local function | Upstream grammar rule | Assessment |
|---|---|---|
| `select_query` | `selectQuery` | strongly grammar-derived |
| `subquery` | `subquery` | strongly grammar-derived |
| `union_clause` | `union` | strongly grammar-derived |
| `query` | `query` | strongly grammar-derived |
| `selected_fields` | `selectedFields` | strongly grammar-derived |
| `selected_field` | `selectedField` | strongly grammar-derived |
| `asterisk_field` | `asteriskField` | strongly grammar-derived |
| `alias` | `alias` | strongly grammar-derived |
| `from_clause` | `FROM dataSources` / `dataSources` | strongly grammar-derived |
| `data_source` | `dataSource` | strongly grammar-derived |
| `table_ref` | `table` + VT-related source forms | strongly grammar-derived, but locally adapted |
| `where_clause` | `WHERE logicalExpression` | strongly grammar-derived |
| `join_clause` | `joinPart` | strongly grammar-derived |
| `limitations` | `limitations` | strongly grammar-derived |
| `top_clause` | `top` | strongly grammar-derived |
| `group_by_clause` | `GROUP BY groupByItem` | grammar-derived but simplified |
| `order_by_clause` | `orderBy` | grammar-derived but simplified |
| `order_by_item` | `ordersByExpession` | grammar-derived but simplified |
| `having_clause` | `HAVING logicalExpression` | strongly grammar-derived |
| `for_update_clause` | `FOR UPDATE` | strongly grammar-derived |
| `index_by_clause` | `INDEX BY indexingItem` | strongly grammar-derived |
| `autoorder_clause` | `AUTOORDER` | strongly grammar-derived |
| `totals_by_clause` | `totalBy` / `totalsGroup` | grammar-derived but heavily simplified |

## What looks most derived

These parts should be assumed to remain in the copyleft-risk bucket unless
rewritten from an independent specification:

### 1. Rule decomposition

The file decomposes the parser almost exactly along the same rule boundaries as
the upstream grammar:

- `selectQuery`
- `subquery`
- `union`
- `query`
- `selectedFields`
- `selectedField`
- `alias`
- `joinPart`
- `limitations`
- `top`
- `orderBy`
- `totalBy`

That alone does not prove infringement, but together with the documented project
history it is strong evidence of derivation.

### 2. Same grammar concepts with the same special cases

Examples:

- `JOIN` without explicit type treated as implicit inner join
- permutations of `AUTOORDER`, `ORDER BY`, `TOTALS BY`
- `AS? identifier` alias shape
- `TOP`, `DISTINCT`, `ALLOWED` limitation handling
- explicit handling of `GROUP BY`, `ORDER BY`, `HAVING`, `FOR UPDATE`, `INDEX BY`

These are all language features, but the way they are split into local parsing
units aligns closely with the upstream grammar work.

### 3. Explicit upstream-reference comments

Current file still contains comments such as:

- `Note: In ANTLR grammar, JOIN alone defaults to INNER JOIN`
- `ANTLR grammar has all permutations, but we simplify...`

These comments are not fatal by themselves, but they confirm that the file was
written against the upstream grammar text, not purely from primary 1C language
documentation.

## What looks materially original

These parts look like genuine local implementation work and are the strongest
evidence that the file is not just a textual port:

### 1. Event-based parser structure

The whole file is written around local parser primitives:

- `Parser`
- `NodeKind`
- `start() / complete() / abandon()`
- Rowan-compatible event generation

This is a local architectural decision, not inherited from ANTLR.

### 2. Error recovery functions

These functions look especially local and IDE-oriented:

- `recover_field_to_alias_or_delimiter`
- `recover_to_delimiter_vt`
- `is_data_source_start`
- `is_field_start`
- `is_clause_keyword`
- `is_join_keyword`

They encode recovery strategy for incomplete code and editor interaction, not
just formal grammar acceptance.

### 3. Simplification choices

The file repeatedly simplifies or normalizes grammar behavior rather than trying
to preserve ANTLR structure exactly.

Examples:

- `limitations()` accepts keywords in any order instead of reproducing all
  explicit permutations from the ANTLR grammar;
- `totals_by_clause()` is intentionally simplified;
- `group_by_clause()` and `order_by_clause()` are parsed in simpler local forms;
- `table_ref()` mixes language parsing with local VT-parameter recovery and
  missing-argument nodes;
- `selected_field()` contains local incomplete-expression recovery.

These are good signs for future rewrite strategy, because they show where the
current file already departs from upstream expression.

## Practical rewrite strategy

The best path is not to argue that `select.rs` is already clean. It probably is
not clean enough.

Instead, split it into two conceptual buckets:

### Bucket A: reusable local ideas

These ideas can be preserved in a rewrite:

- event-based parsing architecture
- local `NodeKind` structure
- recovery approach for incomplete field lists
- recovery around aliases and VT parameters
- permissive acceptance of partial/incomplete queries for IDE workflows

### Bucket B: grammar-derived content

These parts should be considered rewrite targets:

- top-level SDBL SELECT rule decomposition
- subquery / union / query structure
- alias/data-source/join clause decomposition
- clause-order and limitation handling derived from upstream grammar references

## Recommended classification

For present-day licensing analysis:

- file status: `high-risk grammar-derived`
- not a good permissive candidate in current form
- good candidate for targeted clean-room rewrite because its architecture can be
  preserved while the grammar expression is reauthored

## Best next step

If the goal is to make concrete progress toward permissive licensing, the most
useful next task is:

1. keep the current behavior contract;
2. write an independent mini-spec for the SELECT grammar using official 1C docs
   plus observed tests;
3. then rewrite `select.rs` from that mini-spec without consulting upstream
   grammar text during implementation.

That would give the highest legal leverage for the SDBL parser layer.
