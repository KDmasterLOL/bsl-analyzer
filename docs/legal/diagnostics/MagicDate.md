# Provenance: MagicDate

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic maintainability rule.

Hard-coded dates are a common static-analysis smell because they hide business
meaning and make later changes harder. This idea is generic and not tied to a
specific 1C standard.

There is no direct normative 1C standard source for this exact rule.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/magic_date.rs`
is clearly local and fairly detailed:

- it recognizes both single-quoted date literals and some string-based date
  literals;
- it validates date formats;
- it supports a configurable allowlist of authorized dates;
- it excludes multiple structural contexts such as simple assignments,
  `Дата(...)`, return statements, default values, structure/correspondence
  operations, and property assignments.

This strongly favors permissive treatment because the implementation is local
and substantially more specific than the generic rule idea.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a generic “magic value” style diagnostic and to reflect the actual excluded
contexts.

### Tests

Current tests are local inline Rust scenarios covering:

- direct date literals in expressions and conditions;
- `Дата(...)` handling;
- nested calls and constructors;
- ternary expressions;
- authorized-date configuration;
- excluded contexts such as simple assignment and return statements.

No large external fixture file is involved.

## Important caveat

There is no strong public normative source here. The legal basis comes from
independent local implementation of a generic static-analysis idea, not from
translation of a standard text.

That is still a strong position for permissive licensing.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MagicDate` is a strong permissive candidate because it is a generic
maintainability rule with a clearly local implementation, local tests, and
rewritten documentation.
