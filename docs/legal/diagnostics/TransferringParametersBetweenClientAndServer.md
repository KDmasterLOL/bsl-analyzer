# TransferringParametersBetweenClientAndServer provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C client-server performance guidance: unnecessary data transfer between client and server should be avoided. If a server method is called from the client and a by-reference parameter is never reassigned, sending its value back is wasteful. This is a public performance concern, not a unique analyzer-specific idea.

## Public sources

- 1C developer guide section about transferring control from client to server
- `#std487` "Минимизация количества серверных вызовов и трафика" as general public performance context

## Audit result

The current implementation is local Rust code built on top of the call summary and lowered method bodies.

It:

- finds `&НаСервере` / `&НаСервереБезКонтекста` methods;
- checks only parameters without `Знач`;
- only reports methods that are directly called from `&НаКлиенте` methods in the same module;
- suppresses the remark if the parameter is assigned anywhere in the server method body.

## Important caveats

- The implementation is narrower than the broad public performance guidance.
- It only looks at direct local client-to-server calls in the same module.
- Assignment detection is syntactic and conservative: any assignment to the parameter suppresses the remark.
- The current implementation does not support the `cachedValueNames` configuration described in the old docs.

## Conclusion

`TransferringParametersBetweenClientAndServer` looks like a strong permissive candidate. The rule is grounded in public client-server performance guidance, and the current implementation is local call-graph plus HIR analysis with clearly documented limits.
