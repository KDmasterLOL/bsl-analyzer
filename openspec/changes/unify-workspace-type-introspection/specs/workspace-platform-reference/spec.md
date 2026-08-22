## ADDED Requirements

### Requirement: Справочные действия доступны в профиле workspace
Профиль `workspace` MUST регистрировать `syntax_help` и действия `search`: `list_platform`, `find_docs`, `search_docs`. Профиль `reference` MUST предоставлять те же справочные действия. Существующие `workspace`-действия `search_code` и `status` MUST сохранять имена, входы, семантику и текстовое зеркало; изменённый структурированный search-конверт SHALL получить опубликованную версию 4. Оба search-входа MUST быть tagged `oneOf` action-ветвей с `const action`, branch-specific required fields и `query.minLength=1` для docs search. Существующие в `v0.2.70` ветви MUST продолжать принимать неизвестные дополнительные поля; только новые action соответствующего профиля закрываются через `additionalProperties=false`. `list_platform` MUST принимать только опубликованные значения `kind` и необязательные `name`/`max_output_tokens`, а `syntax_help` — XOR форм `{name,type_name?,max_output_tokens?}` и `{reference_id,max_output_tokens?}` с сохранением legacy additional-properties behavior только первой формы.

#### Scenario: Один сеанс обслуживает исходники и справочник
- **WHEN** клиент подключён к `workspace`
- **THEN** в одном `tools/list` доступны прежний `search_code`, три справочных действия и `syntax_help`

#### Scenario: Старый поиск по исходникам не меняет смысл
- **WHEN** клиент вызывает прежний `search(action="search_code", ...)`
- **THEN** сервер ищет только в `WorkspaceCode`, сохраняет прежнюю семантику и текст и возвращает структурированную форму версии 4

#### Scenario: Обязательный запрос отсутствует
- **WHEN** клиент вызывает `find_docs` или `search_docs` без непустого `query`
- **THEN** сервер возвращает опубликованную ошибку проверки входа и не запускает поиск

### Requirement: Структурированный перечень каталога
`list_platform` SHALL перечислять типы, методы, свойства, конструкторы и глобальные функции непосредственно из `bsl-platform`. `kind` MUST принимать `type`, `method`, `property`, `constructor`, `global_function`; `name` SHALL выполнять регистронезависимый отбор по подстроке русского или английского имени. Каждая сущность MUST содержать `reference_id`, `kind`, `owner`, русское имя, `english_name: string|null` и `description: string|null`; пустые исходные значения MUST нормализоваться в `null`. `reference_id` SHALL иметь определённую design percent-encoded форму `kind:owner:name~digest`; полный identity-only BLAKE3 MUST добавляться всегда для всех пяти видов. Результат MUST сортироваться по kind, case-folded owner/name и bytewise `reference_id`.

#### Scenario: Перечень типов
- **WHEN** клиент вызывает `list_platform` с `kind="type"`
- **THEN** сервер возвращает только типы в устойчивом порядке с русскими и известными английскими именами

#### Scenario: Отбор методов
- **WHEN** клиент задаёт `kind="method"` и часть английского имени
- **THEN** сервер возвращает только соответствующие методы и их тип-владелец

#### Scenario: Стабильный идентификатор
- **WHEN** клиент получает сущность перечня
- **THEN** её `reference_id` однозначно передаётся в `syntax_help(reference_id=...)` без угадывания вида по имени

#### Scenario: Омонимы платформенных типов
- **WHEN** каталог содержит два типа `ЭлементыФормы` с разной структурной идентичностью
- **THEN** они получают разные collision-safe `reference_id`, и каждый идентификатор возвращает свою точную карточку

#### Scenario: Добавление омонима не меняет старый ID
- **WHEN** к каталогу добавляется новая сущность с теми же kind/owner/name, но иной структурной идентичностью
- **THEN** `reference_id` прежней сущности остаётся байт-в-байт тем же, а новая получает другой digest

### Requirement: Полный справочный корпус и его обновление
`find_docs` SHALL выполнять существующий лексический поиск, а `search_docs` — существующий семантический векторный поиск по корпусу `Reference`, содержащему типы, методы, свойства, конструкторы и глобальные функции. Отпечаток корпуса MUST быть полным lowercase BLAKE3 канонических устойчиво отсортированных документов вместе с `REFERENCE_DOCUMENT_SCHEMA_VERSION=1`, а не версией пакета. Старый или неполный кэш MUST быть атомарно обновлён до поиска. Одновременное открытие общего SQLite-кэша несколькими профилями MUST выполнять повторную проверку fingerprint и полную замену коллекции `platform` с её stamp в одной `BEGIN IMMEDIATE`-транзакции. Store MUST вернуть committed fingerprint; процесс с no-op MUST перезагрузить semantic index, если его loaded fingerprint отличается. Новый внешний координатор или второй файл индекса MUST NOT создаваться.

