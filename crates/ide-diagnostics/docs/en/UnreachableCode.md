# Unreachable Code (UnreachableCode)

## Description

This diagnostic reports code that cannot be executed because control flow has already left the current path.

Typical examples are statements placed after `Return`, `Raise`, `Break`, or `Continue`.

The current implementation is CFG-based and intentionally more precise than a simple text scan:

- it computes reachable vertices from the CFG entry using only live edges;
- for method bodies, it additionally distinguishes code that is locally unreachable after a terminator from code that is only unreachable because an outer branch is dead;
- adjacent unreachable statements are merged into one reported range.

## Examples

### Incorrect

```bsl
Procedure Example()
    Return;
    Message("This code will never run");
EndProcedure
```

```bsl
Function Example(Parameter1, Parameter2)
    If Error Then
        Raise "Error occurred";
        Parameter1 = Parameter2;
    EndIf;
    Return Parameter1;
EndFunction
```

### Correct

```bsl
Procedure Example(Condition)
    If Condition Then
        Return;
    EndIf;

    Message("This branch is still reachable");
EndProcedure
```

## Sources

- General control-flow semantics of BSL.
- [v8std.ru: UnreachableCode](https://v8std.ru/diagnostics/bslls/UnreachableCode/)
