# Invalid character (InvalidCharacterInFile)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Module text should not contain non-breaking spaces or characters that only look
like the normal hyphen-minus `-` but have a different Unicode code point.

These characters typically appear after copying code from office documents,
browsers, or rich-text editors and can cause difficult-to-debug problems.

Typical effects:

- text search stops matching as expected;
- editor assistance and static analysis can behave incorrectly;
- using a wrong dash instead of `-` can produce syntax errors.

The current implementation detects:

- soft hyphen;
- figure dash;
- en dash;
- em dash;
- horizontal bar;
- Unicode minus sign;
- non-breaking space.

## Sources

* Primary source: [ITS / v8std #std456: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Secondary source: [v8std.ru: #std456](https://v8std.ru/std/456/)
