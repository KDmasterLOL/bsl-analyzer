# Logical 'OR' in 'JOIN' query section (LogicalOrInJoinQuerySection)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic reports `OR` operators inside query join conditions.

When a join condition combines predicates over different fields with `OR`, the
DBMS may stop using indexes effectively and fall back to scan-heavy execution
plans. That usually means slower queries and less predictable performance.

The current rule is intentionally narrower than “any `OR` is bad”. It does not
report cases where `OR` is used over the same field, because such expressions
can often be normalized to `IN`.

Possible rewrites include:

- splitting the query into separate branches and combining them with
  `UNION ALL`;
- rethinking the join structure;
- moving expensive logic into temporary tables when that preserves semantics.
## Examples
No diagnostic: `OR` over the same field

```bsl
ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Контрагенты КАК Контрагенты
ПО Заказы.Контрагент = Контрагенты.Ссылка
   И (Контрагенты.Рейтинг = 1
     ИЛИ Контрагенты.Рейтинг = 5)
```

Diagnostic: `OR` over different fields

```bsl
ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ПоступлениеТоваров КАК Поступление
ПО ПоступлениеТовары.Ссылка = Поступление.Ссылка
   И (ПоступлениеТовары.Количество > 0
   ИЛИ ПоступлениеТовары.Цена > 0)
```

One possible rewrite:

```bsl
ВЫБРАТЬ *
ИЗ
ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ПоступлениеТоваров КАК Поступление
ПО ПоступлениеТовары.Ссылка = Поступление.Ссылка
   И ПоступлениеТовары.Количество > 0

ОБЪЕДИНИТЬ ВСЕ

ВЫБРАТЬ *
ИЗ
ВНУТРЕННЕЕ СОЕДИНЕНИЕ Документ.ПоступлениеТоваров КАК Поступление
ПО ПоступлениеТовары.Ссылка = Поступление.Ссылка
   И ПоступлениеТовары.Цена > 0
```

Nested joins are also covered:

```bsl
Справочник.Товары КАК Товары
ЛЕВОЕ СОЕДИНЕНИЕ Справочник.Категории КАК Категории
    ЛЕВОЕ СОЕДИНЕНИЕ Справочник.ГруппыКатегорий КАК Группы
    ПО Категории.Группа = Группы.Ссылка
        И (Категории.Активна = ИСТИНА
         ИЛИ Группы.ОбязательнаяПроверка = ИСТИНА)
```

## Sources
- [Standard: Effective Query Conditions, Clause 2 (RU)](https://its.1c.ru/db/v8std/content/658/hdoc)
- [Typical Causes of Suboptimal Query Performance and Optimization Techniques: Using Logical OR in Conditions (RU)](https://its.1c.ru/db/content/metod8dev/src/developers/scalability/standards/i8105842.htm#or)
 - [Public mirror: v8std.ru / #std658](https://v8std.ru/std/658/)
