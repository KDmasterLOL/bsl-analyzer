# Provenance: LineLength

## Status

Likely candidate for `MIT OR Apache-2.0`, with one moderate caveat around the
lineage of the large regression fixture.

## Why this rule exists

This diagnostic is grounded in public 1C guidance about module text style.

The closest public source is `#std456`, which includes the general expectation
that module text should remain readable and consistently formatted. A `120`
character limit is a common practical convention for such style checks.

So the overall rule rationale is public, even though the exact threshold and
config behavior are tool-specific.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/line_length.rs`
is local and nontrivial:

- it computes line lengths using the local `LineIndex`;
- it distinguishes code length from full line length with comments;
- it can ignore multiline string fragments;
- it supports local configuration for threshold, method-description comments,
  and trailing comments.

This strongly favors permissive treatment for the production code.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the
current configurable behavior and tie the rule to `#std456` as public context.

### Tests

Current tests are mostly local Rust-side checks for:

- UTF-8 character counting;
- custom `maxLineLength`;
- excluding method-description comments;
- excluding trailing comments.

There is also one large inline regression fixture with many exact range
assertions. It appears to be inherited from earlier line-length test data and
has not yet been independently rewritten during this audit.

## Important caveat

The exact implementation is local, but the big regression fixture should still
be treated as a cleanup target if the goal is to maximize confidence in fully
permissive provenance for the whole diagnostic package.

So the practical current status is:

`production code is clean; large regression fixture should be reviewed or rewritten later`

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- the large inline fixture has not yet been provenance-cleaned in the same way
  as some other audited diagnostics;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`LineLength` looks like a good permissive candidate at the level of rule idea
and production implementation, but the inherited large regression fixture is the
main remaining cleanup point.
