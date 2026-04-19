# GetForm method call (GetFormMethod)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

1C recommendations prefer opening forms through the global context method
`OpenForm()` (`OpenFormModal()` for older `8.2`-era projects) instead of first
obtaining a form object with `GetForm()`.

The current implementation is intentionally conservative: it reports any direct
`GetForm()` / `ПолучитьФорму()` call, including object method calls, not only
the exact “get form and then call `Open()`” pattern described in the standard.

## Examples
```bsl
Procedure OpenCatalog()
    Form = GetForm("Catalog.Products.FormList");
    Form.Open();
EndProcedure
```

```bsl
Procedure OpenDocument()
    DocumentObject = Documents.SalesInvoice.CreateDocument();
    Form = DocumentObject.GetForm("DocumentForm");
EndProcedure
```

```bsl
Procedure OpenCatalog()
    OpenForm("Catalog.Products.FormList");
EndProcedure
```

## Sources
* Primary source: [ITS / v8std #std404: Opening forms (RU)](https://its.1c.ru/db/v8std#content:404:hdoc)
* Secondary source: [v8std.ru: #std404](https://v8std.ru/std/404/)
* Secondary source: [v8std.ru: GetFormMethod](https://v8std.ru/diagnostics/bslls/GetFormMethod/)
