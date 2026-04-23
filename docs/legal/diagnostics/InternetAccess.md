# Provenance: InternetAccess

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic security-audit rule for Internet-facing access.

It is not tied one-to-one to a single normative 1C standard. The closest public
context is:

- `#std794`, which restricts the use of external resources;
- `#std678`, which treats server APIs as a security boundary.

Those standards provide the security rationale, but the exact detection policy
in `bsl-analyzer` is local.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/internet_access.rs`
is local and HIR-based:

- it scans local `Expr::New` expressions in method bodies and module-level code;
- it matches a local list of constructor names related to HTTP, FTP, WS, mail,
  and proxy access;
- it supports both named constructors and string-based `Новый("...")`.

This favors permissive treatment because the rule concept is generic and the
implementation is clearly local.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as a security-review aid, not as a direct standard-mandated ban.

### Tests

Current tests are local inline Rust scenarios covering:

- Russian and English constructor names;
- case-insensitive matching;
- all supported constructor patterns;
- string-based constructors;
- ignoring standard non-network types.

The tests are local and do not depend on an external fixture file.

## Important caveat

The exact constructor list is a local project choice, not a public canonical
list from 1C standards. So the public rationale is strong, but the detection
surface itself should be understood as `bsl-analyzer` policy.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`InternetAccess` is a strong permissive candidate because it is a local
security-audit rule with a clearly local implementation, even though its
security motivation overlaps with public 1C guidance on external resources and
API boundaries.
