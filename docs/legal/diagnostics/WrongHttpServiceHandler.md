# WrongHttpServiceHandler provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

The rule follows public 1C platform behavior for HTTP service metadata. HTTP service methods reference handlers by name, and those handlers must exist in the module and expose the expected request parameter shape. This is API and metadata validation, not an original upstream idea.

## Public sources

- 1C developer documentation on Internet service mechanisms.
- 1C methodological materials on Web services and HTTP services.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and metadata-based:

- it runs only for `HTTPServiceModule`;
- it reads handler names from `metadata.http_service`;
- it resolves handlers through the local symbol tree;
- it validates that the handler exists and has exactly one parameter.

The implementation does not inspect handler bodies and does not depend on parser-derived query logic or borrowed SDBL code.

## Audit notes

- Rule idea: clean.
- Docs were rewritten to remove incorrect claims about empty handler bodies.
- Existing tests are local metadata fixtures and do not show direct copying risk.
