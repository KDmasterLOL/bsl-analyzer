# Provenance: OrderOfParams

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std640` ("Parameters of procedures and functions") supports the general
recommendation that optional parameters should follow required ones. The rule is
therefore directly grounded in a published 1C guideline.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/order_of_params.rs`
is local and straightforward:

- it reads method parameters from the local item tree;
- it finds the first optional parameter;
- it reports each required parameter that appears after that point.

This strongly favors permissive treatment because the handler is a small local
implementation of a public parameter-ordering rule.

### Documentation

RU/EN documentation was rewritten during this audit to point directly to
`#std640` and to describe current behavior in local wording.

### Tests

Current tests are local inline fixtures covering:

- methods without parameters;
- all-required and all-optional parameter lists;
- correct ordering;
- one misplaced required parameter;
- multiple required parameters after optional ones.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`OrderOfParams` is a strong permissive candidate because it directly implements
a public 1C parameter-ordering recommendation through local item-tree analysis,
with local tests and now-local documentation.
