# Unsafe FindByCode() method usage (UnsafeFindByCode)

## Description

This diagnostic reports calls to `FindByCode()` / `НайтиПоКоду()` for metadata objects where code uniqueness is not guaranteed.

The current implementation checks:

- catalogs
- charts of characteristic types
- charts of accounts

The diagnostic is triggered when:

- code uniqueness control disabled (the **Check unique** property is set to `False`)
- or code series enabled not for the entire catalog (the **Code series** property is not equal to `Whole catalog`)

In such cases, `FindByCode()` may return an unexpected object because the same code can exist in more than one place.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->
Incorrect:

```bsl
// Catalog without uniqueness control
Item = Catalogs.Items.FindByCode("001");
```

```bsl
// Chart of accounts with code series not covering the whole chart
Account = ChartsOfAccounts.Management.FindByCode("10");
```

Correct:

```bsl
// Safe when uniqueness is guaranteed
Item = Catalogs.Items.FindByCode("001");
```

```bsl
// Or use another search strategy
Item = Catalogs.Items.FindByDescription("Item");
```

## Sources

* [v8std: UnsafeFindByCode](https://v8std.ru/diagnostics/bslls/UnsafeFindByCode/)
