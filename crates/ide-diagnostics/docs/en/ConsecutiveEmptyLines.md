# Consecutive Empty Lines (ConsecutiveEmptyLines)

## Description

A single empty line is usually enough to separate neighboring blocks of code.
When two or more empty lines appear in a row, they rarely add structure and
instead make the module visually longer and less dense.

This diagnostic reports groups of consecutive empty lines that exceed the
configured limit. By default, one empty line is allowed.

## Examples

Incorrect:

```bsl
Procedure Run()


    PrepareData();
EndProcedure
```

Correct:

```bsl
Procedure Run()

    PrepareData();
EndProcedure
```

## Sources

- [v8std: module-consecutive-blank-lines](https://v8std.ru/diagnostics/v8-code-style/module-consecutive-blank-lines/)
