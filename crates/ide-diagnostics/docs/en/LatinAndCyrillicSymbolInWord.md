# Mixing Latin and Cyrillic characters in one identifier (LatinAndCyrillicSymbolInWord)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Do not mix Latin and Cyrillic characters inside one identifier.

Such names are hard to read, easy to mistype, and unreliable for search,
review, and refactoring because visually similar letters may belong to
different alphabets, for example `o` / `о`, `c` / `с`, `B` / `В`.

The diagnostic checks mixed-script identifiers in several places, including:

- procedure and function names;
- variable declarations and assignment targets;
- parameters;
- annotation names and annotation parameters;
- region names;
- goto labels.

To reduce noise, the diagnostic allows a common trailing-part pattern by
default, so names like `HTTPСоединение` or `ВИмениEnglish` are not reported.
This behavior can be adjusted through the diagnostic configuration.

## Examples
Invalid:

```bsl
Перем КодТовараВcистеме; // Latin `c` inside a Cyrillic word
Перем Сontрагент;       // Latin `C` inside a Cyrillic word
```

Correct:

```bsl
Перем КодТовараВСистеме;
Перем Контрагент;
```

## Sources
This diagnostic has no direct normative 1C standard source.

Related public context:

* [v8std.ru / bslls / LatinAndCyrillicSymbolInWord](https://v8std.ru/diagnostics/bslls/LatinAndCyrillicSymbolInWord/)
