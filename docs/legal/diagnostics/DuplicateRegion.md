# DuplicateRegion provenance

## Assessment

`DuplicateRegion` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public module-structure convention described in `#std455`. The diagnostic idea is not specific to `bsl-language-server`: if a module is organized into named regions, repeating the same top-level section is an obvious structural defect.

The current implementation in `bsl-analyzer` is local and AST-based:

- it reads module-level regions from local parser infrastructure;
- it normalizes a fixed set of standard Russian and English region names;
- it reports the first duplicated top-level region with local message formatting;
- it ignores nested regions by relying on local region extraction.

## Source basis

- 1C standard on module structure: <https://its.1c.ru/db/v8std/content/455/hdoc>
- public mirror: <https://v8std.ru/std/455/>

These sources are enough to justify both the existence of standard region names and the equivalence of Russian and English naming variants used by the diagnostic.

## Residual risk

Residual risk is low.

- The canonical mapping is derived from public standard templates.
- The implementation shape is local and tied to the project's AST/query architecture.
- Current tests use mostly generic examples built around public standard names.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
