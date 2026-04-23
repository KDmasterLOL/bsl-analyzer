# SameMetadataObjectAndChildNames provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public naming guidance for 1C metadata objects: child elements should not reuse the same name as their owner. The rationale is also straightforward and public, because duplicate names create ambiguity in query expressions and maintenance.

## Public sources

- `#std474` "Организация хранения данных. Имя, Синоним, Комментарий"

## Audit result

The current implementation is local Rust code that inspects already loaded metadata and compares child object names with their parent names. It covers:

- metadata object attributes
- tabular sections
- tabular section attributes
- register dimensions
- register resources
- register attributes

The code shape is simple and metadata-driven.

## Important caveats

- Current implementation intentionally supports only the module kinds that are wired into the project infrastructure.
- `SessionModule` is listed in metadata, but the handler currently documents that this path is not yet supported in infrastructure.

## Conclusion

`SameMetadataObjectAndChildNames` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local and metadata-driven.
