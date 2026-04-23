# Type mismatch (TypeMismatch)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic is reserved for cases where the inferred type of an expression does not match the type expected by the surrounding context.

At the moment, the handler and metadata already exist, but the live emitter in type inference is still disabled. So this diagnostic is not yet produced for user code.

When the emitter is enabled, the expected use case is reporting situations such as assigning a value of an incompatible type, passing an argument of the wrong type, or returning a value that does not match the declared expectations of the surrounding context.

## Examples

Planned future example:

```bsl
Value = 1;
Value = "text"; // type mismatch in a stricter typed context
```

## Sources

- Internal type-inference based diagnostic in `hir-ty`
