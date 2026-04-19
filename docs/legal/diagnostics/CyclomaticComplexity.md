# Provenance: CyclomaticComplexity

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is based on a well-known general software metric rather than a
1C-specific standard.

Public supporting sources:

- McCabe-style cyclomatic complexity references
- PDepend metric documentation

The rule measures the number of independent execution paths in a method and is
commonly used to flag overly branched code.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/cyclomatic_complexity.rs` is local and
HIR-based:

- threshold filtering happens in local diagnostic code;
- the actual complexity value is calculated through local `hir` machinery;
- the code is integrated with local editor features such as code lenses.

This favors permissive treatment because the metric itself is general and the
active implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited
wording and to explain the metric in project-local language.

### Tests

Current tests are local and inline. During this audit, the previous
high-complexity stress fixture was replaced with a new local example method
that still verifies complexity `21` and the diagnostic threshold behavior.

Covered scenarios include:

- simple function below threshold;
- `Else` branch accounting;
- threshold exceedance;
- direct complexity calculation through local HIR.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because the metric
  is standard and widely reused;
- deeper provenance of the underlying HIR complexity algorithm still depends on
  the broader audit of `hir` and related infrastructure;
- repository history may still contain earlier upstream-aligned wording or test
  material.

## Conclusion

`CyclomaticComplexity` is a reasonable permissive candidate because:

- the rule is based on a general public metric rather than a proprietary idea;
- the active implementation is local and HIR-based;
- the current docs and stress tests no longer require retaining copyleft
  treatment on their face.
