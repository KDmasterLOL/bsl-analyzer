## Context

В PR #54 `WorkspaceLease::with_ownership` удерживает process-local lifecycle mutex и `writer.lease.lock` на всём времени замыкания. Это необходимо для короткой необратимой публикации, но несовместимо с тремя другими профилями: длительным refresh, сетевой подготовкой и запросным чтением. Текущие wrappers дополнительно теряют происхождение отказа: `None` и `Applied(Err(_))` в отдельных циклах превращаются в одинаковый `Retry`.

Change сохраняет решения `prevent-superseded-daemon-reclaim`: владение определяется токеном, замеченный живой чужой токен необратимо защёлкивает `superseded`, `UNCLAIMED` остаётся временным состоянием, вытесненный демон не пишет общие кэши и может читать только собственный уже открытый graph descriptor.

## Goals / Non-Goals

**Goals:**

- исключить неконтролируемое удержание lease lock на работе, длительность которой зависит от workspace или сети: независимые writes порционируются, а обязательная атомарная transaction получает heartbeat/terminal checkpoints;
- сохранить точную причину отказа через все host wrappers и retry loops;
- не блокировать поступление новых drift events устойчивой ошибкой одной операции;
- не повторять сетевой embedding из-за отказа локальной публикации;
- убрать межпроцессное ожидание с async request threads;
- сделать retry graph build следствием исходного transient refusal.
- сохранить ограниченное время `release()` независимо от размера workspace.

**Non-Goals:**

- менять формат аренды, SQLite-схемы или MCP wire schema/shape/tool contract;
- гарантировать успешную индексацию при устойчивой ошибке диска;
- вводить очередь неограниченного размера, новый worker или общий scheduler;
- переносить reference-поиск под workspace lease;
- автоматически переподключать вытесненный MCP-клиент.

## Review Traceability

| Находка | Корень | Решение |
|---|---|---|
| HIGH-1 | topology refresh целиком под flock | подготовка вне lock, bounded apply batches; atomic shared transaction только с checkpoints |
| HIGH-2 | operation error отображается в `Retry` и держит `pending` | явный `OperationError`, cursor ack и одно coalesced retry-обязательство; nudges независимы |
| HIGH-3 | transient publish refusal повторяет весь embedding pass | ownership gate до сети, сохранённый prepared batch, configured bounded retry |
| HIGH-4/5 | `snapshot()` открывает путь под flock в async handler | request path только из descriptor, заранее открытого publish/adoption path |
| HIGH-6 | `record_load_failure` повторно вызывает `may_build()` | причина отказа передаётся и сохраняется напрямую |

## Decisions

### 1. Один исход сохраняет две независимые оси ошибки

Lease primitive возвращает исход в точке ограды: `Applied(T)`, `TransientRefusal` либо `Terminal`. Он не возвращает `None` с последующим опросом атомиков, потому что состояние может измениться между отказом и классификацией. `Terminal` покрывает и замеченный чужой token, и `release`, но клиентское сообщение о supersession разрешено только когда `is_superseded()` действительно установлен.

Host-level apply contract добавляет четвёртый исход `OperationError(E)` и один раз переводит `Applied(Err(E))` в него. Poisoned search mutex и отсутствие опубликованного engine также являются operation errors, а не lease contention. Это сохраняет две независимые оси: допустила ли ограда callback и чем завершилась сама операция.

Новый универсальный trait или scheduler не вводится. Расширяется существующий `WorkspaceSearchApply`, а остальные callsites используют малый lease outcome напрямую. Для редких атомарных SQLite row loops primitive передаёт малый checkpoint callback, который без повторного захвата lock обновляет heartbeat и возвращает stdlib `ControlFlow::Break(())` при terminal flag. Checkpointed callback после rollback возвращает этот control signal самому primitive; primitive преобразует его в `LeaseOutcome::Terminal` до выхода из fence. `ControlFlow::Continue(Err(E))` остаётся `Applied(Err(E))` и лишь host wrapper превращает его в `OperationError(E)`. Ни callback, ни caller не перечитывает атомики post-hoc; обычные короткие callbacks используют прежнюю форму.

