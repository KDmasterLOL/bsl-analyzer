# ExcessiveAutoTestCheck provenance

## Assessment

`ExcessiveAutoTestCheck` is a good candidate for `MIT OR Apache-2.0`.

The rule follows from a deprecated coding pattern around the `"АвтоТест"` / `"AutoTest"` parameter. It is tied to public 1C coding guidance and to the practical fact that these early-return branches are now dead compatibility code.

The current implementation in `bsl-analyzer` is local:

- it scans local syntax trees for `If` statements;
- it detects a small set of text patterns for `"АвтоТест"` / `"AutoTest"`;
- it verifies through local CST structure that the branch body contains only `Return`;
- it includes a parser-bug workaround based on the project's own syntax tree behavior.

## Source basis

- 1C standard on module texts: <https://its.1c.ru/db/v8std/content/456/hdoc>
- public mirror: <https://v8std.ru/std/456/>

In the public mirror, `ExcessiveAutoTestCheck` is explicitly listed as a crossed-out check under `#std456`, which is enough to support the diagnostic concept.

## Residual risk

Residual risk is low.

- the rule is based on a public and well-known legacy pattern;
- the implementation is structurally local and parser-specific;
- current tests are generic and mostly demonstrate matching behavior rather than borrowed prose.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
