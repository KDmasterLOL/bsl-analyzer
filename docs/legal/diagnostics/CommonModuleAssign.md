# Provenance: CommonModuleAssign

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is based on platform behavior rather than on a direct published
`v8std` rule.

The underlying idea is straightforward: a common module name resolves to a
metadata object reference, not to a writable local variable, so assigning to it
is invalid and leads to a runtime failure.

Public catalog reference:

- `v8std.ru` marks `CommonModuleAssign` as a diagnostic without a direct
  standard mapping

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_assign.rs`
is small and local:

- assignment candidates are emitted by local HIR lowering;
- the handler resolves the target name through local configuration metadata;
- the diagnostic is reported only when the identifier matches a common module.

This favors permissive treatment because the handler is an independent metadata
lookup over a simple platform rule.

### Documentation

Public documentation was rewritten during this audit to describe the runtime
behavior directly rather than inheriting placeholder or upstream wording.

### Tests

Current tests are local and narrow:

- no metadata case;
- property access;
- index access;
- simple identifier assignment candidate.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- there is no clean official standard page to cite as a primary normative source;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleAssign` is a good permissive candidate because:

- the rule follows from basic platform name-resolution behavior;
- the current handler is small and independently implemented;
- docs and tests do not require retaining copyleft treatment.
