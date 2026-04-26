# Erroneous indication of several compilation directives (SeveralCompilerDirectives)

## Description

A module variable, procedure, or function should not have more than one compilation directive.

Using several directives on the same item is a syntax-level error and also makes the execution context ambiguous.

The current implementation is simple and exact:

- it checks top-level procedures, functions, and module variables from the item tree;
- it reports any item with more than one annotation;
- comments or blank lines between directives do not matter.

## Examples

### Incorrect

```bsl
&AtServer
&AtClient
Var MyVariable;

&AtServer
&AtClient
Procedure MyProcedure()

EndProcedure
```

### Correct

```bsl
&AtClient
Var MyVariable;

&AtServer
Procedure MyProcedure()
EndProcedure
```

## Sources

- Public BSL syntax and compiler-directive semantics.
- [v8std.ru: SeveralCompilerDirectives](https://v8std.ru/diagnostics/bslls/SeveralCompilerDirectives/)
