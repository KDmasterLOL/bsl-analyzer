# Access to an unknown field (UnresolvedField)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

The diagnostic reports field access on an expression whose type is known well enough for the analyzer to say that the requested field does not exist.

This usually means one of the following:

- the field name contains a typo;
- the code expects a different metadata object type;
- the expression can be `Undefined` or of another type, but the current code does not reflect that explicitly.

The current implementation is conservative. It reports only cases where type inference has enough type information to prove that the field lookup fails. For adopted managed forms in configuration extensions, the analyzer also compares controls recorded in the extension's `<BaseForm>` snapshot with the current target configuration: `Items.<Name>` is unresolved when the control came from the old base form but no longer exists in the target base. Controls owned by the extension remain valid.

## Examples

Incorrect:

```bsl
Ref = CommonModule.GetCatalogRef();
Name = Ref.UnknownField;
```

Correct:

```bsl
Ref = CommonModule.GetCatalogRef();
Name = Ref.Description;
```
