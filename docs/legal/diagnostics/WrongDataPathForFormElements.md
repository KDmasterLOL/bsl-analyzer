# WrongDataPathForFormElements provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

The rule is based on public 1C platform behavior rather than on an original upstream idea. A form element whose XML `DataPath` starts with `~` points to an unresolved metadata reference. This follows from how 1C stores broken form bindings in form metadata and from general configuration consistency requirements.

## Public sources

- `#std467` General requirements for configurations.
- Public descriptions of the diagnostic on `v8std.ru`, used only as a secondary reference.

## Implementation notes

The current implementation is local and metadata-based:

- it runs only for `FormModule`;
- it reads the current form metadata via `ModuleMetadata`;
- it reports elements returned by `form.elements_with_wrong_data_path()`.

This is not a parser-port or SDBL-derived rule. The implementation is specific to this project and tied to its metadata extraction pipeline.

## Audit notes

- Rule idea: clean.
- Current docs were rewritten to describe the actual behavior instead of broader form maintenance scenarios.
- Existing tests are local metadata fixtures and do not show direct copying risk.
