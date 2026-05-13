# Break or Continue outside a loop (MisplacedLoopControl)

## Description

`Break` and `Continue` can only be used inside loop bodies: `While`, `For`, or
`For Each`. The diagnostic reports these statements when they appear at method
top level or inside non-loop blocks such as `If` or `Try`.

## Examples

### Incorrect

```bsl
Procedure Test()
    Break;
EndProcedure
```

### Correct

```bsl
Procedure Test()
    While True Do
        Break;
    EndDo;
EndProcedure
```
