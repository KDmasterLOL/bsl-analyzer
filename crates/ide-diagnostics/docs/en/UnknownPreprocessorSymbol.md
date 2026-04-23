# Unknown preprocessor symbol (UnknownPreprocessorSymbol)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Unknown preprocessor symbols in `#If` / `#Если` conditions are invalid. When such a symbol is used, the conditional compilation logic becomes unreliable and the intended code branch may be skipped silently.

Use only symbols supported by the BSL platform preprocessor.

## Examples

Incorrect:

```bsl
#If UnknownSymbol Then
    DoWork();
#EndIf
```

Correct:

```bsl
#If Server Then
    DoWork();
#EndIf
```
