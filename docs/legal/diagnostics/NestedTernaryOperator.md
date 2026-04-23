# Provenance: NestedTernaryOperator

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic readability rule.

The core idea is straightforward: nested ternary expressions and ternaries used
inside branch conditions make control flow harder to read and reason about.
That idea is common across many languages and is not tied to a unique 1C
standard.

There is no strong direct normative `v8std` source for this exact rule.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/nested_ternary_operator.rs`
is local and syntax-based:

- it scans `IF_STMT` and `ELSIF_CLAUSE` conditions for ternary expressions;
- it reports nested ternaries inside another `TERNARY_EXPR`;
- it builds the diagnostic message locally.

This strongly favors permissive treatment because the implementation is a local
AST pattern check over a generic readability concern.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current
behavior directly in local wording.

### Tests

Current tests are local inline fixtures covering:

- simple ternaries that should not trigger;
- nested ternaries in assignments;
- ternaries in `If` conditions;
- ternaries in `ElseIf` conditions;
- disabling the diagnostic through config.

The test corpus is embedded directly in the Rust module.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NestedTernaryOperator` is a strong permissive candidate because it is a generic
readability rule with local AST-based implementation, local tests, and now-local
documentation.
