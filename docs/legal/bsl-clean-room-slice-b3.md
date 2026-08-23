# BSL clean-room slice B3 — grammar attestation

Слайс аттестует четыре файла BSL-грамматики по главе 4 руководства разработчика
1С:Предприятия 8.3.27. Для каждой функции фиксируется одно из трёх оснований,
а обнаруженные расхождения разбираются ниже как D1–D10. Исполняемый checklist в
`crates/parser/tests/grammar_attestation.rs` хранит полные построчные
обоснования; таблица в этом документе — проверяемый инвентарь тех же вердиктов.

- `R` — the grammar form is required by the documented language.
- `A` — an explicitly accepted recovery or editor allowance.
- `D` — an unresolved inherited form; none are permitted at completion.

## Findings and decisions

### D1 — `Тогда` сохраняется при ошибке условия обычного `Если`

Глава 4 отделяет условие от тела словом `Тогда`. Без локальной границы ошибка
или ещё не набранное условие потребляет этот токен и затем ложно сообщает, что
он отсутствует. Решение: `statements.rs::at_then` принадлежит правилу обычного
`Если` и оставляет `Тогда` ожидающему его заголовку.

### D2 — одноимённая граница препроцессора остаётся отдельной

У `#Если … Тогда` та же текстовая граница, но другой владелец и другая область
восстановления (раздел 4.8.1.2). Общий predicate перестал бы различать две
позиции одинакового токена. Решение: сохранить отдельный
`grammar.rs::at_then`; две функции намеренно совпадают по телу, но не по
контексту.

### D3 — скобки в условиях препроцессора приняты как совместимость

Продукция 4.8.1.2 задаёт плоскую цепочку `[НЕ] <Символ>` с `И`/`Или` и не
описывает скобки. В проверенном корпусе из 75 438 файлов они встречаются 29 раз
в коде, который собирает платформа. Решение: оставить скобки allowance в
`preproc_logical_operand`, не вводя отсутствующий в источнике приоритет `И` над
`Или`.

### D4 — вложенная аннотация не является значением параметра

Раздел 4.8.2 разрешает значение параметра аннотации, но не другую аннотацию;
пользовательские аннотации также не заявлены. Унаследованная ветка принимала
`&Перед(&НаКлиенте)`, хотя в том же корпусе форма не встретилась ни разу.
Решение: ветка удалена, а `annotation_param_value` следует форме раздела 4.8.2.

### D5 — keyword в имени процедуры сохраняет тело для диагностики

Раздел 4.2.4.6 запрещает ключевые слова в именах объявляемых методов. Полный
отказ parser на `Процедура Если()` потерял бы всё тело. Решение: grammar
принимает токен как имя, а существующая `ReservedWordAsMethodName` выдаёт
Blocker; это recovery allowance, не расширение языка.

### D6 — то же разделение применяется к функции

Для `Функция Новый()` действует тот же запрет 4.2.4.6 и та же цена потери тела.
Решение симметрично D5: `function_def_content` сохраняет дерево, нарушение
сообщает `ReservedWordAsMethodName`.

### D7 — `Экспорт` после списка переменных следует примерам источника

Раздел 4.6.1 противоречив: продукция ставит `[Экспорт]` после первого имени,
проза требует его для каждой переменной, а пример показывает
`Перем А, Б Экспорт;`. Решение: принимать две показанные примером формы — один
экспортируемый идентификатор и `Экспорт` после всего списка; не принимать
неподтверждённое `Перем А Экспорт, Б;`.

### D8 — `Выполнить` без скобок сохраняется по корпусу

Раздел 4.6.8 показывает `Выполнить(<Строка>)`, однако в корпусе обнаружена 41
бесскобочная форма против 1833 скобочных. Решение: `execute_stmt` принимает обе,
чтобы не выдавать ложные ошибки на коде, который принимает платформа.

### D9 — унарные `+`/`-` пока не получают отдельный узел

Приоритет соответствует строке 7 таблицы 4.5.4 и проверен относительно
умножения и postfix-операций. Но `Не` строит `UnaryExpr`, а `+`/`-` остаются
токенами рядом с операндом. Выравнивание изменило бы дерево каждого такого
выражения и всех его потребителей. Решение: сохранить совместимость дерева;
долг и ненаблюдаемая граница precedence вынесены в issues #50 и #51.

### D10 — недоопределённые и редакторские формы зафиксированы явно

Оставшиеся allowance не принимаются как неявное наследство:

- имя формального параметра временно необязательно, чтобы незавершённый
  `Процедура П(Знач …)` не разрушал заголовок (4.6.3/4.6.4);
- `Перейти Метка` принимается наряду с примером `Перейти ~Метка` из 4.6.7,
  сохраняя имя во время набора тильды;
- `[]` размещена на postfix-уровне вместе с `.` и `()`: разделы 4.2.5, 4.7.1 и
  4.7.5 задают форму индекса, но не её приоритет; это размещение сохраняет
  левое связывание `а.б[0].в`;
- голое `Новый` принимается только как editor recovery до появления имени типа
  или функциональных скобок; языковые варианты остаются вариантами 4.6.2.

