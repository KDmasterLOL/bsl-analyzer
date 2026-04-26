# Join with virtual table (JoinWithVirtualTable)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Do not join a query source with a virtual table.

For SDBL queries, prefer joins between metadata objects or temporary tables.
If data from a virtual table is required, read it in a separate step and save
the result into a temporary table before the main join.

The public 1C guidance warns that joins with virtual tables may lead to
unstable or slow execution, especially for large datasets.

## Examples
Invalid:

```bsl
Запрос.Текст =
"ВЫБРАТЬ
|    Накладные.Ссылка,
|    Остатки.КоличествоОстаток
|ИЗ
|    Документ.РасходнаяНакладная КАК Накладные
|    ЛЕВОЕ СОЕДИНЕНИЕ РегистрНакопления.ОстаткиТоваров.Остатки КАК Остатки
|    ПО Накладные.Номенклатура = Остатки.Номенклатура";
```

Preferred approach:

```bsl
Запрос.Текст =
"ВЫБРАТЬ
|    Номенклатура,
|    КоличествоОстаток
|ПОМЕСТИТЬ ВременныеОстатки
|ИЗ
|    РегистрНакопления.ОстаткиТоваров.Остатки
|;
|ВЫБРАТЬ
|    Накладные.Ссылка,
|    Остатки.КоличествоОстаток
|ИЗ
|    Документ.РасходнаяНакладная КАК Накладные
|    ЛЕВОЕ СОЕДИНЕНИЕ ВременныеОстатки КАК Остатки
|    ПО Накладные.Номенклатура = Остатки.Номенклатура";
```

## Sources
* [Standard: Restrictions on SubQuery and Virtual Table Joins (RU)](https://its.1c.ru/db/v8std#content:655:hdoc)
* [Public mirror: v8std.ru / #std655](https://v8std.ru/std/655/)
