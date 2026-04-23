# Redundant access to an object (RedundantAccessToObject)

## Description

This diagnostic reports redundant self-access through `ThisObject` or through the current module name.

In object, form, and record set modules, members of the current object can usually be accessed directly without the `ThisObject` prefix. In common and manager modules, calling your own methods through the current module path is also redundant in the supported cases.

## Examples

In an object module, this is redundant:

```bsl
ThisObject.Contractor = GetContractor();
```

Use the property directly:

```bsl
Contractor = GetContractor();
```

In a common module, this is also redundant:

```bsl
Commons.SendMessage("en = 'Hi!'");
```

Call the method directly:

```bsl
SendMessage("en = 'Hi!'");
```
