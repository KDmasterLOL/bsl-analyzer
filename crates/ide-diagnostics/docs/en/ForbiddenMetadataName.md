# Metadata object has a forbidden name (ForbiddenMetadataName)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
<!-- Описание диагностики заполняется вручную. Необходимо понятным языком описать смысл и схему работу -->

Metadata objects, their attributes, tabular sections, dimensions, and resources should not use names that collide with reserved query table names such as `Document`, `Catalog`, or `InformationRegister`.

Such names make queries harder to read, can break the query designer, and may lead to query execution errors.

## Examples
<!-- В данном разделе приводятся примеры, на которые диагностика срабатывает, а также можно привести пример, как можно исправить ситуацию -->

Forbidden names:
- `Catalog.Catalog`
- `Catalog.Products.Attribute.Document`
- `InformationRegister.Settings.Dimension.Documents`

Allowed names:
- `Catalog.ProductKinds`
- `Catalog.Products.Attribute.BaseDocument`
- `InformationRegister.Settings.Dimension.RegisterType`

## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->
<!-- Примеры источников

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Useful information: [Refusal to use modal windows (RU)](https://its.1c.ru/db/metod8dev#content:5272:hdoc)
* Источник: [Cognitive complexity, ver. 1.4](https://www.sonarsource.com/docs/CognitiveComplexity.pdf) -->
* [Standard: Name, Synonym, Comment (RU)](https://its.1c.ru/db/v8std/content/474/hdoc)
* [v8std.ru: #std474](https://v8std.ru/std/474/)