#### Scenario: Лексический поиск известного свойства
- **WHEN** клиент вызывает `find_docs` с точным именем свойства
- **THEN** сервер возвращает совпадение с `reference_id`, владельцем, именем, видом и кратким описанием

#### Scenario: Семантический поиск конструктора
- **WHEN** клиент вызывает `search_docs` с фразой, семантически близкой описанию конструктора
- **THEN** сервер возвращает ранжированное совпадение из корпуса, общего с `reference`

#### Scenario: Кэш прежнего состава
- **WHEN** кэш имеет прежний отпечаток без свойств и конструкторов
- **THEN** сервер перестраивает справочные документы и не сообщает неполный кэш готовым

#### Scenario: Два процесса впервые открывают общий кэш
- **WHEN** процессы `workspace` и `reference` одновременно открывают отсутствующий `reference-search.db`
- **THEN** операции SQLite завершаются без повреждения, а оба профиля видят один полный корпус

#### Scenario: Читатель не видит частичную перестройку
- **WHEN** один процесс заменяет устаревшую коллекцию, а второй читает или одновременно начинает ту же замену
- **THEN** читатель видит целое старое либо целое новое поколение, проигравший writer синхронизирует свой in-memory semantic index с committed fingerprint, оба процесса находят известный новый документ, а integrity/FTS-consistency остаются зелёными

### Requirement: Отдельное состояние справочного поиска workspace
`workspace` MUST лениво открывать отдельный single-flight `ReferenceSearchState` с собственными lifecycle, progress/readiness, deferred baseline и явным shutdown; профиль `reference` MUST использовать тот же компонент и сохранить eager `ensure_loading` при создании. Оно MUST NOT заменять движок или статус `WorkspaceCode`. Только `find_docs`/`search_docs` SHALL зависеть от readiness; `list_platform` и `syntax_help` MUST оставаться доступны во время загрузки или отказа корпуса. Настроенный `[search.baseline.reference]` MUST работать fail-closed без local fallback: временная загрузка возвращает `not_ready`, отсутствующий snapshot — MCP error с `data.reasonCode="baseline_unavailable"`, несовместимая document schema — `"schema_version_mismatch"`, остальные terminal errors — их `SearchError::reason_code()`; `Failed` фиксируется до перезапуска процесса. Эти состояния MUST NOT блокировать поиск исходников.

#### Scenario: Локальный справочный кэш
- **WHEN** внешняя baseline `Reference` не настроена и приходит первый `find_docs`/`search_docs` профиля `workspace`
- **THEN** сервер лениво открывает общий локальный `reference-search.db`, не создавая копию корпуса

#### Scenario: Внешний справочный корпус
- **WHEN** настроен `[search.baseline.reference]` PostgreSQL
- **THEN** `find_docs`/`search_docs` профиля `workspace` применяет тот же snapshot/readiness, что `reference`, а `search_code` продолжает использовать `WorkspaceCode`

#### Scenario: Справочник не готов
- **WHEN** справочный корпус загружается
- **THEN** `find_docs`/`search_docs` возвращают структурированный `not_ready`, а `list_platform`, `syntax_help`, `search_code` и прежний workspace-`status` остаются доступны без ожидания writer

#### Scenario: Настроенный внешний корпус недоступен
- **WHEN** настроенный PostgreSQL snapshot отсутствует, несовместим либо завершился terminal connect/auth/storage error
- **THEN** оба профиля возвращают одинаковую MCP-ошибку справочника и не подменяют внешний корпус локальным

#### Scenario: Два одновременных первых запроса
- **WHEN** два запроса одновременно инициируют неоткрытый `ReferenceSearchState`
- **THEN** запускается одна инициализация, оба видят одно lifecycle-состояние, а shutdown во время loading корректно завершает worker и baseline

