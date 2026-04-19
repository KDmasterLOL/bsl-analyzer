# EmptyRegion provenance

## Assessment

`EmptyRegion` is a good candidate for `MIT OR Apache-2.0`.

The rule follows naturally from the public module-structure convention in `#std455`: regions are a structuring tool, so an empty region adds noise without adding structure. This diagnostic idea is not specific to `bsl-language-server`.

The current implementation in `bsl-analyzer` is local:

- empty regions are detected during local preprocessing and HIR lowering;
- the diagnostic handler only formats the emitted message;
- nested-region behavior is defined by local lowering logic and covered by local tests.

## Source basis

- 1C standard on module structure: <https://its.1c.ru/db/v8std/content/455/hdoc>
- public mirror: <https://v8std.ru/std/455/>

These sources justify the role of regions as module-structure elements. An empty region is then an obvious structural smell.

## Residual risk

Residual risk is low.

- the rule is generic and derived from public coding conventions;
- the current behavior is tied to local lowering semantics;
- the main cleanup needed here was documentation wording.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
