# ExecuteExternalCode provenance

## Assessment

`ExecuteExternalCode` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public 1C security guidance in `#std770`: using `Выполнить` / `Вычислить` in server-side code can enable arbitrary code execution when the executed string is influenced by external input.

The current implementation in `bsl-analyzer` is local:

- detection points are implemented in the project's own AST-to-HIR lowering;
- the IDE diagnostic layer only maps that local HIR signal into a user-facing report;
- the client-only exemption is defined by local annotation/context logic;
- common modules are handled separately by `ExecuteExternalCodeInCommonModule`.

## Source basis

- 1C standard on restrictions for `Выполнить` and `Вычислить` on the server: <https://its.1c.ru/db/v8std/content/770/hdoc>
- public mirror: <https://v8std.ru/std/770/>

These sources are sufficient to justify both the security concern and the server-only scope of the diagnostic.

## Residual risk

Residual risk is low.

- the rule is grounded in a public security standard;
- the implementation is tightly coupled to local HIR/context analysis;
- the main cleanup needed here was documentation wording, not algorithmic rewrite.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
