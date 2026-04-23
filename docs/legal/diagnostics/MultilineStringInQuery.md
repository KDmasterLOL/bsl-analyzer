# Provenance: MultilineStringInQuery

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule exists

This diagnostic is a practical query-correctness rule.

The core idea is straightforward: a multi-line string literal inside SDBL query
text is unusual and often means that double quotes were escaped incorrectly.
One common mistake is using `""` where the query language expects `""""` for an
empty string.

There is no strong direct normative `v8std` source for this exact rule.
`v8std.ru` exposes it as a `bslls` diagnostic without a direct standard mapping.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/multiline_string_in_query.rs`
is local and split into two parts:

- SDBL lowering reports candidate multi-line string nodes;
- the handler applies an additional local filter by scanning the original query
  text and rejecting obvious false positives.

This local filtering logic is specific to the current parser/mapper behavior and
is not a literal restatement of an upstream textual rule.

### Documentation

RU/EN documentation was rewritten during this audit to describe the practical
query behavior directly and to avoid inheriting placeholder wording.

### Tests

Current tests are local inline fixtures that cover:

- accidental multi-line strings created by `""` inside query text;
- valid string literals in `CASE`;
- correct empty-string escaping with `""""`.

The fixtures are embedded directly in the Rust test module.

## Important caveat

Although the rule itself is clean, its implementation depends on the current
SDBL parser and `sdbl_hir` pipeline, which is still being audited separately.

## Remaining caveats

- repository-wide relicensing still depends on the broader SDBL/parser audit;
- repository history may still contain earlier wording closer to upstream docs.

## Conclusion

`MultilineStringInQuery` looks like a good permissive candidate at the rule and
documentation level, but final confidence for the implementation still depends
on the broader SDBL audit.
