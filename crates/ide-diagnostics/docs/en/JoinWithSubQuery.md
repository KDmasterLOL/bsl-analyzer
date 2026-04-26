# Join with sub queries (JoinWithSubQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Do not join a query source with a subquery result.

For SDBL queries, prefer joins between metadata objects or temporary tables.
If a subquery must be prepared first, move it into a separate batch step and
store the result in a temporary table.

The public 1C guidance warns that joins with subqueries may lead to:

- very slow execution even under light load;
- unstable performance depending on data distribution and statistics;
- noticeably different plans on different DBMS;
- high sensitivity to outdated database statistics.

## Examples
Invalid:

```bsl
Запрос.Текст =
"ВЫБРАТЬ
|    Продажи.Ссылка
|ИЗ
|    Документ.РеализацияТоваровУслуг КАК Продажи
|    ЛЕВОЕ СОЕДИНЕНИЕ (
|        ВЫБРАТЬ Остатки.Номенклатура
|        ИЗ РегистрНакопления.ОстаткиТоваров.Остатки КАК Остатки
|        ГДЕ Остатки.Склад = &Склад
|    ) КАК Остатки
|    ПО Продажи.Товар = Остатки.Номенклатура";
```

Preferred approach:

```bsl
Запрос.Текст =
"ВЫБРАТЬ
|    Остатки.Номенклатура
|ПОМЕСТИТЬ ВременныеОстатки
|ИЗ РегистрНакопления.ОстаткиТоваров.Остатки КАК Остатки
|ГДЕ Остатки.Склад = &Склад
|;
|ВЫБРАТЬ
|    Продажи.Ссылка
|ИЗ
|    Документ.РеализацияТоваровУслуг КАК Продажи
|    ЛЕВОЕ СОЕДИНЕНИЕ ВременныеОстатки КАК Остатки
|    ПО Продажи.Товар = Остатки.Номенклатура";
```

## Sources
* [Standard: Restrictions on SubQuery and Virtual Table Joins (RU)](https://its.1c.ru/db/v8std#content:655:hdoc)
* [Public mirror: v8std.ru / #std655](https://v8std.ru/std/655/)
