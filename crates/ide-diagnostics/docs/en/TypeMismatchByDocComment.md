# Type mismatch by doc-comment (TypeMismatchByDocComment)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

This diagnostic reports an argument-position type mismatch where the expected parameter type was taken from a user function's doc-comment rather than from the platform, the metadata model, or the type corpus.

Doc-comment type annotations are written by hand and are frequently incomplete, outdated, or imprecise, so a mismatch derived from them is low-confidence. For that reason this is reported as a smell (informational) rather than as an error: it is a hint to review the call or the doc-comment, not a guaranteed bug.

The corresponding high-confidence cases — where the expected type comes from the platform, metadata, or corpus — are reported by `TypeMismatch` at its normal severity.

This diagnostic is **disabled by default** because of its low confidence; enable it explicitly in the project configuration if you want these hints.

## Examples

```bsl
// Параметры:
//   Значение - Число - описание
Процедура Обработать(Значение) Экспорт
КонецПроцедуры

Обработать("текст"); // type from the doc-comment says Number, a String is passed
```

## Sources

- Internal type-inference based diagnostic in `hir-ty`
