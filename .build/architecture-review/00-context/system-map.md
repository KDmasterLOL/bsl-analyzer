# Контекст и карта системы

## Что считать системой

Система: движок анализа BSL-проектов с несколькими delivery-механизмами:

- LSP server;
- MCP server;
- debug/execution tooling;
- поиск и вспомогательные CLI/extension integration.

## Главный поток данных

`Source text / metadata / project config`
→ parsing
→ semantic model / HIR / types
→ optional CFG/dataflow/SDBL analysis
→ diagnostics / assists / IDE features
→ delivery через LSP/MCP/CLI

## Что ревьюим в первую очередь

- Насколько внутреннее представление языка и проекта независимо от внешних механизмов.
- Насколько сценарии анализа выражены как application layer, а не размазаны по database/query коду.
- Насколько outer layer зависит от внутренних интерфейсов, а не наоборот.

## Спорные зоны

- `bsl-metadata`: domain model или XML/infrastructure adapter.
- `ide-db`: adapter layer или application facade.
- `hir`: entity layer или use-case-support layer.
- `cfg/dataflow`: часть ядра анализа или специализированный application service.

## Фиксация выводов

При ревью дополняем этот файл:

- подтверждёнными границами;
- найденными нарушениями зависимости;
- участками, которые требуют переразделения по слоям.
