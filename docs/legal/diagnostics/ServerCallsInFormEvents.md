# ServerCallsInFormEvents provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C performance guidance: frequent form events should avoid unnecessary server calls and network traffic. This is a public client-server design concern, not a unique analyzer-specific idea.

## Public sources

- `#std487` "Минимизация количества серверных вызовов и трафика"
- `#std630` "Правила создания модулей форм"
- Infostart article about problematic server calls from form events as a secondary practical reference

## Audit result

The current implementation is local Rust code built on top of the project's call-summary graph for form modules.

It:

- only runs for `FormModule`;
- only starts from the `OnActivateRow` and `OnStartChoice` form events;
- walks local call chains with bounded BFS;
- reports local methods that switch to server execution with context;
- reports immediate qualified common-module calls to server-only export methods;
- downgrades severity for paths that go through idle-handler registration.

## Important caveats

- The detector is narrower than the broad public performance guidance. It only covers two event types.
- It does not try to model every possible cross-module or asynchronous execution path.
- The idle-handler downgrade is a local project policy choice based on the current implementation.

## Conclusion

`ServerCallsInFormEvents` looks like a strong permissive candidate. The rule is grounded in public client-server performance guidance, and the current implementation is local call-graph analysis with a clearly documented scope.
