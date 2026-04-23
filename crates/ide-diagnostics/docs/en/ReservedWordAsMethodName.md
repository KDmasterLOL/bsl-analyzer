# Reserved word used as procedure/function name (ReservedWordAsMethodName)

## Description

Procedure and function names cannot match reserved BSL keywords. Such code is invalid at the language level and is rejected by the platform parser or compiler.

The current implementation is intentionally simple:

- it reports procedures and functions whose declared name is already recognized as a reserved word during HIR lowering;
- it applies to both Russian and English keywords;
- it does not rely on project-specific configuration.

## Examples

### Incorrect

```bsl
&AtClient
Procedure Execute(Command)
    // ...
EndProcedure
```

### Correct

```bsl
&AtClient
Procedure ExecuteCommand(Command)
    // ...
EndProcedure
```

## Sources

- Public BSL language syntax and reserved-keyword rules.
