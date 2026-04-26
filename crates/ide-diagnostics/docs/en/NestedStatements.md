# Control flow statements should not be nested too deep (NestedStatements)

## Description

This diagnostic reports control-flow statements nested deeper than the allowed
limit.

Deep nesting makes code harder to read, reason about, test, and refactor.
Usually it is a sign that part of the logic should be extracted into a separate
method or flattened with early exits.

The default maximum nesting level is `4`, but the diagnostic can be configured
through `maxAllowedLevel`.

## Examples

Incorrect:

```bsl
If Something Then
    If SomeCondition Then
        For Num = 0 To 10 Do
            Try
                If OneMoreCondition Then
                    // nesting level 5
                EndIf;
            Except
            EndTry;
        EndDo;
    EndIf;
EndIf;
```

Correct:

```bsl
If Not Something Then
    Return;
EndIf;

If Not SomeCondition Then
    Return;
EndIf;

ProcessItems();
```

## Sources

- Source: [Sonar RSPEC-134](https://rules.sonarsource.com/java/RSPEC-134)
