# Function returned values description is missing (MissingReturnedValueDescription)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->

The description of a method (procedure or function) should be formatted correctly to help programmers use the functionality correctly.

When an export function already has a documentation comment, that comment must contain a return-value section. You must provide a description for all possible return types.

Missing whole-method documentation is handled by a separate diagnostic: `PublicMethodsDescription`.

> **Dependency:** if `PublicMethodsDescription` is disabled in the config or filtered out, export functions with no documentation comment at all (including value-returning ones) will receive *no* diagnostic — neither from this check, nor from `PublicMethodsDescription`. To cover both cases — missing comment and missing returned-value section — both diagnostics must remain active.

Diagnostics detects typical errors:

- Missing return value description in an existing export-function comment
- Return value description for procedure
- Poor description of the return value: when the type name is present in the description, but its description is not specified
  - To activate this more stringent check, you must turn off the short form permission by the diagnostic parameter

The current implementation has a few important scope details:

- missing returned-value sections are checked only for export functions that already have documentation comments;
- procedures are checked only to ensure they do not contain a returned-value section;
- a completely missing export-function comment is not reported by this diagnostic;
- hyperlink-style comments such as `See OtherMethod()` are skipped;
- the `allowShortDescriptionReturnValues` parameter controls whether a type name alone is accepted.

## Examples

### Incorrect

```bsl
// Calculates the total order amount.
Function TotalOrderAmount(Order) Export
```

### Correct

```bsl
// Calculates the total order amount.
//
// Return value:
//   Number - total amount of all order lines including discounts
Function TotalOrderAmount(Order) Export
```

## Sources
- [Standard: Procedures and functions description (RU)](https://its.1c.ru/db/v8std#content:453:hdoc)
- [v8std.ru: MissingReturnedValueDescription (RU)](https://v8std.ru/diagnostics/bslls/MissingReturnedValueDescription/)
