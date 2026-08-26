## ADDED Requirements

### Requirement: Ограждённая операция сохраняет причину исхода

Каждый lease primitive MUST фиксировать допуск, временный отказ или terminal stop в точке ограды без последующего опроса изменяемых флагов. Mid-callback checkpoint MUST возвращать primitive внутренний `ControlFlow::Break` после rollback; primitive MUST преобразовывать его в terminal outcome до выхода из fence, не в `Applied(Err)`/`OperationError`. Каждый host-level вызов записи общего workspace-кэша MUST дополнительно различать собственную ошибку операции.

#### Scenario: Операция завершилась ошибкой под действующей арендой

- **WHEN** lease fence допустил callback, а callback вернул ошибку Store, SQLite, mutex либо подготовленного plan
- **THEN** caller получает `OperationError` с исходной причиной
- **AND** retry policy не классифицирует эту ошибку как lock contention

#### Scenario: Lease lock временно недоступен

- **WHEN** callback не начинался из-за временно занятого lock или `UNCLAIMED` без замеченного чужого token
- **THEN** caller получает `TransientRefusal`
- **AND** terminal supersession не устанавливается

#### Scenario: Замечен живой чужой token

- **WHEN** fence читает действующую чужую запись после собственного владения
- **THEN** caller получает terminal outcome
- **AND** необратимый `superseded` и запрет последующих записей сохраняются

#### Scenario: Release остановил ограждённую операцию

- **WHEN** `release()` установлен до допуска следующего callback
- **THEN** lease primitive возвращает terminal outcome без выполнения callback
- **AND** caller не сообщает наблюдение чужого token, если `is_superseded()` не установлен

#### Scenario: Release пришёл после допуска атомарной операции

- **WHEN** callback уже начал SQLite transaction, а checkpoint после `release()` возвращает `ControlFlow::Break`
- **THEN** callback откатывает transaction и lease primitive возвращает ровно terminal outcome до выхода из fence
- **AND** этот исход не проходит через `Applied(Err)` или `OperationError` и не требует post-hoc чтения flags

### Requirement: Длительная работа сохраняет lease liveness и bounded release

Работа, стоимость которой зависит от числа workspace-объектов, файлов, chunks либо задержки сети, MUST готовиться вне lease lock. Независимо допустимые shared SQLite mutations MUST публиковаться fenced-порциями не более `WORKSPACE_APPLY_BATCH_ROWS = 64`. Требующая атомарности shared SQLite transaction MUST оставаться внутри fence, обновлять heartbeat и проверять заранее устанавливаемый terminal/release flag каждые 64 элемента; terminal MUST откатывать её до выхода. In-memory value и sidecar MUST готовиться до fenced swap/replace. Новый scheduler, thread, schema либо operator batch knob MUST NOT добавляться.

#### Scenario: Полный topology context refresh превышает stale interval

- **WHEN** topology refresh требует не менее 65 row/chunk mutations и длится дольше `STALE_AFTER`
- **THEN** heartbeat обновляет запись между batch
- **AND** живой владелец не становится stale из-за собственного refresh
- **AND** выполняются как минимум два fence допуска, завершённые batch сохраняются, а mark одного path очищается только после его последней порции

#### Scenario: Вытеснение замечено между batch

- **WHEN** другой владелец захватил аренду после одного успешного batch
- **THEN** следующий fence возвращает terminal outcome
- **AND** worker прекращает общие мутации без отката уже завершённых batch

#### Scenario: Shutdown пришёл во время короткой публикации

- **WHEN** один ограниченный apply batch уже допущен, а другой поток вызывает `release()`
- **THEN** `release()` может ждать только завершение текущего batch и `LOCK_WAIT`, но не workspace-sized prepare
- **AND** после release следующий batch не допускается

#### Scenario: Shutdown пришёл во время атомарной SQLite transaction

- **WHEN** root transition, overlay Phase C, bootstrap migration/FTS либо иной атомарный plan обрабатывает более 64 shared rows внутри fenced transaction, а другой поток вызывает `release()`
- **THEN** `release()` устанавливает terminal flag до ожидания lifecycle mutex
- **AND** transaction обновляет heartbeat и замечает flag не позднее следующей границы 64 элементов, откатывается и освобождает fence без частичной публикации

#### Scenario: Атомарная публикация потеряла аренду перед commit

- **WHEN** workspace roots transition, overlay Phase C либо bootstrap Store mutation полностью подготовлена, но admission получает `TransientRefusal` либо terminal checkpoint срабатывает внутри transaction
- **THEN** промежуточная root/cache/schema публикация не становится видимой
- **AND** transient сохраняет то же подготовленное обязательство, а terminal откатывает его без дальнейшей общей записи

