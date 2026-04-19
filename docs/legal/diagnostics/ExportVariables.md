# ExportVariables provenance

## Assessment

`ExportVariables` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public 1C guidance in `#std639`: exported module variables broaden visibility of mutable state and are discouraged because they lead to fragile coupling and hard-to-reproduce bugs.

The current implementation in `bsl-analyzer` is local:

- module variables are collected during the project's own HIR lowering;
- the diagnostic checks the local `is_export` flag on module-level variable declarations;
- tests are short and generic and do not depend on borrowed fixtures.

## Source basis

- 1C standard on using variables in modules: <https://its.1c.ru/db/v8std/content/639/hdoc>
- public mirror: <https://v8std.ru/std/639/>

These sources are sufficient to justify both the problem statement and the recommended alternatives such as `AdditionalProperties`.

## Residual risk

Residual risk is low.

- the rule is grounded in a public standard;
- the implementation is tightly coupled to the local HIR representation;
- the main cleanup needed here was documentation wording, not algorithmic rewrite.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
