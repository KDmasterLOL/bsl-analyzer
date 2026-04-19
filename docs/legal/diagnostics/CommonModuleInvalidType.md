# Provenance: CommonModuleInvalidType

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C rules for creating common
modules.

Primary source:

- ITS / v8std `#std469`

The rule is organizational and metadata-based: a common module should match one
of the standard execution-context combinations used by the platform and by the
1C naming conventions.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_invalid_type.rs`
uses local metadata and local helper predicates:

- common module flags come from local metadata extraction;
- validation is delegated to local `common_module_helpers`;
- the diagnostic reports the whole module as a metadata-level issue.

This favors permissive treatment because the rule is standards-based and the
implementation is a straightforward local validation over metadata flags.

### Documentation

Public documentation was rewritten during this audit to describe the rule from
`#std469` and the module-type matrix rather than inherited wording.

### Tests

Current tests are local and synthetic:

- invalid common module flags;
- valid server module flags;
- non-common-module metadata.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleInvalidType` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
