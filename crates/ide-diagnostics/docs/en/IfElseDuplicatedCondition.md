# Duplicated conditions in If...Then...ElseIf... statements (IfElseDuplicatedCondition)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports repeated conditions inside the same `If` / `ElseIf`
chain.

When the same condition appears again later in the chain, that later branch is
effectively unreachable because the earlier branch already handled the same
case. In practice this usually means a copy-paste error or an unfinished edit.

## Examples

```bsl
If p = 0 Then
    t = 0;
ElseIf p = 1 Then
    t = 1;
ElseIf p = 1 Then
    t = 2;
Else
    t = -1;
EndIf;
```

```bsl
If p = 0 Then
    t = 0;
ElseIf p = 1 Then
    t = 1;
ElseIf p = 2 Then
    t = 2;
Else
    t = -1;
EndIf;
```

## Sources

No direct 1C standard is used as the normative basis for this diagnostic.
It is a local suspicious-pattern rule implemented in `bsl-analyzer`.
