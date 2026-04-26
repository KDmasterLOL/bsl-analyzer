# Cast to number of try catch block (TryNumber)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Using `try...except` as a normal way to convert a value to number is discouraged. Exception handling should be reserved for exceptional situations, not routine type conversion.

The current implementation reports calls to `Number()` / `Число()` that occur inside the `try` part of a `try...except` block.

## Examples

Incorrect:

```bsl
Try
 NumberDaysAllowance = Number(Value);
Raise
 NumberDaysAllowance = 0; // default value
EndTry;
```

Correct:

```bsl
TypeDescription = New TypeDescription("Number");
NumberDaysAllowance = TypeDescription.CastValue(Value);
```

## Sources

- [#std499: Catching exceptions in code (RU)](https://its.1c.ru/db/v8std#content:499:hdoc)
