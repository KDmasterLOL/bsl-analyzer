## Why

PR #54 правильно делает замеченное вытеснение необратимым, но один и тот же `with_ownership` сейчас используется для мгновенной публикации, многоминутной обработки и запросного чтения. Из-за этого долгий refresh блокирует heartbeat, ошибки операции схлопываются с временным отказом аренды, сетевой embedding повторяется без границы, а `snapshot()` блокирует async executor. HIGH-1–5 из ревью и issue #71 происходят из смешения трёх профилей операции; HIGH-6 требует того же сохранения первоначальной причины отказа. Они должны закрываться одним контрактом, а не локальными обходами.

## What Changes

- Ограда владения защищает короткие commit/swap и порции до 64 общих SQLite mutations; подготовка, сетевые вызовы и произвольные обходы выполняются вне lease lock. Независимо допустимые изменения публикуются фиксированными порциями. Единственное исключение — требующая атомарности общая SQLite mutation: её transaction остаётся fenced, обновляет heartbeat и проверяет terminal/release каждые 64 элемента, чтобы не публиковать частичное состояние и не задерживать shutdown на весь workspace.
- Lease primitive фиксирует исход в точке ограды, а host wrapper различает успешное применение, временный отказ аренды, терминальное вытеснение/завершение и собственную ошибку операции.
- Search drift не удерживает cursor на одной ошибке: новые события продолжают материализоваться, graph nudges не зависят от успешности search-marking, а незавершённая работа сохраняется как одно объединяемое retry-обязательство с ограниченным backoff.
- Embedding проверяет владение до сетевого batch, не оплачивает повторно уже подготовленный batch при временном отказе публикации и использует существующий backoff с настраиваемым общим budget 10 минут по умолчанию.
- Запросный graph snapshot не берёт межпроцессную блокировку на async-потоке; blocking publish/adoption заранее открывает прежнюю ёмкость descriptor pool вне fence, затем коротко перепроверяет generation/path identity и атомарно устанавливает `Ready` вместе с pool. Единственный фоновый sync-caller использует отдельный blocking fenced-open с сохранением retry-долга при отказе.
- Повтор graph build взводится по сохранённой причине исходного отказа, без повторного probe аренды.

## Capabilities

### New Capabilities

Нет.

### Modified Capabilities

- `superseded-daemon-lifecycle`: уточняется контракт fences, ошибок, retry и request-time чтения, добавленный зависимым change `prevent-superseded-daemon-reclaim`.

## Impact

- `crates/mcp-server/src/workspace_lease.rs` и `state/mod.rs`: типизированный исход ограждённой операции.
- `crates/mcp-server/src/state/{bootstrap,sync,embed}.rs` и существующий `overlay_retry`: bounded retry без блокировки cursor, непроверяемых долгих lease callbacks и повторной сетевой работы.
- `crates/bsl-search/src/{engine,store,workspace_overlay}.rs`: порционные либо cooperatively-cancellable атомарные публикации context/root/drift/bootstrap/overlay мутаций и сохранение подготовленного embedding batch.
- `crates/mcp-server/src/graph/{snapshot,build,state}.rs` и async MCP handlers: дешёвый snapshot path и явная причина отложенной сборки.
- Форматы `writer.lease`, SQLite, MCP wire contract и новые фоновые потоки не добавляются; добавляется только performance knob `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS` с default `600`.

Этот change является stacked-дополнением к PR #54 и не предназначен для отдельного применения к чистому `develop` до `prevent-superseded-daemon-reclaim`.
