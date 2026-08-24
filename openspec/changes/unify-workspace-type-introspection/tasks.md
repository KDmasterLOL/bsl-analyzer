## 1. Машинный контракт и справочник платформы

- [x] 1.1 Поднять `CONTRACT_VERSION` с `1.12` до `1.13`, ввести tagged `WorkspaceSearchParams`/`ReferenceSearchParams` с общей внутренней командой и schema-level `oneOf` для syntax-help форм `{reference_id,max_output_tokens?}`/`{name,type_name?,max_output_tokens?}`; контрактным тестом `tools/list` проверить обязательный непустой `query`, enum `kind`, отклонение cross-profile action, закрытость только новых action/reference-id ветвей и прежнее принятие добавочных полей ветвями `v0.2.70` вместе с сохранением входов/семантики `search_code`/`status`.
- [x] 1.2 Ввести общий DTO `reference_id/kind/owner/name/english_name|null/description|null` и прямой `list_platform`; тестами пяти kinds проверить RU/EN case-folded substring, RFC3986 encoding для `:`, `%` и Unicode, точный sort tuple, method/property и constructor collisions, всегда добавляемый полный identity-only digest, разные round-trip ID двух типов `ЭлементыФормы` и неизменность старого ID после добавления омонима.
- [x] 1.3 Вынести в библиотеку `mcp-server` один детерминированный построитель документов `Reference`, переиспользовать его local runtime и baseline publisher и включить свойства/конструкторы; тестами проверить полный одинаковый состав и порядок обоих потребителей без новой зависимости.
- [x] 1.4 Ввести `REFERENCE_DOCUMENT_SCHEMA_VERSION=1`, fingerprint как BLAKE3 канонических документов и атомарный Store API `replace_reference_collection_if_stale`; тестами перестановки/изменения данных и старого неполного кэша проверить одинаковый fingerprint, rebuild и невидимость частичного поколения.
- [x] 1.5 Выделить из `SharedState::reference` общий single-flight `ReferenceSearchState` с `Uninitialized|Loading|Ready|Failed`, собственными engine/progress/deferred baseline и shutdown, сохранив eager `ensure_loading` reference-профиля; тестами eager status, двух конкурентных ensure и shutdown during Loading проверить один worker и отсутствие lifecycle leak.
- [x] 1.6 Подключить `ReferenceSearchState` к `workspace`, не меняя `WorkspaceCode`, и выровнять local SQLite/`[search.baseline.reference]` с профилем `reference`; barrier-тестами проверить быстрый `not_ready` только для docs actions, независимость `list_platform`/`syntax_help`/`search_code`/workspace-status и fail-closed MCP error без local fallback с `data.reasonCode=baseline_unavailable|schema_version_mismatch|<SearchError::reason_code()>` для соответствующих terminal external failures.
- [x] 1.7 Сделать конкурентную публикацию общего `reference-search.db` идемпотентной одной `BEGIN IMMEDIATE`-транзакцией без нового координатора; subprocess-тестом absent и stale DB со стартовым barrier проверить одинаковые committed fingerprint/count, no-op проигравшего writer, перезагрузку его semantic index при отличающемся loaded fingerprint, нахождение нового документа обоими процессами, `PRAGMA integrity_check`, FTS consistency и отсутствие частичного корпуса.
- [x] 1.8 Добавить точное разрешение `syntax_help(reference_id)`, карточки свойства и варианта конструктора, расширить `SyntaxHelpResponse::Type` свойствами и поднять схему до версии 2; тестами пяти видов, legacy-входа, обеих форм с `max_output_tokens`, омонимов типов, method/property, constructor overloads и `not_found` проверить ответы обоих профилей и соответствие success/not_found/truncated `structuredContent` опубликованной `outputSchema` v2.
- [x] 1.9 Обогатить `find_docs`/`search_docs` полями общего DTO, поднять обе ветви до версии 4 и сохранить соответственно lexical/semantic semantics; тестами проверить свойства, конструкторы, несовместимую запись кэша и предметный parity профилей.
- [x] 1.10 Опубликовать дискриминированную `search`-`outputSchema`: `search_code`/`find_docs`/`search_docs` и их `not_ready` v4, `list_platform` v1 и `status` точной формы `{action:"status",schema_version:"1",profile,state}`; проверить success/status всех `ready|loading|busy|failed` и обоих профилей, not_ready/truncation против `const`-ветвей, а invalid input и terminal failures отдельно как MCP errors.
- [x] 1.11 Применить один бюджет к полному text+`structuredContent` для `list_platform`, `find_docs`, `search_docs` и `syntax_help`; тестами большой карточки/перечня и минимального бюджета проверить строковый `budget_hint`, schema-valid минимальные конверты с пустыми `hits`/`items` либо syntax `status="budget_exhausted"`, отсутствие сверхбюджетной сущности и одинаковый prefix/границу усечения двух профилей.