У каждого решения есть отдельное обоснование в typed checklist; D10 не означает
открытый verdict. На завершении слайса все строки разрешены в `R` или `A`.

| Grammar function | Verdict |
|---|---|
| `src/grammar.rs:annotated_item` | A |
| `src/grammar.rs:source_file` | R |
| `src/grammar.rs:preprocessor_region` | A |
| `src/grammar.rs:preprocessor_end_region` | A |
| `src/grammar.rs:preprocessor_if` | R |
| `src/grammar.rs:at_then` | A |
| `src/grammar.rs:at_paren_list_punctuation` | A |
| `src/grammar.rs:at_closing_paren` | A |
| `src/grammar.rs:at_closing_bracket` | A |
| `src/grammar.rs:at_declaration_start` | A |
| `src/grammar.rs:at_preproc_closer` | A |
| `src/grammar.rs:preproc_content` | A |
| `src/grammar.rs:preproc_expression` | R |
| `src/grammar.rs:preproc_logical_expression` | R |
| `src/grammar.rs:preproc_logical_operand` | A |
| `src/grammar.rs:preprocessor_delete` | A |
| `src/grammar.rs:preprocessor_insert` | A |
| `src/grammar.rs:preproc_symbol` | R |
| `src/grammar/items.rs:compiler_directive` | R |
| `src/grammar/items.rs:annotation` | R |
| `src/grammar/items.rs:annotation_params` | R |
| `src/grammar/items.rs:annotation_param` | R |
| `src/grammar/items.rs:annotation_param_value` | R |
| `src/grammar/items.rs:procedure_def` | R |
| `src/grammar/items.rs:at_end_procedure` | A |
| `src/grammar/items.rs:at_end_function` | A |
| `src/grammar/items.rs:procedure_def_content` | A |
| `src/grammar/items.rs:function_def` | R |
| `src/grammar/items.rs:function_def_content` | A |
| `src/grammar/items.rs:param_list` | R |
| `src/grammar/items.rs:param` | A |
| `src/grammar/items.rs:var_declaration` | R |
| `src/grammar/items.rs:var_declaration_content` | A |
| `src/grammar/statements.rs:stmt_list` | A |
| `src/grammar/statements.rs:expect_stmt_list_terminator` | A |
| `src/grammar/statements.rs:statement` | A |
| `src/grammar/statements.rs:return_stmt` | R |
| `src/grammar/statements.rs:at_end_do` | A |
| `src/grammar/statements.rs:at_then` | A |
| `src/grammar/statements.rs:at_handler_comma` | A |
| `src/grammar/statements.rs:at_do` | A |
| `src/grammar/statements.rs:at_to_or_do` | A |
| `src/grammar/statements.rs:at_if_closer` | A |
| `src/grammar/statements.rs:at_try_closer` | A |
| `src/grammar/statements.rs:if_stmt` | R |
| `src/grammar/statements.rs:while_stmt` | R |
| `src/grammar/statements.rs:for_stmt` | R |
| `src/grammar/statements.rs:try_stmt` | R |
| `src/grammar/statements.rs:raise_stmt` | R |
| `src/grammar/statements.rs:at_end_of_statement` | A |
| `src/grammar/statements.rs:parse_raise_call_args` | R |
| `src/grammar/statements.rs:goto_stmt` | A |
| `src/grammar/statements.rs:label_stmt` | R |
| `src/grammar/statements.rs:execute_stmt` | A |
| `src/grammar/statements.rs:add_handler_stmt` | R |
| `src/grammar/statements.rs:remove_handler_stmt` | R |
| `src/grammar/statements.rs:assignment_or_call` | A |
| `src/grammar/statements.rs:stmt_list_inner` | A |
| `src/grammar/expressions.rs:expression` | R |
| `src/grammar/expressions.rs:postfix_expression_for_assignment` | A |
| `src/grammar/expressions.rs:or_expr` | R |
| `src/grammar/expressions.rs:and_expr` | R |
| `src/grammar/expressions.rs:not_expr` | R |
| `src/grammar/expressions.rs:comparison_expr` | R |
| `src/grammar/expressions.rs:additive_expr` | R |
| `src/grammar/expressions.rs:multiplicative_expr` | R |
| `src/grammar/expressions.rs:unary_expr` | A |
| `src/grammar/expressions.rs:postfix_expr` | A |
| `src/grammar/expressions.rs:postfix_expr_with_call_info` | A |
| `src/grammar/expressions.rs:primary_expr` | R |
| `src/grammar/expressions.rs:string_literal` | R |
| `src/grammar/expressions.rs:at_adjacent_string_literal` | R |
| `src/grammar/expressions.rs:string_continuation_tail` | R |
| `src/grammar/expressions.rs:await_expr` | R |
| `src/grammar/expressions.rs:new_expr` | A |
| `src/grammar/expressions.rs:continues_the_surrounding_expression` | A |
| `src/grammar/expressions.rs:ternary_expr` | R |
| `src/grammar/expressions.rs:arg_list` | R |
