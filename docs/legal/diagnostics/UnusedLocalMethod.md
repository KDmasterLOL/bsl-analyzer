# UnusedLocalMethod provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic maintainability rule: local procedures and functions that are never called should usually be removed or completed. The idea is standard static-analysis guidance and not a unique analyzer-specific invention.

## Public sources

- `#std456` "Тексты модулей"
- `v8std.ru/diagnostics/bslls/UnusedLocalMethod/` as a secondary public reference

## Audit result

The current implementation is local Rust code. It combines several local sources of information:

- call graph summaries for direct local calls
- HIR method-call collection for conservative self-call detection
- metadata-derived platform handlers for forms and HTTP services
- local exclusions for exported methods, extension annotations, attachable prefixes, and platform event handlers

This makes the implementation more than a simple text scan; it is a local semantic rule over the project's own HIR and metadata model.

## Important caveats

- The exact exclusion list is a local implementation choice of this project.
- `checkObjectModule` and `attachableMethodPrefixes` are local configuration policy, not public standard text.

## Conclusion

`UnusedLocalMethod` looks like a strong permissive candidate. The rule is generic, and the current implementation is local and semantically richer than a direct port would need to be.
