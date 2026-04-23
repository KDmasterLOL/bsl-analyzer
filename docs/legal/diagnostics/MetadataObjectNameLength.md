# Provenance: MetadataObjectNameLength

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is directly grounded in public 1C guidance.

The primary source is `#std474`, which explicitly says that metadata object
names must not exceed `80` characters.

So the rule idea and threshold are public and straightforward.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/metadata_object_name_length.rs` is local
and metadata-driven:

- it reads the effective maximum length from local configuration;
- it checks common modules, metadata objects, and registers;
- it has separate logic for session-module analysis of metadata objects without
  modules.

This strongly favors permissive treatment because the implementation is local
and directly derived from a public requirement.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
and its current implementation scope more clearly.

### Tests

Current tests are local Rust-side metadata scenarios covering:

- long and short names for common modules;
- long and short names for registers and metadata objects;
- custom max length;
- disabled diagnostics;
- session-module handling for objects without modules.

No external fixture file is involved.

## Important caveat

This rule is simple and strongly standard-based. No special parser or SDBL
provenance caveat applies here.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MetadataObjectNameLength` is a strong permissive candidate because it is a
direct implementation of a public 1C standard rule with local metadata-driven
code, local tests, and rewritten documentation.
