# FormDataToValue provenance

## Assessment

`FormDataToValue` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public 1C standard `#std409`: in most form-module cases, `РеквизитФормыВЗначение()` should be used instead of `ДанныеФормыВЗначение()` because it has simpler syntax and reduces the chance of mistakes.

## Source basis

- 1C standard on using `РеквизитФормыВЗначение` and `ДанныеФормыВЗначение`: <https://its.1c.ru/db/v8std/content/409/hdoc>
- public mirror: <https://v8std.ru/std/409/>

This is enough to justify the recommendation itself.

## Implementation notes

The current implementation in `bsl-analyzer` is local:

- detection happens during the project's own AST-to-HIR lowering;
- the diagnostic reports global and qualified `ДанныеФормыВЗначение` / `FormDataToValue` calls;
- methods marked `БезКонтекста` are excluded by the local context analysis.

The `БезКонтекста` exclusion is an implementation detail of this project, not an explicit statement in the standard.

## Residual risk

Residual risk is low.

- the rule is explicitly standard-based;
- the implementation is local and context-aware;
- the main cleanup needed here was documentation wording and clarification of the local scope.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
