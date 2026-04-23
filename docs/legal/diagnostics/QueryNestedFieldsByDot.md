# Provenance: QueryNestedFieldsByDot

## Status

Candidate for `MIT OR Apache-2.0`, with an SDBL implementation caveat.

## Why this rule exists

This diagnostic has a strong public technical rationale.

Guidance around dereference of reference fields in query language and the
performance cost of implicit joins is publicly documented in 1C materials such
as `#std654`. The underlying rule concept therefore does not depend on a unique
upstream invention.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/query_nested_fields_by_dot.rs`
is local at the handler layer:

- `sdbl_hir` lowering produces `QueryNestedFieldsByDot` diagnostics for
  matching query fragments;
- the handler maps those local SDBL diagnostics back to source ranges and emits
  the user-facing warning.

The concrete matching logic currently covers ordinary nested field access,
virtual-table parameters, and dereference after `ВЫРАЗИТЬ`.

### Documentation

RU/EN documentation was rewritten during this audit to describe the actual
covered cases and to point to related public 1C guidance in local wording.

### Tests

Current tests are local and fairly broad. They cover:

- nested fields in `SELECT`;
- nested fields in `WHERE`;
- nested fields in `JOIN`;
- virtual-table parameters;
- dereference after `ВЫРАЗИТЬ`;
- negative cases for MDO type paths and simple two-part paths.

The test suite is embedded directly in the Rust module.

## Important caveat

Although the rule itself looks clean, the implementation depends on the current
SDBL parser and `sdbl_hir` lowering pipeline, which are being audited
separately.

## Remaining caveats

- repository-wide relicensing still depends on the broader SDBL/parser audit.

## Conclusion

`QueryNestedFieldsByDot` looks like a good permissive candidate at the rule and
documentation level, but final confidence for the implementation still depends
on the broader SDBL audit.
