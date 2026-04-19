# Provenance: CommonModuleNameServerCall

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`, section `2.2`

The rule is organizational and metadata-based: a common module intended for
server calls from client code should expose that role in its name through the
`ВызовСервера` / `ServerCall` postfix.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_server_call.rs` is
defined through local metadata predicates:

- it checks that the module is marked for server calls;
- it validates the module name against local keyword variants;
- it reports only when the naming rule is violated.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local metadata check.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std469` and server-call module naming semantics rather than inherited
short-form wording.

### Tests

Current tests are local and synthetic:

- server-call module without postfix;
- server-call module with English postfix;
- server-call module with Russian postfix.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameServerCall` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