#### Scenario: Full rescan превышает одну порцию

- **WHEN** drift rescan требует более 64 независимо допустимых row/chunk mutations
- **THEN** ни один lease callback не выполняет больше 64 mutations
- **AND** interruption сохраняет `rescan_required` и остаток plan до последующего convergent apply

#### Scenario: Fused ingest одного большого файла остаётся атомарным

- **WHEN** ingest одного файла требует более 64 chunks
- **THEN** один fenced file transaction обновляет heartbeat и проверяет terminal не реже каждой границы 64 chunks
- **AND** terminal откатывает весь file update, не оставляя частично видимые chunks/hash

### Requirement: Drift failure не блокирует новые события и graph nudges

Устойчивая ошибка search drift apply MUST не удерживать hub cursor на одном batch и MUST не блокировать независимые graph nudges. Неисполненная search-работа MUST сохраняться как ограниченное объединяемое retry-обязательство.

#### Scenario: Первый batch получил устойчивую Store error

- **WHEN** применение первого materialized batch возвращает `OperationError`, а затем приходит второй file change
- **THEN** первый batch подтверждается для продвижения cursor и его долг coalesce-ится в retry-обязательство
- **AND** второй change материализуется и добавляется к тому же обязательству
- **AND** память не растёт на один pending batch для каждой ошибки

#### Scenario: Удалённый файл появился снова до retry

- **WHEN** failed batch содержал removal, cursor продвинулся, а до retry тот же путь появился снова
- **THEN** coalesced `rescan_required` повторно материализует текущее состояние диска
- **AND** устаревший removal не применяется поверх появившегося файла

#### Scenario: Search marking упал для topology drift

- **WHEN** классификация требует `nudge_project_reload` либо `nudge_rebuild`, а search Store mutation возвращает ошибку
- **THEN** требуемый graph nudge всё равно выполняется
- **AND** search retry следует bounded backoff без busy loop

#### Scenario: Аренда временно отказала до apply

- **WHEN** prepared drift plan получает `TransientRefusal`
- **THEN** тот же plan сохраняется для повторного короткого apply
- **AND** terminal outcome завершает sink без дальнейших записей

#### Scenario: Full rescan apply завершился собственной ошибкой

- **WHEN** bounded rescan batch возвращает `OperationError` после продвижения hub cursor
- **THEN** остаётся ровно один coalesced `rescan_required` slot с текущим apply cursor/root/topology flags
- **AND** свежий drift может разбудить retry, но не сбрасывает backoff устойчивой ошибки

### Requirement: Embedding retry не повторяет оплаченную сетевую работу

Collection и overlay embedding workers MUST проверять возможность владения до каждого network batch, хранить один prepared result до terminal исхода его SQLite embedding/cache commit и использовать один deadline текущего obligation. После всех batches отдельный готовый sidecar/index либо whole-plan Phase C bundle MUST повторяться при transient final admission без network calls. Только `TransientRefusal` MUST повторяться и расходовать budget; network, Store, poison и подготовительная ошибки MUST немедленно завершать obligation как `OperationError`.

#### Scenario: Владение отсутствует до сетевого batch

- **WHEN** свежая проверка возвращает временное отсутствие владения
- **THEN** сетевой embedder не вызывается
- **AND** первый такой `TransientRefusal` запускает общий deadline и повтор планируется существующим bounded retry driver

#### Scenario: Lock занят после получения vectors

- **WHEN** network batch завершён, но его fenced SQLite embedding/cache commit получает `TransientRefusal`, либо готовый final sidecar/index/Phase C bundle не допущен
- **THEN** сохраняется только текущий paid batch либо уже несетевой final bundle и повторяется только отказавшая публикационная фаза
- **AND** embedder для этого batch вызван ровно один раз
- **AND** настоящая Store/network ошибка не переклассифицируется в `TransientRefusal`; существующая best-effort sidecar I/O policy сохраняется

#### Scenario: Retry budget исчерпан

- **WHEN** временный отказ сохраняется до настроенного deadline
- **THEN** worker прекращает повтор без busy loop
- **AND** semantic runtime покидает `Indexing` и сообщает `Failed`

#### Scenario: Retry budget настроен допустимым значением

- **WHEN** `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS` содержит положительное число секунд и checked deadline представим
- **THEN** одно transient publish obligation использует этот deadline вместо default `600`

#### Scenario: Retry budget настроен недопустимым значением

