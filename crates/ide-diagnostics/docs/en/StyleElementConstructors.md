# Style element constructor (StyleElementConstructors)

## Description

UI appearance should be controlled through style elements rather than by constructing concrete colors, fonts, or borders directly in code. This keeps similar controls visually consistent across forms and makes appearance changes centralized.

The current implementation is narrower than the full style-guidance topic. It reports direct constructors for these style-related types:

- `Color`
- `Font`
- `Border`

It also catches string-based constructor forms such as `New("Color", ...)` and nested constructor usage inside other `New(...)` expressions.

## Examples

### Incorrect

```bsl
Control.TextColor = New Color(255, 0, 0);
```

```bsl
FontData = New ValueStorage(New("Font"));
```

### Correct

```bsl
Control.TextColor = StyleItems.ErrorColor;
```

## Sources

- [Style elements - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:667:hdoc)
- [v8std.ru: StyleElementConstructors](https://v8std.ru/diagnostics/bslls/StyleElementConstructors/)
