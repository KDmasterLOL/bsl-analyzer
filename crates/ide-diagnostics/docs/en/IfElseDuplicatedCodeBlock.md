# Duplicated code blocks in If...Then...ElseIf... statements (IfElseDuplicatedCodeBlock)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports `If` / `ElseIf` / `Else` branches that contain the
same block of code.

When two branches execute identical statements, the condition usually carries no
useful distinction. In practice this often means one of two things:

- the conditions can be merged;
- one branch was copied and then not updated correctly.

## Examples

```bsl
If p = 0 Then
    t = 0;
ElseIf p = 1 Then
    t = 1;
ElseIf p = 2 Then
    t = 1;
Else
    t = -1;
EndIf;
```

```bsl
If p = 0 Or p = 1 Then
    t = 1;
Else
    t = -1;
EndIf;
```

## Sources

* Related public context: [ITS / v8std #std440: Duplicate code usage](https://its.1c.ru/db/v8std#content:440:hdoc)
* Secondary source: [v8std.ru: #std440](https://v8std.ru/std/440/)
