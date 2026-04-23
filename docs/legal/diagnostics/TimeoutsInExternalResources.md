# TimeoutsInExternalResources provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The underlying rule comes directly from public 1C guidance: operations that access external resources should use explicit timeouts. This is a public platform and reliability requirement, not a unique analyzer-specific idea.

## Public sources

- `#std748` "Таймауты при работе с внешними ресурсами"
- `metod8dev` note about the default timeout of `ИнтернетПочтовыйПрофиль`
- `v8std.ru/std/748/` as a public secondary reference

## Audit result

The current implementation is local Rust code built on top of the project's HIR bodies.

It recognizes a fixed set of constructors:

- `FTPConnection`
- `HTTPConnection`
- `WSDefinitions`
- `WSProxy`
- `InternetMailProfile`

The detector reports a constructor call when:

- the timeout argument is absent, missing, or effectively undefined at the expected parameter position;
- and no later assignment to `Timeout` / `Таймаут` is found for the same simple variable.

## Important caveats

- The implementation is narrower than the full public guidance in `#std748`. It does not model every way of configuring timeouts for external resources.
- The post-construction check only recognizes assignment to a simple variable like `Connection.Timeout = ...`.
- The detector does not perform path-sensitive reasoning. Any later timeout assignment to the same simple variable suppresses the diagnostic, even if it happens only inside a branch.
- `InternetMailProfile` support is configurable through `analyzeInternetMailProfileZeroTimeout`.

## Conclusion

`TimeoutsInExternalResources` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local project-specific HIR logic with clearly documented scope limitations.
