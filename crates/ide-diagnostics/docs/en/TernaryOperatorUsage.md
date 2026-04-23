# Ternary operator usage (TernaryOperatorUsage)

## Description

This diagnostic recommends replacing the ternary operator `?(...)` with an explicit `If ... Else ... EndIf` construct.

The rationale is readability: compact ternary expressions become hard to parse, especially when nested or embedded into larger expressions.

The current implementation is intentionally broad:

- it reports every ternary operator usage;
- nested ternaries are reported separately for each nested `?(...)`;
- the rule is disabled by default and only works when explicitly enabled in configuration.

## Examples

### Incorrect

```bsl
Result = ?(X % 15 <> 0, ?(X % 5 <> 0, ?(X % 3 <> 0, X, "Fizz"), "Buzz"), "FizzBuzz");
```

### Correct

```bsl
If X % 15 = 0 Then
    Result = "FizzBuzz";
ElseIf X % 3 = 0 Then
    Result = "Fizz";
ElseIf X % 5 = 0 Then
    Result = "Buzz";
Else
    Result = X;
EndIf;
```

### Incorrect

```bsl
If ?(P.Emp_emptype = Null, 0, P.Emp_emptype) = 0 Then
    Status = "Done";
EndIf;
```

### Correct

```bsl
If P.Emp_emptype = Null OR P.Emp_emptype = 0 Then
    Status = "Done";
EndIf;
```

## Sources

- Generic readability guidance for conditional expressions.
- [v8std.ru: TernaryOperatorUsage](https://v8std.ru/diagnostics/bslls/TernaryOperatorUsage/)
