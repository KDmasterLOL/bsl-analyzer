# Ambiguous field in query (AmbiguousFieldInQuery)

## Description

The diagnostic reports a field referenced without a source name when more than one table in the query offers that field. The platform rejects such a query: it cannot choose the source on the author's behalf.

The check fires only when the field is provably present in several sources. If any of them has an incomplete field model (a virtual table, a temporary table, a subquery), ambiguity is not asserted.

## Examples

`Наименование` exists in both joined tables:
```sdbl
ВЫБРАТЬ
	Наименование КАК Значение
ИЗ
	Справочник.Товары КАК Товары
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Склады КАК Склады
		ПО Товары.Ссылка = Склады.Владелец
```

The fix is to name the source explicitly:
```sdbl
ВЫБРАТЬ
	Товары.Наименование КАК Значение
ИЗ
	Справочник.Товары КАК Товары
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Склады КАК Склады
		ПО Товары.Ссылка = Склады.Владелец
```

## The head of a qualified reference

The same ambiguity in a second shape: `Имя.Поле`, where `Имя` is at once a source alias and the name of a field offered by a source at the same query level. The platform rejects such a query, and the fix differs — rename the SOURCE rather than add a qualifier, because the qualifier is already there and is itself the problem.

The rule is about names and is settled before types: colliding with an ordinary String attribute produces the same error as colliding with a tabular section's name. Declaring such an alias is allowed; referencing through it is not. One source suffices — an alias can collide with a field of its own table. Levels do not mix: the same collision inside a subquery is accepted.

```sdbl
ВЫБРАТЬ
	Заказ.Ссылка КАК Значение
ИЗ
	Документ.ЗаказКлиента КАК Заказ
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента.Товары КАК Товары
		ПО Заказ.Ссылка = Товары.Ссылка
```

`Товары` is both the alias of the joined tabular section and a field of the `Заказ` document. The fix is to give the source another name:

```sdbl
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ЗаказКлиента.Товары КАК СтрокиТоваров
		ПО Заказ.Ссылка = СтрокиТоваров.Ссылка
```

## Sources

* [1C:ITS — query language fields](https://its.1c.ru/db/pubqlang)