### 2. Lease lock ограничен bounded либо checkpointed общей мутацией

Чтение файлов, построение plan/provider, сетевой вызов, HNSW/контекстный расчёт и обход коллекции выполняются вне `with_ownership`. Под lease lock остаются готовый commit/swap/replace и не более `WORKSPACE_APPLY_BATCH_ROWS = 64` независимо допустимых общих SQLite row/chunk mutations; предел `64` внутренний и не является operator-настройкой.

Для независимо допустимых изменений каждая порция до 64 row/chunk mutations имеет собственную транзакцию и fence. Общая SQLite transaction не считается process-private до `COMMIT`: её WAL/writer lock уже затрагивает shared cache. Поэтому состояние, которое должно стать видимым атомарно, использует один fenced transaction, но row loop внутри него обновляет lease heartbeat и проверяет заранее устанавливаемый `release`/terminal flag не реже чем через 64 элемента; terminal откатывает transaction до выхода из fence. Новое in-memory значение полностью собирается вне fence, а под fence остаётся pointer/map swap. Sidecar сначала сериализуется во временный файл, под fence остаётся только replace.

`release()` атомарно устанавливает terminal flag до ожидания process-local lifecycle mutex. Обычная подготовка замечает его на следующей границе 64 элементов и не входит в fence; fenced atomic transaction получает `ControlFlow::Break(())` на такой же границе, откатывается и возвращает primitive именно terminal control, а не operation error. Это единственное исключение из короткого callback: оно сохраняет существующую атомарность без новой SQLite generation schema либо небезопасной замены открытого DB-файла.

Эта граница применяется ко всем фактическим долгим callbacks PR #54, а не только к первоначальным HIGH:

| Путь | Контракт публикации |
|---|---|
| topology context refresh и full-rescan drift | подготовка вне fence, порции до 64 rows/chunks; mark очищается только после последней порции своего path |
| workspace roots transition | validated plan/in-memory staging вне fence; единый cooperatively-cancellable fenced SQLite transaction и map/index swap без промежуточной смены root table |
| overlay Phase C | сборка нового cache/bundle вне fence; shared fingerprint/embedding writes и map swap выполняются одной fenced transaction с heartbeat/terminal checkpoints после повторной проверки publication baseline |
| bootstrap open/migrations/clear/FTS и directory/fused ingest | filesystem/network preparation вне fence; независимо видимые file groups публикуются отдельными fences, а атомарные migration/clear/FTS и single-file ingest transactions обновляют heartbeat и проверяют terminal внутри fence каждые 64 rows/chunks |
| graph rename/adoption и resident index swap | готовый файл/descriptor/index публикуется одним коротким fence |

Topology context refresh сохраняет прежний `seq_bound`. Если один path содержит больше 64 изменяемых chunks, его mark остаётся до fenced commit последней порции; interruption повторяет оставшуюся либо идемпотентно уже применённую работу. Успешные порции других paths сохраняются, а необработанные marks остаются обязательством.

`release()` может ждать только текущий короткий batch/swap/`LOCK_WAIT` либо следующую границу 64 элементов и rollback атомарной transaction, но не off-fence prepare и не весь workspace. Regression блокирует одну инъецированную порцию, запускает `release()` и доказывает завершение после её отпускания; отдельный atomic-transaction regression доказывает terminal checkpoint, rollback и heartbeat при виртуальном превышении `STALE_AFTER`. Положительный контроль отличает transient contention от настоящего foreign-token terminal latch.

### 3. Drift cursor и retry-обязательство разделены

Hub batch подтверждается после классификации, даже если применение завершилось `OperationError`: cursor должен разрешить материализацию следующих событий. Такой plan сворачивается в одно консервативное `rescan_required`-обязательство с root/topology flags; новые batch coalesce в тот же slot. Full-rescan apply использует те же порции до 64 mutations. Повтор заново материализует текущее состояние диска, поэтому конфликт remove → recreate не применяет устаревший remove и память не растёт с числом ошибок.

