# UsingObjectNotAvailableUnix provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public platform constraints: some objects and technologies are unavailable in Unix/Linux environments, so cross-platform code should avoid relying on them without an appropriate platform guard.

This is a public compatibility concern, not a unique analyzer-specific idea.

## Public sources

- official 1C guidance on cross-platform application development
- official 1C material about running the client application on Linux

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It reports a fixed set of object types, currently including `COMObject` / `COMОбъект` and `Mail` / `Почта`, and suppresses diagnostics when local guard-detection logic sees platform checks around the usage.

## Important caveats

- The set of reported object types is a local implementation choice of this project.
- The suppression logic for platform guards is also local and heuristic; it is not a formal proof that all safe cases are recognized.
- The message itself is intentionally advisory: it asks the developer to verify Unix-compatible analogs rather than claiming the code is always invalid.

## Conclusion

`UsingObjectNotAvailableUnix` looks like a strong permissive candidate. The rule is grounded in public platform constraints, and the current implementation is local HIR-based compatibility logic.
