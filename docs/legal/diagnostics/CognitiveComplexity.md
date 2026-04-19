# Provenance: CognitiveComplexity

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is based on the public Cognitive Complexity specification from
SonarSource rather than on a 1C-specific or `bsl-language-server`-specific
original rule.

Primary source:

- SonarSource, `Cognitive Complexity` specification v1.4

## Audit result

### Production code

The current implementation is expressed through local HIR lowering and local
complexity calculation code:

- `crates/hir-def/src/cognitive_complexity.rs`
- `crates/ide-diagnostics/src/handlers/cognitive_complexity.rs`

This supports permissive treatment because:

- the metric itself is documented in a public specification;
- the calculation is implemented on top of local HIR structures;
- the diagnostic handler only applies threshold filtering and message creation.

### Documentation

Both public documentation pages were rewritten during this audit to describe the
rule directly from the SonarSource specification instead of reusing inherited
long-form text and examples.

### Tests

The previous large fixture explicitly mirrored an upstream diagnostic sample.
During this audit it was replaced with a new local function that still stresses:

- nested branching;
- nested loops;
- `ElseIf` / `Else` handling;
- `While` in a nested branch;
- threshold-based reporting.

The expected complexity value was updated to match the new local fixture.

## Remaining caveats

- repository history may still contain earlier upstream-aligned texts;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CognitiveComplexity` is a strong permissive candidate because the underlying
rule comes from a public non-copyleft specification and the current
implementation and tests can be expressed independently from `bsl-language-server`.
