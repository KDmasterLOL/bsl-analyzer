# Local variable used before definition (LocalVariableUsedBeforeDefinition)

A loop-local variable introduced by `For` or `For Each` is read before execution reaches the loop definition. 1C treats this as a compile-time error.

## Incorrect

```bsl
Row.Marked = False;
For Each Row In Rows Do
    // ...
EndDo;
```

## Correct

Move the use inside the loop or define a different value before the first read.
