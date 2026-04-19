# Provenance: CommandModuleExportMethods

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows the published 1C standard on restrictions for exported
procedures and functions in command modules and common command modules.

Primary source:

- ITS / v8std `#std544`

The rule is platform-behavior-based: those module types are not designed as
externally callable APIs, so `Экспорт` is ineffective there.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/command_module_export_methods.rs`
uses local module metadata and local item-tree analysis:

- module type is derived from the local metadata layer;
- exported procedures and functions are read from the local item tree;
- the diagnostic only reports the method name range.

This favors permissive treatment because the rule is standards-based and the
implementation is expressed through local infrastructure.

### Documentation

Public documentation was rewritten during this audit to describe the rule from
the 1C standard and platform behavior rather than inherited wording.

### Tests

Current tests use local inline fixtures and a local command-module path setup.
They do not depend on borrowed upstream resource files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommandModuleExportMethods` is a good permissive candidate because:

- the rule directly follows from `#std544`;
- the handler relies on local metadata and item-tree analysis;
- the current test setup is already local and straightforward.
