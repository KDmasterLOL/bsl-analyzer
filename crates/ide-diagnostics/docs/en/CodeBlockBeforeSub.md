# Method definitions must be placed before the module body operators (CodeBlockBeforeSub)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
BSL module structure expects executable initialization code to appear after
procedure and function declarations.

In the general case the order is:

1. module-level variables;
2. procedure and function definitions;
3. executable module body statements.

If free executable code appears before the first procedure or function, the
module becomes harder to read and no longer follows the standard module layout.

## Examples

### Incorrect

```bsl
SomeMethod();
Message("Before methods definition");

Procedure SomeMethod()
// Method body
EndProcedure
```

### Correct

```bsl
Procedure SomeMethod()
    // Method body
EndProcedure

SomeMethod();
Message("Initialization after method definitions");
```

## Sources

Primary source: [Module structure (RU)](https://its.1c.ru/db/v8std/content/455/hdoc)

Secondary source: [v8std.ru: #std455 Module structure](https://v8std.ru/std/455/)

Additional reference: [v8std.ru: CodeBlockBeforeSub](https://v8std.ru/diagnostics/bslls/CodeBlockBeforeSub/)
