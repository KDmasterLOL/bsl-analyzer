# Unary Plus sign in string concatenation (UnaryPlusInConcatenation)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

When concatenating strings, it is easy to accidentally type an extra `+`: `String1 + + String2`. In that case the second plus is parsed as a unary operator, and the platform tries to interpret the right operand as a number.

In most real cases this leads to a runtime error instead of normal string concatenation.

The current implementation reports exactly this accidental pattern. Unary plus on a numeric literal is not reported.

## Examples

Incorrect:

```bsl
Message = "Document: " + + DocumentNumber;
```

Correct:

```bsl
Message = "Document: " + DocumentNumber;
```

## Sources

- Public BSL language semantics for `+` and string concatenation
