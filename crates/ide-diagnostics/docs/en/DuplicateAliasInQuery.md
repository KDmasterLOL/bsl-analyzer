# Duplicate query source alias (DuplicateAliasInQuery)

## Description

The diagnostic reports two query sources sharing one alias. The platform rejects such a query, and until now the analyzer silently kept only the last source in scope — every reference to that name resolved to it, so mistakes in the other source's fields went unreported entirely.

A source's name is its alias, or the full table name when no alias is given. The comparison is case-insensitive, as everywhere in the query language.

## Examples

Both sources are named `Т`:
```sdbl
ВЫБРАТЬ
	Т.Наименование КАК Значение
ИЗ
	Справочник.Товары КАК Т
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Склады КАК Т
		ПО ИСТИНА
```

The fix is to give the sources distinct names:
```sdbl
ВЫБРАТЬ
	Товары.Наименование КАК Значение
ИЗ
	Справочник.Товары КАК Товары
		ВНУТРЕННЕЕ СОЕДИНЕНИЕ Справочник.Склады КАК Склады
		ПО ИСТИНА
```

## Sources

* [1C:ITS — query language](https://its.1c.ru/db/pubqlang)