## 2. Семантические источники прикладных членов

- [x] 2.1 Добавить в `ide-db`/`hir` запрос effective metadata members, который обходит уже загруженные root в топологическом порядке и сохраняет источник добавления/замены; тестами покрыть базу, уникальное добавление, одноимённую замену, unread root и приоритет двух расширений без повторного чтения XML.
- [x] 2.2 Добавить topology-aware query экспортных методов/переменных object, manager и managed-form modules поверх существующих effective/weaving queries; матрицей base, extension-only, `&ИзменениеИКонтроль`, `&Вместо`, `&Перед`, `&После`, unread root и двух extensions проверить внешние target names, отдельные source candidates и отсутствие full index scan на lookup.
- [x] 2.3 Подключить экспортные переменные object/manager module из того же query к `hir-ty::field_lookup`; регрессиями `BA-010` проверить обе грани, extension composition и отсутствие ложного `UnresolvedField`.
- [x] 2.4 Расширить разбор `symbol_info` четырьмя именованными гранями без нового MCP-инструмента; тестами всех граней проверить версионированный `not_found` и для неизвестной грани, и для неизвестного объекта, а malformed symbol — отдельно как MCP invalid params.
- [x] 2.5 Расширить `SymbolMember` совместимым строковым типом либо структурированной сигнатурой, `type_variants` только для typed member, `origin`, `source_extension` и `availability {contexts,context_status,reason?}`; тестами проверить одноимённых source candidates, module-variable как `property`, nullable/unknown availability и отсутствие неприменимого type field у callable.
  - Этап цикла: реализация
  - Состояние шага: выполнен; DTO, все существующие producers, serializer и transport покрывают origin/source/availability и взаимоисключающие type/signature ветви
  - Проверка: `cargo test -p ide --test symbol_info`; `cargo test -p mcp-server tools::symbol_info::tests`; `cargo test -p mcp-server --test symbol_info`; strict clippy `ide,mcp-server`
  - Файлы шага: `crates/ide/src/symbol_info.rs`, `crates/ide/src/symbol_info/form/card.rs`, `crates/ide/tests/symbol_info.rs`, `crates/mcp-server/src/tools/symbol_info.rs`, `crates/mcp-server/tests/symbol_info.rs`, `openspec/changes/unify-workspace-type-introspection/tasks.md`
- [x] 2.6 Перевести полный и точный поиск прикладного члена на один сборщик после устойчивой сортировки, исключив first-wins; модульными тестами synthetic metadata/module/platform candidates проверить полный prefix, exact candidates и `not_found` до подключения конкретных граней.
  - Этап цикла: реализация
  - Состояние шага: выполнен; полный и exact applied lookup используют один source-preserving collector без dedup/first-wins
  - Проверка: synthetic metadata/module/platform matrix; real exact metadata member/miss; strict clippy `ide --all-targets --all-features`