Retry переиспользует существующую функцию `retry_delay` и timed wait в том же sink; новый scheduler/thread не создаётся. Свежий drift может разбудить проверку, но не обнуляет накопленный backoff устойчивой ошибки. `nudge_project_reload` и `nudge_rebuild` выполняются по классифицированному drift независимо от результата search Store. `TransientRefusal` сохраняет тот же prepared plan и текущий apply cursor без повторной подготовки; `OperationError` подтверждает hub batch и оставляет coalesced rescan debt; `Terminal` завершает sink.

### 4. Embedding разделяет сетевую подготовку и локальную публикацию

Перед каждым network batch выполняется свежая короткая проверка владения тем же typed lease primitive с пустым callback. Первый `TransientRefusal` — на pre-network gate либо на любой post-network fence — запускает единый deadline текущего embedding obligation. Только `TransientRefusal` расходует этот budget и повторяется; network/Store/poison/подготовительная ошибка является `OperationError`, немедленно переводит obligation в `Failed` и не маскируется как contention.

После получения vectors один in-memory batch сохраняется до terminal исхода его fenced SQLite embedding/cache commit. `TransientRefusal` повторяет только этот commit с backoff и не вызывает embedder повторно; после успешного commit batch можно отбросить и начать следующий. После всех network batches collection отдельно готовит sidecar/resident-index bundle, а overlay — whole-plan Phase C cache/fingerprint bundle из уже сохранённых embeddings. Transient final fence повторяет готовый bundle без network calls. Существующая best-effort семантика самой sidecar I/O ошибки сохраняется; речь идёт об отказе lease admission перед replace.

Fenced workspace path выполняет network batches последовательно: следующий batch не начинается, пока SQLite commit текущего не получил terminal исход. Direct/non-fenced embedding сохраняет существующую concurrency. Это гарантирует ровно один неопубликованный paid batch; отдельный final publication bundle не содержит сетевого обязательства и не требует новой очереди.

`OverlayRetry` сохраняет свой worker и backoff, но получает deadline одного retry-обязательства. Collection embed worker переиспользует ту же функцию задержки и budget в существующем потоке вместо отдельного цикла каждые две секунды. Новые сигналы во время активного obligation coalesce и не продлевают deadline и не сбрасывают streak; только сигнал, пришедший после terminal `Failed` по budget либо `OperationError`, создаёт новое obligation с полным budget.

Budget один раз читается MCP-host конфигурацией рядом с существующим `SearchConfig`, но не добавляется в публичный `bsl_search::SearchConfig` и не влияет на direct/non-fenced API. `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS` имеет default `600`; значение должно быть положительным числом секунд и давать представимый checked `Instant` deadline, иначе единожды пишется warning и используется default. По исчерпании budget статус становится `Failed`, повторы прекращаются до нового внешнего сигнала. При успехе статус становится `Ready`, при terminal supersession/release — существующий terminal `Failed`.

### 5. Request-time snapshot является только дешёвым чтением пула

`GraphState::snapshot()` для request paths только извлекает собственный descriptor из `snapshot_pool` и не читает lease-файл, не открывает общий путь и не ждёт flock. Уже существующий blocking publish/adoption path сначала открывает и валидирует `SNAPSHOT_POOL_CAP = 4` handles вне fence. Затем короткий fence повторно проверяет ownership, ожидаемые persisted graph generation/fingerprint и identity общего path, после чего атомарно устанавливает `Ready` вместе с новым pool. Для fresh build rename выполняется отдельным коротким fence до pre-open; если path изменился между rename/open/final admission, handles отбрасываются и publication retry-ится. Initial publish/adoption без полного проверенного pool не становится `Ready`; reload сохраняет прежнюю публикацию и получает typed failure/retry.

Динамический refill при request-time промахе не вводится: выданный descriptor возвращается в pool, а новый generation получает descriptors от собственного publish/adoption path. При четырёх одновременно занятых handles следующий request немедленно получает прежнюю временную форму отсутствия snapshot. `resolve_names`, `graph` и `symbol_info` используют этот pool-only contract; последний уже находится в blocking resident callback, но проверяется вместе с остальными, чтобы общий API не вернул reopen.

