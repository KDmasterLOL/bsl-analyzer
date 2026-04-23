# Provenance: PublicMethodsDescription

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std453` ("Description of procedures and functions") supports the expectation
that public procedures and functions should be documented. That public guidance
is independent of any specific upstream implementation.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/public_methods_description.rs`
is local and moderately specific:

- it reads exported methods from local module/item data;
- it obtains parsed documentation comments through local `method_docs`;
- by default it checks only methods inside `ПрограммныйИнтерфейс` / `Public`;
- it supports a local `checkAllRegion` option to expand the scope beyond that
  default.

This strongly favors permissive treatment because the handler is a local
implementation of a public documentation rule, with a project-specific scope
switch.

### Documentation

RU/EN documentation was rewritten during this audit to clearly distinguish the
public rationale from the current local default scope and `checkAllRegion`
behavior.

### Tests

Current tests are local and cover:

- default mode, where only `ПрограммныйИнтерфейс` / `Public` is checked;
- `checkAllRegion = true`, where all exported methods are checked.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the exact default scope (`Public` only, not `Internal`) is a local
  implementation choice on top of the public rule;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`PublicMethodsDescription` is a strong permissive candidate because it
implements a public 1C documentation recommendation through local method-doc and
region analysis, with local tests and now-local documentation.
