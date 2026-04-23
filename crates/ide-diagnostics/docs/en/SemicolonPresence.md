# Statement should end with semicolon symbol ";" (SemicolonPresence)

## Description

In BSL, the end of line is not a statement terminator. Statements are separated by the semicolon character `;`.

Even in cases where the platform tolerates a missing semicolon, leaving it out makes statement boundaries less explicit and hurts readability.

The current implementation is narrow and syntax-oriented:

- it reports statements that reached HIR lowering without a trailing semicolon;
- it skips labels, empty statements, and statements that already have parse errors;
- it provides an automatic fix that inserts `;` at the end of the reported range.

`Procedure`, `EndProcedure`, `Function`, and `EndFunction` are not ordinary statements and should not end with `;`.

## Examples

### Incorrect

```bsl
А = 1
Б = 2
```

### Correct

```bsl
А = 1;
Б = 2;
```

## Sources

- [Module texts - Standard 1C (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
- [v8std.ru: BSL language](https://v8std.ru/lang/)
