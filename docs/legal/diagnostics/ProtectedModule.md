# Provenance: ProtectedModule

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic code-reviewability and security-awareness rule.

The core idea is straightforward: password-protected modules hide source code,
which makes inspection, auditing, and controlled maintenance harder. This is not
tied to a unique upstream concept or to a direct `v8std` standard mapping.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/protected_module.rs`
is local and metadata-based:

- it runs only for `SessionModule` files;
- it loads configuration metadata locally;
- it scans common modules for the local `is_protected()` flag;
- it emits one project-level diagnostic per protected module.

This strongly favors permissive treatment because the implementation is a simple
local metadata check over a generic maintainability/security concern.

### Documentation

RU/EN documentation was rewritten during this audit to describe the actual
behavior and `SessionModule`-only scope in local wording.

### Tests

Current tests are local and cover:

- non-session modules;
- disabled diagnostic config;
- absence of metadata.

The implementation itself is small and easy to inspect directly.

## Remaining caveats

- the choice to evaluate this rule only from `SessionModule` is a local product
  decision;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`ProtectedModule` is a strong permissive candidate because it is a local
metadata-based rule about protected source visibility, with local tests and
now-local documentation.
