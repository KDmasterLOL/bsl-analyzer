# ExtraCommas provenance

## Assessment

`ExtraCommas` is a good candidate for `MIT OR Apache-2.0`.

The rule is a generic syntax/style check: trailing commas at the end of an argument list without a following parameter make the call harder to read and blur the distinction between intentionally skipped optional parameters and accidental punctuation.

The public 1C parameter-style guidance in `#std640` supports the general concern around method parameters and punctuation, even though the exact detection rule here is a local implementation detail.

## Source basis

- 1C standard on procedure and function parameters: <https://its.1c.ru/db/v8std/content/640/hdoc>
- public mirror: <https://v8std.ru/std/640/>

These sources provide sufficient context for the readability concern. The exact notion of “extra trailing commas” is implemented locally.

## Implementation notes

The current implementation in `bsl-analyzer` is local:

- detection happens during the project's own AST-to-HIR lowering;
- the diagnostic targets trailing commas after the last effective argument in a call;
- skipped optional parameters in the middle of the call are allowed and covered by tests.

## Residual risk

Residual risk is low.

- the rule is generic and syntax-oriented;
- the implementation is local and parser-specific;
- examples and tests are simple and do not appear to rely on protected upstream expression.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
