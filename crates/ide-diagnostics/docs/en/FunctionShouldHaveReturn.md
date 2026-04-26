# The function should have return (FunctionShouldHaveReturn)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

`Function` differs from a `Procedure` only that it necessarily returns a value and can be used in expressions.

Based on the above-mentioned, a `function` which does not contain a return is itself erroneous. Corrections required:

- implement return if the implemented method is a function
- rewrite function to procedure if return is not needed

The current implementation checks only that the function contains at least one `Return`. It does not prove that every control-flow path returns a value; that stricter case belongs to `AllFunctionPathMustHaveReturn`.

## Examples

### Incorrect

```bsl
Function Total(Document)
    Sum = 0;
EndFunction
```

### Correct

```bsl
Function Total(Document)
    Sum = 0;
    Return Sum;
EndFunction
```

## Sources

- [v8std.ru: FunctionShouldHaveReturn (RU)](https://v8std.ru/diagnostics/bslls/FunctionShouldHaveReturn/)
