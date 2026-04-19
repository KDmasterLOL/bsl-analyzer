# CommonModuleAssign (CommonModuleAssign)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports assignments whose left-hand side resolves to the name of
a common module from metadata.

In 1C, a common module name is not a writable variable. If local code tries to
assign a value to such an identifier, the platform treats the target as a
reference to the common module and the statement fails at runtime.

The most common cause is a naming conflict: a local variable name accidentally
matches an existing common module name.

## Examples

### Wrong

```bsl
// Metadata contains common module CommonUtilities
CommonUtilities = 42;
```

### Correct

```bsl
LocalUtilities = 42;
```

## Sources

Secondary source: [v8std.ru: CommonModuleAssign](https://v8std.ru/diagnostics/bslls/CommonModuleAssign/)

Additional reference: [bsl-language-server: CommonModuleAssign](https://1c-syntax.github.io/bsl-language-server/diagnostics/CommonModuleAssign/)