### Requirement: Точная карточка платформенной сущности
`syntax_help` MUST работать в обоих профилях. Прежняя форма `name` с необязательными `type_name`/`max_output_tokens` MUST сохраняться; новая взаимоисключающая форма MUST принимать `reference_id` и необязательный `max_output_tokens`, но не `name`/`type_name`. Карточка типа версии 2 SHALL включать свойства, методы и конструкторы; точная карточка метода, свойства, варианта конструктора или глобальной функции SHALL включать вид, тип или сигнатуру, описание, контексты доступности и ограничение версии, когда они известны. Неизвестная точная сущность MUST возвращать структурированный `not_found`, а не частичное совпадение.

#### Scenario: Карточка типа
- **WHEN** клиент запрашивает известный тип
- **THEN** ответ содержит его свойства, методы и конструкторы без чтения исходников конфигурации

#### Scenario: Карточка метода по идентификатору поиска
- **WHEN** клиент передаёт полученный `reference_id` метода
- **THEN** `syntax_help` возвращает карточку именно этого метода

#### Scenario: Коллизия свойства и метода
- **WHEN** свойство и метод одного владельца имеют одинаковое имя
- **THEN** разные `reference_id` возвращают карточки соответствующего вида

#### Scenario: Перегрузки конструктора
- **WHEN** тип имеет несколько вариантов конструктора
- **THEN** каждый вариант имеет отдельный `reference_id` и точную карточку

#### Scenario: Legacy-вход карточки
- **WHEN** старый клиент передаёт `name` и необязательный `type_name`
- **THEN** прежняя семантика входа и совместимого ответа сохраняется

#### Scenario: Неизвестный тип
- **WHEN** клиент запрашивает отсутствующий тип
- **THEN** сервер возвращает версионированный `not_found` и не подменяет его частичным совпадением

### Requirement: Версионированная совместимость профилей
`tools/list` MUST публиковать дискриминированные `outputSchema` целых инструментов `search` и `syntax_help`, включая все успешные прежние действия и состояния каждого профиля. Каждый ответ `search` MUST содержать `action`; `search_code`, `find_docs`, `search_docs` и их `not_ready` MUST иметь `schema_version="4"`, `list_platform` — `"1"`, а `status` — точную форму `{action:"status", schema_version:"1", profile:"workspace"|"reference", state:"ready"|"loading"|"busy"|"failed"}`. Схема MUST различать ветви через `const action` и `const schema_version` и обязательные поля из design. Ошибки MCP SHALL проверяться отдельно от схемы успешного `structuredContent`. Одинаковые справочные запросы в `workspace` и `reference` MUST иметь одинаковое предметное содержание, схему и усечение. MCP `CONTRACT_VERSION` MUST стать `"1.13"`; старые действия и текстовые блоки MUST сохраняться.

#### Scenario: Совпадение профилей
- **WHEN** одинаковый справочный запрос с одинаковым бюджетом отправлен в оба профиля
- **THEN** предметные поля, версия схемы и граница усечения совпадают

#### Scenario: Проверка опубликованной схемы
- **WHEN** сериализуются успешный, `status`, `not_found`, `not_ready` и усечённый ответы
- **THEN** `structuredContent` каждого ответа проходит `outputSchema` из `tools/list`, а полный `CallToolResult` отдельно проходит проверку бюджета и текстового зеркала

### Requirement: Единый диагностируемый бюджет справочного ответа
`max_output_tokens` MUST применяться к совместимому тексту и `structuredContent` одного сериализованного `CallToolResult`, а не отдельно к каждой части. После устойчивой сортировки сервер SHALL возвращать укладывающийся префикс, `budget_exhausted=true`, строковый `budget_hint` и текстовое зеркало структурированных данных. Минимальный schema-valid конверт search-hit ветви MUST сохранять `action`, `schema_version`, пустые `hits`, `shown=0`, `total`, `budget_exhausted=true` и `budget_hint`; `list_platform` — те же счётчики с пустым `items`; `syntax_help` — `{schema_version:"2", status:"budget_exhausted", budget_exhausted:true, budget_hint}`. Только соответствующий обязательный минимальный конверт без предметных элементов MAY превысить слишком малый бюджет.

#### Scenario: Большой перечень усечён
- **WHEN** перечень не помещается в бюджет
- **THEN** полный ответ содержит устойчивый префикс, `budget_exhausted=true` и подсказку `kind`/`name` и не расходует бюджет независимо на text и JSON

#### Scenario: Бюджет меньше обязательного конверта
- **WHEN** бюджет не вмещает минимальный версионированный конверт
- **THEN** сервер возвращает только отмеченный минимальный конверт без сверхбюджетной сущности, и этот `structuredContent` проходит опубликованную `outputSchema`
