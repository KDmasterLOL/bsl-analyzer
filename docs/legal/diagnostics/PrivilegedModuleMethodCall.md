# Provenance: PrivilegedModuleMethodCall

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic security-hotspot rule.

The core idea is straightforward: calls into privileged modules deserve review
because privileged code may execute with broader rights than the caller. This is
not tied to a unique protectable upstream concept or to a single 1C normative
standard.

There is no direct `v8std` standard mapping for this exact rule.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/privileged_module_method_call.rs`
is local and substantial:

- it loads configuration metadata and finds privileged common modules locally;
- it uses the local call summary and qualified-module call edges;
- it resolves the target path locally to ensure the call actually hits an
  exported method;
- it supports a local policy option, `validateNestedCalls`, to suppress
  privileged self-calls.

This strongly favors permissive treatment because the implementation is a local
security analysis built on project-specific metadata and call-graph facilities.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current
hotspot semantics and the local `validateNestedCalls` behavior.

### Tests

Current tests are local and cover:

- missing metadata;
- disabled diagnostic config.

The direct behavioral logic is also easy to inspect from the small handler and
its dependence on local metadata/call-summary services.

## Remaining caveats

- this is a security policy rule rather than a direct public standard;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`PrivilegedModuleMethodCall` is a strong permissive candidate because it is a
local security-hotspot rule implemented with local metadata and call-graph
analysis, with now-local documentation.
