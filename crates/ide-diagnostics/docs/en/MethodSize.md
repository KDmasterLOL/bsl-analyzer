# Method size (MethodSize)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A large method is harder to understand, test, and maintain.

Methods usually become too large when new logic keeps being added directly to
the same procedure or function instead of being extracted into smaller parts.

The current implementation uses a configurable line-count threshold:

- `maxMethodSize`, `200` by default.

Practical refactoring heuristics:

- if you want to add a comment explaining a code block, that block may deserve a
  separate method with a meaningful name;
- if one method performs several subtasks, split them into focused helper
  methods.

## Examples

Invalid:

```bsl
Procedure ProcessDocument(Document)
    // 200 lines of validation, calculations, persistence, notifications
EndProcedure
```

Better:

```bsl
Procedure ProcessDocument(Document)
    ValidateDocument(Document);
    CalculateTotals(Document);
    SaveDocument(Document);
    NotifyUser(Document);
EndProcedure
```

## Sources

This diagnostic has no direct normative 1C standard source.

Related public context:

- [Martin Fowler: Refactoring](https://www.refactoring.com/)
- [Refactoring tools in 1C (RU)](https://v8.1c.ru/o7/201312ref/index.htm)