Фоновый `state/sync.rs` не является request path: вычисление referencing modules для XML drift использует отдельный blocking acquisition, который сначала пробует pool, а при miss выполняет fresh typed preflight, один раз открывает descriptor вне fence и затем коротким fence повторно проверяет ownership и ожидаемые generation/path identity. `TransientRefusal`, changed identity либо open error не превращаются в пустой набор модулей: descriptor отбрасывается, plan сохраняет `rescan_required/context` debt и повторяет вычисление. Request miss никогда не вызывает этот путь.

### 6. Причина load failure фиксируется в точке отказа

Build/load path передаёт в `record_load_failure` классифицированную причину: transient ownership refusal, terminal supersession/release либо operation failure. Только исходный `TransientRefusal` взводит `withheld_build`. Повторный `may_build()` после ошибки удаляется, поэтому возврат владения между отказом и записью status не теряет retry-обязательство.

`publish_or_discard` также возвращает исходный typed outcome и больше сам не взводит `withheld_build`: единственная запись retry-флага выполняется в `record_load_failure` по переданной причине. Ошибка rename/build/panic остаётся operation failure даже при отрицательном более позднем probe.

## Risks / Trade-offs

- [Порционный context refresh временно оставляет смешанное состояние] → каждый batch атомарен, оставшиеся marks сохраняются, а готовность объявляется только после исчерпания обязательства.
- [Cursor движется при устойчивой Store error] → coalesced retry slot сохраняет семантический долг без блокировки новых событий и без неограниченной очереди.
- [Пустой snapshot pool даёт временный отказ здоровому владельцу] → выданный descriptor возвращается после запроса, а следующий generation pre-opened при публикации; request miss не открывает общий путь.
- [Prepared vectors занимают память во время lock contention] → одновременно удерживается только один bounded network batch либо один уже несетевой final bundle.
- [Последовательный fenced embedding снижает network parallelism] → ограничение действует только на workspace path с publish fence; direct/non-fenced path сохраняет concurrency.
- [Transient contention длится дольше operator budget] → через 10 минут по умолчанию статус становится `Failed`; активные сигналы deadline не продлевают, оператор может изменить положительный `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS`, а новый сигнал после `Failed` получает новый budget.
- [Fenced atomic SQLite transaction может быть долгой] → она нужна только там, где частичная видимость недопустима; heartbeat и terminal проверяются каждые 64 rows/chunks, `release()` устанавливает flag до mutex и ждёт только следующую границу плюс rollback.
- [Все четыре snapshot descriptors заняты] → следующий request немедленно получает временный miss; background sync использует отдельный fenced acquisition и не теряет context debt.
- [Новый outcome затронет много callsites] → расширяется существующий enum, exhaustive matches и адресные tests не позволяют неявно вернуть старое схлопывание.

## Operational Properties

- Время удержания `writer.lease.lock` ограничено готовым commit/swap/replace, порцией до `WORKSPACE_APPLY_BATCH_ROWS = 64` либо cooperatively-cancellable atomic transaction с heartbeat/terminal checkpoint на той же границе.
- Ни один async MCP handler не выполняет flock либо `GraphDb::open` общего пути напрямую.
- Retry не создаёт новый поток и не растёт по памяти с числом входящих событий.
- Сетевой embedding одного prepared batch выполняется не более одного раза независимо от локальных transient refusals.
- Одно retry-обязательство embedding живёт не более `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS` (`600` по умолчанию).
- `release()` не ждёт workspace-sized/network prepare и не ждёт atomic transaction дальше следующего 64-element checkpoint плюс rollback.
- `Ready` graph всегда публикуется вместе с четырьмя заранее открытыми и повторно проверенными descriptors текущего generation; ни descriptor open, ни request miss не выполняется под fence/request path.
- Тесты используют инъецируемые lock verdict/clock/hooks и не ждут реальный `STALE_AFTER`.

## Migration Plan

Миграции данных нет. Change реализуется поверх rebased PR #54. Сначала вводится outcome и переводятся все его callers, затем отдельно меняются long-running paths и snapshot path. Откат возвращает только этот stacked change; форматы PR #54 остаются совместимыми.

## Open Questions

Отсутствуют. Retry budget согласован: 10 минут по умолчанию, положительное число секунд с представимым checked deadline через `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS`.
