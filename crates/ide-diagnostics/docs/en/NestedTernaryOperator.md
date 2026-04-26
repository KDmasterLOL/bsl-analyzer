# Nested ternary operator (NestedTernaryOperator)

## Description

This diagnostic reports ternary operators that are either nested inside another
ternary operator or embedded directly into `If` / `ElseIf` conditions.

Both forms are valid, but they usually make the code harder to read and debug.
In practice it is often clearer to assign an intermediate value first or to
replace a complex ternary chain with an explicit `If ... ElseIf ... EndIf`
branch.

## Examples

Incorrect:

```bsl
Result = ?(X > 10, ?(X > 100, "Large", "Medium"), "Small");
```

```bsl
If ?(EmployeeType = Null, 0, EmployeeType) = 0 Then
    Status = "Done";
EndIf;
```

Correct:

```bsl
If X > 100 Then
    Result = "Large";
ElseIf X > 10 Then
    Result = "Medium";
Else
    Result = "Small";
EndIf;
```

## Sources

- Secondary reference: [v8std.ru: NestedTernaryOperator](https://v8std.ru/diagnostics/bslls/NestedTernaryOperator/)
