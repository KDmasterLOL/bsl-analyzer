# Provenance: OSUsersMethod

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic security-hotspot rule.

The underlying idea is straightforward: code that enumerates operating-system
users may reveal sensitive environment information and deserves explicit review.
This is not tied to a unique 1C standard or to any uniquely protectable
upstream concept.

There is no direct normative `v8std` source for this exact rule.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/os_users_method.rs`
is very small and local:

- HIR lowering identifies unqualified calls to `ПользователиОС` / `OSUsers`;
- the handler converts that local body diagnostic into a security-hotspot
  message;
- qualified calls and non-call references are intentionally excluded by the
  local lowering logic, as shown by tests.

This strongly favors permissive treatment because the implementation is a small
local hotspot check over a generic security concern.

### Documentation

RU/EN documentation was rewritten during this audit to describe the rule as a
security-review trigger rather than as copied upstream text.

### Tests

Current tests are local inline fixtures covering:

- detection of `ПользователиОС`, `OSUsers`, and case-insensitive variants;
- exclusion of plain references without a call;
- exclusion of qualified calls like `МойМодуль.ПользователиОС()`.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the exact security rationale is a project policy choice rather than a direct
  1C standard requirement;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`OSUsersMethod` is a strong permissive candidate because it is a local
security-hotspot rule with local implementation, local tests, and now-local
documentation.
