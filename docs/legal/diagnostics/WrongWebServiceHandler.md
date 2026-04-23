# WrongWebServiceHandler provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

The rule follows public 1C platform behavior for Web service metadata. Web service operations reference handlers by name, and those handlers must exist in the corresponding module. This is metadata and API validation, not an original upstream idea.

## Public sources

- 1C developer documentation on Internet service mechanisms.
- 1C methodological materials on Web services and HTTP services.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and metadata-based:

- it runs only for `WebServiceModule`;
- it reads handler names from `metadata.web_service`;
- it resolves handlers through the local symbol tree;
- it reports missing or unresolved handler names.

It does not validate handler bodies or compare parameter lists.

## Audit notes

- Rule idea: clean.
- Docs were rewritten to remove unsupported claims about handler bodies and parameter counts.
- Existing tests are local metadata fixtures and do not show direct copying risk.