- [x] 2.7 Подключить к сборщику object cards справочника и обработки из effective metadata, object-module exports и platform receiver; тестами содержательной `ОбработкаОбъект`, реквизита/стандартного свойства/метода/переменной/platform member и missing/unread module проверить происхождение и сохранение metadata/platform.
  - Этап цикла: реализация
  - Состояние шага: выполнен; object cards объединяют effective metadata, effective exports и hir-owned platform receiver surface
  - Проверка: IDE symbol_info 28/28; exact export; missing/unread module; MCP transport 2/2; strict clippy `hir-ty,hir,ide`
- [x] 2.8 Подключить ссылочную карточку только из читаемых реквизитов и платформы, а менеджерскую — из свойств, manager-module candidates и платформы; тестами проверить отсутствие object-module примеси, экспортную переменную менеджера, exact candidates и одинаковый module result с `field_lookup`.
  - Этап цикла: реализация
  - Состояние шага: выполнен; reference и manager используют раздельные receiver/module surfaces без object-module примеси
  - Проверка: IDE symbol_info 29/29; exact manager variable; parity через `hir::Type::has_field`; hir type_facade 33/33; MCP transport 2/2; strict clippy `hir-ty,hir,ide`

## 3. Структурированные варианты статических и живых типов

- [x] 3.1 Добавить к статическому typed member `type_variants` с `presentation`, nullable `technical_name`, `resolution` и `reason`, сохранив строковый тип; тестами проверить составной тип, одинаковые presentations при разных technical names и отсутствие type_variants у signature-only callable.
  - Этап цикла: реализация
  - Состояние шага: выполнен; static AttributeType variants сохраняют machine identity, unresolved остаётся явным, callable branch не получает type fields
  - Проверка: composite static variants; equal presentation/different technical names; IDE 29/29; MCP serializer 9/9 и transport 2/2; strict clippy `ide,mcp-server`
- [x] 3.2 Расширить `СтруктураМетаданныхPOST` полем `typeVariants: [{technicalName:string|null,presentation}]`: applied types получать через `Метаданные.НайтиПоТипу` и таблицу metadata-коллекций, primitive/platform — через таблицу фактических `ТипЗнч`, unknown technical name — `null`, никогда не из `Строка(Тип)`; raw contract fixture/test проверить primitive, platform, applied, composite, unsupported и RU/EN session locale при сохранённом `type`.
  - Этап цикла: реализация
  - Состояние шага: выполнен; producer сохраняет `type`, выдаёт locale-independent machine variants и оставляет unsupported как `null`
  - Проверка: BSL parser; raw RU/EN fixture для primitive/platform/applied/composite/unsupported; `extension_live_metadata_contract` 3/3; strict clippy `bsl-analyzer`
- [x] 3.3 Типизировать известную часть ответа в `onec-client`, принимать только известные producer IDs, неизвестный/null переводить в `unresolved`, а неизвестные поля игнорировать; фикстурами проверить primitive, platform, applied, composite и future unknown identifier.
  - Этап цикла: реализация
  - Состояние шага: выполнен; typed reader принимает whitelist producer IDs, future/null остаются unresolved, unknown JSON fields игнорируются
  - Проверка: shared raw fixture; `onec-client` 4/4; strict clippy; producer contract 3/3; OpenSpec strict valid
- [x] 3.4 Для старого ответа без машинных вариантов возвращать `technical_name=null`, `resolution="unresolved"` без сопоставления с деревом workspace; тестами `BA-007` проверить одинаковые представления, другую информационную базу и явную причину неразрешённости.
  - Этап цикла: реализация
  - Состояние шага: выполнен; legacy `type` становится presentation-only variant с `legacy_type_only`, без workspace lookup
  - Проверка: paired same-presentation/different-infobase fixture; `onec-client` 5/5; strict clippy
- [x] 3.5 Добавить `schema_version="1"` к существующему структурированному варианту `metadata object`, сохранив конверт и поля; тестами сериализации проверить `source`, `type_variants`, unresolved reasons, unknown fields и отдельно MCP-ошибку live-сервиса без tool-wide `metadata outputSchema`.
  - Этап цикла: реализация
  - Состояние шага: выполнен; live object envelope v1 нормализует variants, а service failure остаётся MCP error
  - Проверка: MCP serialization 2/2; workspace HTTP metadata contract 1/1; strict clippy `mcp-server`
