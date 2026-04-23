# ReadOnlyPropertyAssignment provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule expresses a generic language and platform constraint: assigning to a property that is documented as read-only is an error or a no-op. That idea follows directly from platform API semantics and does not depend on a unique analyzer-specific concept.

## Public basis

- Platform help-book entries that mark properties as `Использование: Только чтение`

There is no direct `v8std` mapping for this rule.

## Audit result

The current implementation is local Rust code. The diagnostic is created from `InferenceDiagnostic::ReadOnlyPropertyAssignment`, which comes from the project's own HIR type inference and property resolution pipeline.

The handler itself is very small and only formats the final message. The meaningful implementation work happens in local inference code that resolves platform properties and checks the help-book metadata.

## Important caveats

- The rule depends on the accuracy of the bundled help-book metadata for platform properties.
- False positives would more likely indicate stale property metadata than code provenance issues.

## Conclusion

`ReadOnlyPropertyAssignment` looks like a strong permissive candidate. The rule is grounded in public platform semantics, and the current implementation path is local and HIR-based.
