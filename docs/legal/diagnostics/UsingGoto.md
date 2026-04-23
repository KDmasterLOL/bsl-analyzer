# UsingGoto provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule is a straightforward public coding-style restriction: `Goto` / `Перейти` is unstructured control flow and should be replaced with structured constructs. This is not a unique analyzer-specific idea.

## Public sources

- official 1C standard on using `Перейти`
- `v8std.ru/diagnostics/bslls/UsingGoto/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. The handler is minimal and only emits the final message for `Goto` statements that were already recognized during lowering.

## Important caveats

- The actual recognition of `Goto` happens in the local HIR lowering layer, not in this handler file.
- The rule itself is simple enough that there is little room for expressive borrowing beyond the message and docs, both of which are now local.

## Conclusion

`UsingGoto` looks like a strong permissive candidate. The rule is public and generic, and the implementation path is local and HIR-based.