- **WHEN** `EMBEDDING_PUBLISH_RETRY_BUDGET_SECS` содержит invalid, zero либо не дающее представимого checked deadline значение
- **THEN** host единожды пишет warning и использует default `600`

#### Scenario: Свежие сигналы приходят во время активного budget

- **WHEN** новые drift/kick signals coalesce-ятся, пока transient publish obligation ещё активно
- **THEN** его первоначальный deadline и накопленный backoff не сбрасываются и не продлеваются
- **AND** непрерывный поток сигналов не удерживает runtime в `Indexing` дольше настроенного budget

#### Scenario: Новый drift пришёл после исчерпания budget

- **WHEN** предыдущий transient publish obligation завершился `Failed` по deadline, а затем пришёл новый внешний signal
- **THEN** новый pass получает полный новый retry budget
- **AND** старый busy loop не возобновляется сам по себе

#### Scenario: Operation error завершил embedding obligation

- **WHEN** network либо Store operation возвращает собственную ошибку до исчерпания deadline
- **THEN** runtime немедленно становится `Failed` без автоматического повтора network/sidecar работы
- **AND** только последующий внешний signal создаёт новое obligation с полным budget

#### Scenario: Fenced embedding сохраняет один prepared batch

- **WHEN** workspace embedding использует network concurrency больше единицы в общей конфигурации
- **THEN** fenced workspace path не начинает следующий network batch до terminal исхода SQLite commit текущего
- **AND** final Phase C/sidecar/index bundle создаётся после всех batches и при transient retry не вызывает network
- **AND** direct/non-fenced embedding сохраняет настроенную concurrency

### Requirement: Async graph request не ждёт межпроцессную блокировку

Запросный graph snapshot MUST использовать только собственный заранее открытый descriptor и MUST не брать lease flock или открывать общий graph path на async executor. Blocking publish/adoption MUST открыть `SNAPSHOT_POOL_CAP = 4` descriptors вне fence, затем коротким fence повторно проверить ownership и ожидаемые generation/path identity и атомарно установить `Ready` вместе с pool. Background sync open MUST также выполняться вне fence после fresh preflight с короткой post-open identity проверкой и MUST NOT быть вызван request miss.

#### Scenario: Snapshot descriptor доступен

- **WHEN** async handler запрашивает snapshot при доступном собственном descriptor в pool
- **THEN** snapshot возвращается без чтения lease-файла и ожидания lock

#### Scenario: Publication не подготовила полный descriptor pool

- **WHEN** initial publish/adoption не может вне fence открыть все четыре descriptors либо post-open fence обнаруживает смену generation/path identity
- **THEN** graph не становится `Ready`, а typed failure сохраняет retry
- **AND** failed reload продолжает обслуживать прежнюю атомарную публикацию

#### Scenario: Все descriptors заняты

- **WHEN** четыре descriptors заняты конкурентными запросами и следующий async handler запрашивает snapshot, пока конкурент удерживает lease lock
- **THEN** handler немедленно получает временное отсутствие snapshot
- **AND** tokio worker не блокируется на `LOCK_WAIT`
- **AND** request miss не запускает открытие или refill общего path

#### Scenario: Вытесненный процесс видит заменённый общий файл

- **WHEN** snapshot pool пуст после замеченного вытеснения
- **THEN** общий path не открывается никаким refill
- **AND** существующая terminal ошибка переподключения сохраняется

#### Scenario: Background XML drift пришёл при занятом pool

- **WHEN** `state/sync` вычисляет referencing modules, а все request descriptors заняты
- **THEN** background path делает fresh typed preflight, открывает descriptor вне fence и коротко перепроверяет generation/path identity
- **AND** transient refusal либо open error сохраняет `rescan_required/context` debt вместо пустого набора и потери context marks

### Requirement: Build retry следует исходному отказу

Graph load failure MUST сохранять классификацию отказа в точке его возникновения. Только исходный временный отказ аренды MUST взводить обязательство повторной сборки; terminal и operation failure MUST не переопределяться последующим probe владения.

#### Scenario: Владение вернулось после временного отказа

- **WHEN** публикация получила `TransientRefusal`, а владение восстановилось до записи `Failed` status
- **THEN** `withheld_build` всё равно взводится по сохранённой причине
- **AND** следующий `ensure_loading` запускает повтор

#### Scenario: Реальная ошибка сборки при временно отрицательном probe

- **WHEN** build завершился собственной ошибкой, а последующая проверка владения была бы отрицательной
- **THEN** ошибка остаётся operation failure
- **AND** `withheld_build` не взводится как следствие повторного probe
