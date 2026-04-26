# Double negatives (DoubleNegatives)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Double negatives make expressions harder to read and easier to misinterpret. It is usually better to rewrite them into a direct positive or direct comparison form.

The current implementation reports three structural patterns:

- `Not (Not X)`
- `Not (X <> Y)`
- `(Not X) <> Y`

It intentionally skips more complex expressions that contain logical `AND` / `OR` inside the candidate fragment, because such cases are more likely to need manual review than a mechanical rewrite.

## Examples

### Incorrect

```bsl
If Not ValueTable.Find(SearchValue, "Column") <> Undefined Then
    // Do the action
EndIf;
```

### Correct

```bsl
If ValueTable.Find(LookupValue, "Column") = Undefined Then
    // Perform action
EndIf;
```

## Sources

- [Refactoring Catalog: Remove Double Negative](https://www.refactoring.com/catalog/removeDoubleNegative.html)
