# Provenance: CommonModuleMissingAPI

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C module-structure guidance.

Primary source:

- ITS / v8std `#std455`

Related architectural context:

- `#std551` on reusable library code and metadata objects

The rule is organizational: common modules and manager modules with methods are
expected to expose an explicit API shape through exported methods and dedicated
API regions.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_missing_api.rs`
uses local syntax-tree analysis:

- module type comes from local metadata;
- methods are detected from local AST traversal;
- exported methods and API region names are checked through local predicates.

This supports permissive treatment because the rule is standards-based and the
implementation is a simple local structural check.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
module structure and API separation rather than inherited short-form wording.

### Tests

Current tests are local and synthetic:

- valid module with export and API region;
- missing export;
- missing API region;
- modules without methods;
- ignored module types.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the exact “must have both export and API region” interpretation is still an
  implementation policy layered on top of the broader structure standards;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleMissingAPI` is a good permissive candidate because:

- the rule is derived from published module-structure guidance;
- the current implementation is local and structural;
- the active tests and docs do not require retaining copyleft treatment.