- [x] 3.6 Контрактными фикстурами проверить все четыре комбинации: новый бинарник × старый ответ, новый × новый, frozen legacy consumer release `v0.2.70` × старый и `v0.2.70` × новый; сохранить `type`, записать версии fixture и не считать unknown fields ошибкой.
  - Этап цикла: реализация
  - Состояние шага: выполнен; versioned fixtures cover new/legacy producer with new/frozen consumer and preserve `type`
  - Проверка: frozen `v0.2.70` tag commit/source SHA recorded; matrix test; `onec-client` 6/6; producer contract 3/3; strict clippy; OpenSpec strict valid

## 4. Вход symbol_info, доступность и общий бюджет

- [x] 4.1 Разрешить сочетание `symbol` с парой `path/line` и необязательными `root_id/column`, сохранив старые позиционные запросы `path/line` и `root_id/path/line` без `symbol`; тестами входной схемы проверить все формы и отклонение неполной позиции.
  - Этап цикла: реализация
  - Состояние шага: выполнен; symbol-only и complete-position формы опубликованы через `oneOf`, incomplete position отклоняется до resident loading
  - Проверка: input schema unit; real MCP transport accepted 3 forms/rejected 3 incomplete forms; strict clippy; OpenSpec strict valid
- [x] 4.2 Использовать существующий контекст модуля для `availability.context_status`; тестами client/server, generic thick mapping, недостатка версии каталога и запроса без позиции проверить `available|unavailable|unknown|not_evaluated`, nullable contexts и отсутствие неявной фильтрации.
  - Этап цикла: реализация
  - Состояние шага: выполнен; shared execution environment annotates declarative platform contexts and never filters members
  - Проверка: client/server, generic thick, catalog unknown, no-position, nullable contexts units; same-facet transport count; hir 33/33; IDE 29/29; MCP 11/11; strict clippy; OpenSpec strict valid
- [x] 4.3 Добавить необязательные `member_kind` и точный регистронезависимый `member_name`, не меняя значения `include=definition|type|doc`; регрессионным тестом повторить старый include-запрос и проверить кандидатов при новых фильтрах.
  - Этап цикла: реализация
  - Состояние шага: выполнен; enum kind and exact Unicode case-insensitive name filter only the member list
  - Проверка: schema enum; filter unit; real transport kind/name filters and legacy include; strict clippy; OpenSpec strict valid
- [x] 4.4 Опубликовать дискриминированную `symbol_info`-`outputSchema` v1 для `status=ok|not_found|ambiguous` и усечения; проверить required availability/type-or-signature branches против `tools/list`, а invalid input и internal errors — отдельно как MCP errors.
  - Этап цикла: реализация
  - Состояние шага: выполнен; tools/list publishes const-tagged v1 branches and runtime success/error paths are separated
  - Проверка: schema-shape DTO tests for ok/not_found/ambiguous/truncated; real tools/list transport; invalid/internal MCP errors; mcp-server symbol/contract tests; strict clippy; OpenSpec strict valid
- [x] 4.5 Заменить раздельное бюджетирование doc/snippet/members/usages единым расчётом полного `CallToolResult` после устойчивой сортировки; тестами большого и слишком малого бюджета проверить text+structuredContent, schema-valid минимальный `{schema_version:"1",status:"ok",symbol,truncated:true,budget_hint:string}` без необязательных секций или сверхбюджетного элемента и повторяемый префикс.
  - Этап цикла: реализация
  - Состояние шага: выполнен; one serialized CallToolResult ceiling trims deterministic optional sections/member tail to the v1 minimum envelope
  - Проверка: long doc/snippet plus 200-member unit checks byte ceiling, JSON text mirror, stable sorted prefix and tiny schema-valid envelope; transport 2/2; strict clippy; OpenSpec strict valid

