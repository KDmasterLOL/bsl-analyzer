# Call to an unresolved method (UnresolvedMethodCall)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

The diagnostic reports a qualified method call that cannot be resolved confidently by the analyzer.

This usually means one of the following:

- the common module name is wrong;
- the method name contains a typo;
- the method exists but is not exported;
- metadata refers to a common module whose source file is missing from the workspace.

The current implementation is conservative and focuses on qualified calls such as `CommonModule.Method()`, where the resolver has enough information to produce a useful error.

## Examples

Incorrect:

```bsl
CommonModule.UnknownMethod();
```

Correct:

```bsl
CommonModule.KnownMethod();
```
