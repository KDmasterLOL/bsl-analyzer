# Partially localized text is used in the StrTemplate function (MultilingualStringUsingWithTemplate)

## Description

This diagnostic is a narrower variant of multilingual-string validation: it checks only `NStr(...)` / `НСтр(...)` values that are used as templates for `StrTemplate(...)` / `СтрШаблон(...)`.

If a required language is missing, `NStr` may return an empty string for that language. When such an empty string is used as a template, `StrTemplate` may fail.

The current implementation is configuration-driven and syntax-based:

- required languages come from the `declaredLanguages` setting;
- by default the rule assumes only `ru`;
- it checks `NStr` directly inside `StrTemplate`, and also `NStr` assigned to a variable later used in `StrTemplate`;
- `NStr` outside template context is intentionally ignored by this rule;
- empty `NStr()` in template context is reported as missing all declared languages.

## Examples

### Incorrect

Configuration declares `ru` and `en`.

```bsl
Text = StrTemplate(NStr("ru = 'Processed %1 of %2 documents'"), Done, Total);
```

### Correct

```bsl
Text = StrTemplate(
    NStr("ru = 'Обработано %1 из %2 документов'; en = 'Processed %1 of %2 documents'"),
    Done,
    Total
);
```

## Sources

- [Localization requirements (RU)](https://its.1c.ru/db/v8std/content/763/hdoc)
- [v8std.ru: MultilingualStringUsingWithTemplate](https://v8std.ru/diagnostics/bslls/MultilingualStringUsingWithTemplate/)
