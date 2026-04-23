# Provenance: MagicNumber

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic maintainability rule.

Hard-coded numeric literals are a common static-analysis smell because they hide
meaning and make future changes harder. This idea is generic and not tied to a
specific 1C standard.

There is no direct normative 1C standard source for this exact rule.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/magic_number.rs`
is clearly local and fairly rich:

- it uses HIR-level `MagicNumberContext`;
- it supports authorized-number configuration;
- it supports optional ignoring of array indexes;
- it supports constructor-specific exclusions;
- it distinguishes several contexts such as expression, return, method call,
  structure operations, property assignment, and simple assignment.

This strongly favors permissive treatment because the implementation is local
and much more specific than the generic rule idea.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic magic-value diagnostic and to reflect the actual excluded contexts.

### Tests

Current tests are mostly local inline Rust scenarios covering:

- authorized numbers;
- array-index behavior with config changes;
- return statements;
- structure/correspondence cases;
- default parameters;
- constructor exclusions;
- simple assignments with meaningful names.

There is also a larger inline regression fixture with exact ranges. It appears
to be inherited from earlier test data, but it is embedded directly in the
current Rust test module and does not depend on external fixture files.

## Important caveat

There is no strong public normative source here. The legal basis comes from
independent local implementation of a generic static-analysis idea, not from
translation of a standard text.

The larger inline regression case would still be a reasonable cleanup target if
you want maximal provenance confidence for the whole test corpus, but it does
not look like a serious blocker for permissive treatment of the production code.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MagicNumber` is a strong permissive candidate because it is a generic
maintainability rule with a clearly local HIR-based implementation, local test
coverage, and rewritten documentation.