## 5. Карточки форм

- [x] 5.1 Объединить реквизиты, элементы, экспортные методы и платформенные члены управляемой формы одним сборщиком для полного и точного запроса, сохранив legacy fallback точного поиска к локальному неэкспортному методу; тестами коллизии имён и закрытого helper проверить кандидатов и обратную совместимость.
  - Этап цикла: реализация
  - Состояние шага: выполнен; managed full/exact lookup shares metadata, effective module and platform candidates while unique legacy cards and local fallback remain
  - Проверка: collision/export/platform/private-helper integration; all IDE symbol_info 30/30; collector 5/5; strict clippy; OpenSpec strict valid
- [x] 5.2 Переиспользовать `form_element_type` и результирующие метаданные управляемой формы для `DataPath`, не читая XML повторно; тестом `Объект.ИНН` проверить выведенный тип элемента.
  - Этап цикла: реализация
  - Состояние шага: выполнен; существующий form_element_type сохраняет выведенный binding, а IDE-карточка рендерит его без отдельного XML-пути
  - Проверка: in-memory Объект.ИНН -> Строка; IDE form item -> Строка; strict clippy `hir-ty,ide`; OpenSpec strict valid
- [x] 5.3 Выбирать платформенный базовый тип управляемой формы с учётом типа объекта и расширений; тестами разных типов объекта проверить отсутствие чужих членов и availability.
  - Этап цикла: реализация
  - Состояние шага: выполнен; base ClientApplicationForm объединяется с extension главного реквизита, а общий PlatformObject field enumerator сохраняет свойства и availability
  - Проверка: mapping document/report/dynamic-list; platform property unit; document/data-processor/report matrix; IDE symbol_info 31/31; strict clippy `hir-ty,hir,ide`; OpenSpec strict valid
- [x] 5.4a Проверить полную и отфильтрованную карточки управляемой формы через тот же effective/weaving module query из 2.2 с отдельными source candidates.
  - Этап цикла: реализация
  - Состояние шага: выполнен; выбор базовой формы следует topology rank, а full/exact cards сохраняют extension-only, replacement и wrapper candidates с source_extension
  - Проверка: managed form effective exports integration; form-focused IDE tests 15/15
- [x] 5.4b Проверить устойчивое бюджетное усечение большой карточки управляемой формы и одинаковый префикс повторных ответов.
  - Этап цикла: реализация
  - Состояние шага: выполнен; реальная managed-form карточка дважды даёт одинаковый непустой member prefix под общим transport budget
  - Проверка: MCP symbol_info transport 1/1
- [x] 5.4c Отдельным регрессионным тестом подтвердить неизменность полной карточки и точного разрешения обычной формы без платформенного объединения.
  - Этап цикла: реализация
  - Состояние шага: выполнен; ordinary form сохраняет metadata-only full card и legacy exact lookup локального метода
  - Проверка: form-focused IDE tests 15/15

## 6. Документация, совместимость и приёмка

- [x] 6.1 Обновить `docs/mcp/CONTRACT.md`, `TOOLS_AND_EXTENSION.md` и примеры: действия/профильные входы, версии 1.13/v4/v2/v1, lifecycle `Reference`, fail-closed external baseline, type variants/availability, минимальный budget envelope и необязательность профиля `reference`.
  - Этап цикла: документация
  - Состояние шага: выполнен; контракт, profile matrix, lifecycle, schema branches, member DTO и budget envelope синхронизированы с кодом
  - Проверка: targeted rg; git diff --check
- [x] 6.2 Документировать rollout «drain старых writers → новый бинарник → новое расширение» и rollback «остановить новые writers → единолично запустить v0.2.70 и дождаться legacy-перестройки derived cache до публикации engine»; проверить инструкции по четырём комбинациям совместимости без секретов connection и явно запретить concurrent mixed-version writers.
  - Этап цикла: документация
  - Состояние шага: выполнен; последовательные rollout/rollback, четыре комбинации бинарник/расширение, cache gate и evidence hygiene задокументированы
  - Проверка: targeted rg; git diff --check
