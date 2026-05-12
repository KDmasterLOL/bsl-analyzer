# DuplicatedInsertionIntoCollection

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика ищет повторные вставки одного и того же ключа или значения в одну
коллекцию. Прямого стандарта в локальном `v8std` нет; есть публичная страница
BSLLS в `v8std.ru`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/duplicated_insertion_into_collection.rs`
- `crates/ide-diagnostics/docs/ru/DuplicatedInsertionIntoCollection.md`
- `docs/legal/diagnostics/DuplicatedInsertionIntoCollection.md`
- `crates/hir-def/src/body/lower/diagnostics.rs`

## Как реализовано

Проверка работает по HIR body. `InsertionTracker` отслеживает вызовы
`Добавить`/`Add` и `Вставить`/`Insert`, хеширует receiver и аргументы, учитывает
поколения переменных и scope depth. Для `Insert` сравнивается ключ, для `Add`
по конфигу `isAllowedMethodADD` можно разрешить или запретить анализ.

## Что покрыто

Тесты покрывают repeated insertion, разные коллекции/ключи, специальные
значения (`0`, пустая строка, `Null`, `Undefined`, `Символы.*`), изменение
переменных и breaker-context.

## Пробелы и ограничения

- Сравнение выражений основано на локальном хеше HIR, без полноценного alias и
  interprocedural dataflow.
- Hash collision практически маловероятен, но теоретически возможен.
- Конфиг `isAllowedMethodADD` назван не очень ясно: по смыслу это "анализировать
  Add как дубликат".
- HIR не лоуэрит код внутри `#Если/#Иначе`, поэтому дубликаты по веткам
  препроцессора не ловятся (известное ограничение, см. `test_preprocessor_duplicate`).
- Нет quick-fix: удалять дубликат автоматически безопасно не всегда.

## Может ли инфраструктура улучшить качество

Да. Нужен общий collection-mutation analyzer с expression equality, alias
tracking и пониманием control-flow exits. Он пригодится также для
`DeletingCollectionItem`, `SelfInsertion`, будущих правил по структурам/массивам.

## Возможное объединение

Сливать внешний код с `DeletingCollectionItem` не нужно: там unsafe mutation во
время обхода, здесь suspicious duplicate. Внутренне стоит объединить tracking
collection receiver и аргументов.

## Закрыто Track 6.3

Гэп из секции "Пробелы и ограничения" — «HIR не лоуэрит код внутри
`#Если/#Иначе`, поэтому дубликаты по веткам препроцессора не ловятся» —
устарел и закрыт.

- HIR **лоуэрит** код в препроцессорных ветках с момента появления
  `lower_preproc_if` (`crates/hir-def/src/body/lower/preproc.rs`); каждая
  ветка получает свой `Stmt::PreprocIf` HIR-нод с собственным `Box<[StmtIdx]>`.
- Handler **изолирует** ветки через общий итератор
  `PreprocIfStmt::branches()`, добавленный в Phase B Slice 1 коммитом
  `a6a2d18b` и потреблённый в Phase C handler-refactor'е (`d4ab7303`).
- Cross-branch повторение `Массив.Добавить(X)` в `#Если` и `#Иначе` —
  это **не дубликат**, а параллельные компиляции (см. `test_preprocessor_duplicate`).
  Внутри одной ветки повторения **флагаются**.

Track 6.3 fixture-first audit добавил 3 теста (`91d7871b`):
- `test_preprocessor_intra_branch_dup` — intra-`#Если` повторение → 1 диагностика.
- `test_preprocessor_mixed_intra_dup_with_cross_branch_same` — intra-then дубль
  + ещё одно вхождение в `#Иначе` → ровно 1 диагностика на intra-then пару;
  cross-branch не флагается.
- `test_preprocessor_nested_intra_branch_dup` — повторение во вложенной
  `#Если внутри #Если` ветке → 1 диагностика.

Никаких изменений в коде самой диагностики (за исключением Phase B
рефакторинга на shared iterator) не понадобилось. Active-symbol pruning
не вводится по причинам, описанным в `TRACK_6_3_CLOSURE.md`.

## Вывод

Реализация уже сложная и полезная. Главный долг - переиспользуемый analyzer
коллекций и более ясная конфигурация.

