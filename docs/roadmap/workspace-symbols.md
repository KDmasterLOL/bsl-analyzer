# Workspace symbols

Состояние: `WorkspaceSymbols::default()` сейчас пустой stub (см.
`crates/ide/src/streaming/orchestrator.rs:330`, TODO в коде).
`workspace/symbol` LSP-запрос либо не отвечает, либо отвечает
частично через name-индекс высокого уровня.

## Цель

Полноценный workspace-symbol-индекс, обслуживающий:

- LSP `workspace/symbol` (`Ctrl+T` / «Go to Symbol in Workspace»),
- did-you-mean подсказки в диагностиках (UnresolvedMethodCall /
  UnresolvedField),
- умное ранжирование кандидатов в completion,
- симметричный поиск по CFE-расширениям и основной конфигурации.

## Принципы реализации

### FST вместо HashMap

`fst::Map` (BurntSushi) для имени-индекса. Причины:

- **Fuzzy/subsequence/prefix-поиск** идёт за время длины запроса,
  а не O(N) по всему workspace. На больших конфигурациях разница
  между «мгновенно» (5-15 ms) и «заметный лаг» (100-300 ms).
- **Компактность.** 100K-500K уникальных имён → FST ≈ 5-15 MB; та же
  HashMap-структура с теми же данными — 15-30 MB.
- **Union при запросе.** FST не поддерживает дешёвый update, но
  поддерживает union FSM. Это идеально ложится на гранулярность
  «индекс на модуль + union при запросе».

### Гранулярность — per-module через Salsa

Три уровня Salsa-tracked query:

```text
module_symbols(ModuleId) -> SymbolIndex     // обновляется при правке модуля
workspace_symbols(SourceRootId) -> SymbolIndex  // union модулей корня
platform_symbols() -> SymbolIndex            // иммутабельный, bsl-platform
```

`workspace/symbol` собирает `Vec<&SymbolIndex>` под текущий запрос
и делает union FST'ов **на стороне запроса**, а не на стороне
индекса. Изменение одного файла инвалидирует только FST его модуля.

### Что лежит в `FileSymbol`

Лёгкая ссылка, не сам символ-объект:

- имя (Cyrillic+ASCII),
- позиция (FileId + TextRange),
- категория (`Method` / `Variable` / `Region` / `MdoObject` /
  `Builtin` / `Platform` / …),
- module-путь,
- флаг `is_export` / `is_alias`.

Lifetime от Salsa (`'db`) — не клонируем строки, держим ссылки в
arena.

### Опции `Query`

По образцу классического `workspace/symbol`:

- `mode`: `Exact` / `Fuzzy` / `Prefix`,
- `case_sensitive: bool` (BSL — case-insensitive, default false),
- `category_filter`: только методы / только типы / только MDO,
- `include_platform: bool`,
- `path_filter`: фильтр по сегментам пути (`Документы::ПКО::…`).

## Прогрев и memory

### Без прогрева

Поведение по умолчанию: первый `Ctrl+T` после старта прогревает
кэш через rayon — параллельный обход модулей, ~1-3 секунды на
ERP-class конфигурацию. Дальше всё мгновенно, пока модули не
меняются.

### Опциональный `prime_caches`

Если задержка первого `Ctrl+T` на больших конфигурациях окажется
неприемлемой — добавить фоновую задачу, прогревающую `module_symbols`
по всем модулям после `initialize`. **Не блокирует** LSP-ответы,
работает в отдельном rayon-пуле, кэш подвержен LRU-эвикции на
общих основаниях.

Настройка: `bsl-analyzer.cachePriming.enable` (по умолчанию `false`).

### Бюджет памяти

- FST на ERP: ~5-15 MB.
- Salsa-overhead (Arc-обёртки на модуль): ~1 MB.
- Итого: ~10-20 MB даже при полностью прогретом состоянии.

Незначительно на фоне общего бюджета процесса (~1.7 GB на ERP),
включать опциональный prime_caches безопасно.

## Did-you-mean diagnostics

Натуральный side-effect FST-индекса. При эмиссии `UnresolvedMethodCall`
/ `UnresolvedField` можно дешёво найти top-N ближайших имён через
тот же fuzzy-автомат и предложить пользователю:

```text
Метод «НайтиПомеру» не найден.
Возможно, вы имели в виду: НайтиПоНомеру, НайтиПоКоду?
```

Без FST подобная подсказка стоила бы O(N) на каждой диагностике,
поэтому реально использовать её можно только после индекса.

## Приоритет

Tier 2.5 в общей дорожной карте. Не блокирует type-inference работу,
но закрывает заметный пробел в LSP-функциональности и переиспользует
тот же шаблон («гранулярные Salsa-tracked индексы + union при
запросе») для последующих workspace-wide фич (`workspace/diagnostic`,
extended find-references).