- [x] 6.3 Собрать release-бинарник change и получить frozen legacy consumer/binary `v0.2.70`; записать версии и SHA-256 обоих артефактов и расширения, выполнить offline fixtures и последовательный rollback-smoke открытия/legacy-перестройки нового derived cache с проверкой legacy fingerprint/count, отсутствия new-only документов, `PRAGMA integrity_check` и FTS consistency, подготовить обезличенный шаблон доказательств без установки на стенд.
  - Этап цикла: реализация
  - Состояние шага: выполнен; current/legacy release artifacts и extension tree захешированы, fixture matrix и sequential cache rollback smoke прошли
  - Проверка: release builds; onec-client 6/6; producer 3/3; process-safe 1/1; legacy 9,517 docs, marker absent, SQLite/FTS integrity green
- [x] 6.4 Только после отдельного явного согласования установить проверяемое расширение на согласованную live-базу; новым бинарником подтвердить raw `typeVariants`, normalized `metadata object` и BA-007, а legacy consumer `v0.2.70` — сохранённое поле `type`; сохранить обезличенные versions/SHA/evidence.
  - Этап цикла: финальная проверка
  - Состояние шага: выполнен на отдельно согласованной локальной базе; raw и normalized ответы содержат 6/6 машинных вариантов, legacy consumer сохраняет 6/6 полей `type`, смешанных writers не было
  - Проверка: HTTP producer 1.1.0; fail-closed raw contract и negative self-check; new/legacy MCP consumers; обезличенные SHA и counts записаны в evidence
  - Файлы шага: `extension/`, `openspec/changes/unify-workspace-type-introspection/live-metadata-contract.jq`, `openspec/changes/unify-workspace-type-introspection/evidence-template.md`, `openspec/changes/unify-workspace-type-introspection/tasks.md`
- [x] 6.5 Тем же новым бинарником на согласованном стенде диагностировать реальное обращение к экспортной переменной и подтвердить отсутствие `UnresolvedField`; сохранить обезличенное доказательство `BA-010` отдельно от карточки `symbol_info`.

## 7. Финальные шлюзы

- [x] 7.1 Сопоставить каждый сценарий обеих delta specs с автоматическим тестом; любое исключение требует отдельного явного согласования, а не отметки задачи выполненной.
  - Этап цикла: финальная проверка
  - Состояние шага: выполнен; 64/64 сценария имеют исполняемое доказательство, включая прямой semantic constructor test
  - Проверка: автоматическая сверка заголовков Scenario 64/64; 104 test-reference, 0 отсутствующих функций; focused constructor test 1/1; `git diff --check`
