# There is a localized text for all languages declared in the configuration (MultilingualStringHasAllDeclaredLanguages)

## Description

In a multilingual configuration, `NStr(...)` / `НСтр(...)` should contain text for every language that the project declares as required.

If a required language is missing, the expression may return an empty string for that language.

The current implementation is configuration-driven and syntax-based:

- required languages come from the `declaredLanguages` setting;
- by default the rule assumes only `ru`;
- it parses language keys from the first string literal argument of `NStr`;
- empty `NStr()` calls are reported as missing all declared languages;
- some `NStr` uses inside `StrTemplate` / `СтрШаблон` flows are intentionally skipped to avoid noisy false positives.

## Examples

### Incorrect

Configuration declares `ru` and `en`.

```bsl
Message = NStr("ru = 'Document saved successfully'");
```

### Correct

```bsl
Message = NStr("ru = 'Документ успешно записан'; en = 'Document saved successfully'");
```

## Sources

- [Localization requirements - Standard 1C (RU)](https://its.1c.ru/db/v8std/content/763/hdoc)
- [v8std.ru: MultilingualStringHasAllDeclaredLanguages](https://v8std.ru/diagnostics/bslls/MultilingualStringHasAllDeclaredLanguages/)