- [x] 7.2 Выполнить `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, целевые тесты `onec-client`, `bsl-platform`, `hir-def`, `hir-ty`, `ide-db`, `ide`, `mcp-server` и `extension_live_metadata_contract`.
  - Этап цикла: финальная проверка
  - Состояние шага: выполнен; formatter, full-workspace Clippy, все целевые crate/integration suites и extension contract зелёные
  - Проверка: `cargo fmt --all -- --check`; `cargo clippy --all-targets --all-features -- -D warnings`; целевые семь packages; `extension_live_metadata_contract` 3/3
- [x] 7.3 Выполнить `cargo test --all --no-fail-fast` и `./scripts/check-invariants.sh`; зафиксировать версии/SHA проверенных бинарников и расширения для воспроизводимого отката.
  - Этап цикла: финальная проверка
  - Состояние шага: выполнен; полный regression и invariant gate зелёные, release artifact и extension tree пересобраны и захешированы
  - Проверка: `cargo test --all --no-fail-fast`; `./scripts/check-invariants.sh`; release build; SHA-256 current/legacy binary и 8-file extension tree

## Разрывы ревью

- [x] 8.1 Включить успешный transient `status="loading"` `symbol_info` в его tool-wide `outputSchema` v1 и выдавать тот же `schema_version="1"` в runtime-конверте; реальным transport-тестом проверить loading-ответ против опубликованной схемы.
  - Этап цикла: исправление
  - Состояние шага: выполнен; единый resident loading-конверт адаптирован в четвёртую versioned ветвь `symbol_info` без изменения sibling tools
  - Проверка: schema DTO red/green; mcp-server symbol_info 14/14; real transport 1/1; public surface fingerprint; fmt; OpenSpec strict; diff check
- [x] 8.2 Провести позиционный `file_id` в effective metadata/module member queries прикладных граней и управляемых форм, оставив именованный запрос без позиции в designer-wide across-roots; topology-тестом исключить невидимый sibling extension и сохранить base/dependency/self candidates.
  - Этап цикла: исправление
  - Состояние шага: выполнен; named target, metadata overlay и object/manager/form exports используют file-scoped visibility только при переданной позиции
  - Проверка: topology TDD red/green 2/2; IDE symbol_info 35/35; strict clippy ide; fmt; OpenSpec strict; diff check
- [x] 8.3 Синхронизировать description contract tagged-union входов `search`/`syntax_help` без `<no doc>` и принять только проверенный новый public surface snapshot, включая изменённый fingerprint `symbol_info`.
- [x] 8.4 Ограничить позиционное разрешение самой управляемой формы видимыми для `file_id` configuration roots, сохранив designer-wide именованный запрос без позиции; topology-тестом исключить выбор формы из невидимого sibling extension.
- [x] 8.5 Сделать lifecycle reference-профиля fail-closed: не строить local corpus при всё ещё pending configured external baseline или ошибке чтения project config и будить ожидающий worker при shutdown; тестами короткого pending wait, shutdown и invalid config проверить `Loading`/`Failed` без local engine.
- [x] 8.6 Сделать описание общего `search.query` корректным для workspace code и platform-reference действий и обновить только проверенные description snapshots; контрактным тестом сохранить отсутствие `<no doc>`.
- [x] 8.7 Использовать канонический machine identity `ОтчетОбъект.<Имя>` в producer и consumer whitelist вместо locale spelling `ОтчётОбъект`; fixture/contract-тестом проверить report applied type без изменения presentation.
- [x] 8.8 Добавить raw live fixture с двумя одинаковыми `presentation`, но разными известными `technicalName`, и проверить, что новый consumer сохраняет оба source-варианта без схлопывания.
- [x] 8.9 Проверить `list_platform` на едином бюджете полного `CallToolResult`: большой перечень даёт устойчивый непустой префикс и hint, минимальный бюджет — schema-valid пустой items-конверт без предметной сущности.
- [x] 8.10 Реальным MCP transport-тестом отправить одинаковый усечённый `list_platform` в `workspace` и `reference` и проверить byte-equivalent text, structured content, schema version и границу усечения.
- [x] 8.11 На реальном platform catalog проверить пары method/property с одинаковыми owner/name и варианты конструктора одного owner: collision-safe разные ID должны открывать точные карточки соответствующего kind/variant.
- [x] 8.12 Прямым `find_docs`-тестом проиндексировать реальный property reference document и проверить `reference_id`, owner, name, kind и description в лексическом hit.
- [x] 8.13 Усилить catalog contract test: RU/EN case-folded фильтр для type/method, независимый точный sort tuple и `non_empty("") -> null` для optional DTO fields.
- [x] 8.14 Заменить ASCII-only сравнение имён при наложении metadata members на Unicode case-folding; узким тестом кириллического имени в разном регистре проверить единственного topology winner.
  - Этап цикла: исправление
  - Состояние шага: выполнен; общая точка композиции использует тот же Unicode fold, что и остальное BSL-разрешение
  - Проверка: `effective_metadata_members_keep_topological_winner_and_source` 1/1; `cargo fmt --all -- --check`
