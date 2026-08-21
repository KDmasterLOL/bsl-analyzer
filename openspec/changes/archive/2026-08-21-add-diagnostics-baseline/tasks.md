## 1. Общая модель и формат

- [x] 1.1 Добавить в `ide` типы схемы версии 1 и записи базовой линии со строгой сериализацией; проверка: `cargo test -p ide diagnostics_baseline_schema`.
- [x] 1.2 Добавить в `ide` типы области, классифицированного результата и сводки `disabled/full/partial/error`; проверка: `cargo test -p ide diagnostics_baseline_scope`.
- [x] 1.3 Перенести нормализацию строки и BLAKE3-рецепт Code Quality в `ide`, оставив выбор корня пути вызывающей поверхности; проверка: `cargo test -p ide diagnostics_fingerprint` сохраняет золотые значения для одинакового входного пути.
- [x] 1.4 Запретить строгий отпечаток без исходного фрагмента, не меняя запасное поведение Code Quality; проверка: `cargo test -p ide diagnostics_fingerprint_requires_snippet` и `cargo test -p bsl-analyzer codequality`.
- [x] 1.5 Реализовать сопоставление новых, известных и исчезнувших записей, включая одинаковые строки; проверка: `cargo test -p ide diagnostics_baseline_classify` покрывает сдвиг строк, изменение выражения и порядковые номера.
- [x] 1.6 Реализовать детерминированное чтение и формирование JSON без абсолютных и изменчивых полей; проверка: `cargo test -p ide diagnostics_baseline_io` покрывает побайтовую повторяемость, пересчёт хэша, повреждённую схему, дубликаты и несовместимую область.
- [x] 1.7 Исключить `UnknownSuppressionCode` и `SuppressionWithoutCode` из допустимых записей; проверка: `cargo test -p ide diagnostics_baseline_protected` доказывает, что обе диагностики остаются активными.

## 2. Конфигурация проекта

- [x] 2.1 Извлечь вложенный `[diagnostics.baseline].path` в отдельную типизированную модель `project-model`, сохранив фактический путь конфигурации для разрешения TOML/JSON и не передавая `baseline` парсеру правил; проверка: `cargo test -p project-model diagnostics_baseline_path`.
- [x] 2.2 Проверять, что путь файла, корень источников и упорядоченная топология расширений `{name, path, depends_on}` нормализованы внутри канонического корня проекта, а цель не является символической ссылкой; проверка: `cargo test -p project-model diagnostics_baseline_scope` покрывает смену рабочего каталога, имя, порядок, зависимость, коллизию, внешний корень и выход пути.
- [x] 2.3 Расширить текстовый отчёт `check-config` явными ошибками отсутствующего, неподдерживаемого и повреждённого файла; проверка: `cargo test -p bsl-analyzer check_config_baseline` также покрывает одновременный `search.baseline`.

## 3. CLI и безопасное изменение файла

- [x] 3.1 Добавить в CLI и его машинный контракт группу `diagnostics baseline` с командами `create`, `check` и `update`; проверка: `cargo test -p bsl-analyzer cli_contract_diagnostics_baseline`.
- [x] 3.2 Реализовать `CoverageProof` общего запуска и отказ до записи при incremental/diff, выборе диагностики, фильтре авторов, ошибке файла, отмене или иной неполной области; проверка: `cargo test -p bsl-analyzer diagnostics_baseline_full_gate` проверяет `analyzed == total` и каждый запрещающий признак.
- [x] 3.3 Реализовать `create` только для настроенной отсутствующей цели через `tempfile`, `flush`, `sync_all` и атомарный `std::fs::hard_link` без перезаписи; проверка: `cargo test -p bsl-analyzer diagnostics_baseline_create` покрывает текстовый и машинный результат с числом и составом созданных записей, отсутствие настройки, гонку двух создателей, неподдерживаемую файловую систему, очистку временной ссылки и сохранность победившей цели.
- [x] 3.4 Реализовать `check` только для чтения и его коды завершения: успех только без новых и исчезнувших записей, ошибка при защитной диагностике или некорректном файле; проверка: `cargo test -p bsl-analyzer diagnostics_baseline_check` сравнивает байты файла до и после каждого исхода.
- [x] 3.5 Реализовать `update` только для корректной существующей цели через `tempfile`, `flush`, `sync_all` и платформенно корректный `persist` с заменой; проверка: `cargo test -p bsl-analyzer diagnostics_baseline_update` покрывает счётчики, освежение информационных полей, очистку и прежние байты после сбоя замены на текущей платформе.
- [x] 3.6 Добавить `diagnostics_baseline_create` и `diagnostics_baseline_update` в Windows-задание `.github/workflows/ci.yml`; проверка: `actionlint .github/workflows/ci.yml`, а задание выполняет оба фильтра на `windows-latest`.

## 4. Обычный анализ и отчёты

- [x] 4.1 Подключить базовую линию к `analyze` после правил и директив подавления, но до фильтров представления; проверка: `cargo test -p bsl-analyzer analyze_diagnostics_baseline` покрывает `disabled`, известную, новую, защитную, отключённую правилом и штатно подавленную диагностику.
- [x] 4.2 Добавить сводку базовой линии в консольный отчёт; проверка: `cargo test -p bsl-analyzer console_baseline_summary` покрывает `disabled/full/partial`, а ошибка загрузки прерывает формирование отчёта.
- [x] 4.3 Добавить обратно совместимое поле сводки в корневой объект JSON; проверка: `cargo test -p bsl-analyzer json_baseline_summary` покрывает `disabled/full/partial`, а ошибка загрузки прерывает формирование отчёта.
- [x] 4.4 Добавить сводку в `run.properties` SARIF, не выставляя `baselineState` для частичного результата; проверка: `cargo test -p bsl-analyzer sarif_baseline_summary` покрывает `disabled/full/partial`, а ошибка загрузки прерывает формирование отчёта.
- [x] 4.5 Добавить сводку в свойства набора JUnit без изменения счётчиков тестов и ошибок; проверка: `cargo test -p bsl-analyzer junit_baseline_summary` покрывает `disabled/full/partial`, а ошибка загрузки прерывает формирование отчёта.
- [x] 4.6 Добавить необязательную сводку в событие `done` JSONL и учитывать ошибки файлов в полноте; проверка: `cargo test -p bsl-analyzer jsonl_baseline_summary` покрывает `disabled/full/partial`, сохраняет прежние события, проверяет `failed_files` и прерывание при ошибке загрузки.
- [x] 4.7 Перевести Code Quality на общий рецепт отпечатка, сохранив выбор пути относительно `--workspace-dir`, и оставить артефакт корневым массивом активных замечаний без фиктивной сводки; проверка: `cargo test -p bsl-analyzer codequality` сохраняет золотые отпечатки штатного пути и схему GitLab.
- [x] 4.8 Маркировать результаты `analysis.diff_base`, CLI-фильтров и `ignored_authors` как частичные и не объявлять записи вне доказанной области исчезнувшими; проверка: `cargo test -p bsl-analyzer analyze_baseline_partial_scope`.
- [x] 4.9 Добавить сквозной CLI-тест проекта с основной конфигурацией и расширением: настроить путь, создать файл, проверить известную, новую и исчезнувшую диагностику, выполнить явное обновление и проверить ошибки отсутствующего/повреждённого файла; проверка: `cargo test -p bsl-analyzer --test diagnostics_baseline_cli` также покрывает `workspace_dir != source_dir` и прежний код завершения `analyze` при новой диагностике.

## 5. MCP

- [x] 5.1 Загружать один снимок базовой линии на состояние проекта MCP и применять классификатор `ide` к запросам файла и рабочей области до фильтров ответа; проверка: `cargo test -p mcp-server diagnostics_baseline_snapshot`.
- [x] 5.2 Расширить MCP-ответы, `outputSchema` и опубликованную версию схемы успешной и ошибочной ветвями `oneOf`, сохранив существующие поля, сводку вне урезаемых коллекций и зеркальность текста; проверка: `cargo test -p mcp-server diagnostics_baseline_response` проверяет `disabled`, сериализованную схему, обязательные поля и минимальный бюджет.
- [x] 5.3 Добавить путь базовой линии как отдельный наблюдаемый файл и отдельную эпоху: изменение, атомарная замена и удаление перечитывают снимок без пересоздания Salsa, но меняют MCP `result_id`; проверка: `cargo test -p mcp-server diagnostics_baseline_reload` проверяет прежнее поколение резидентной базы, новый результат и новый идентификатор.
- [x] 5.4 Добавить интеграционный MCP-тест паритета с CLI, актуального ограниченного/отменённого запроса, устаревшего снимка с `resolved = 0`, непрочитанного файла и восстановления после исправления базовой линии; проверка: `cargo test -p mcp-server --test diagnostics_baseline`.

## 6. LSP

- [x] 6.1 Применить общий снимок к публикации LSP: публиковать новые и защитные диагностики, не публиковать известные; проверка: `cargo test -p bsl-analyzer lsp_diagnostics_baseline_publish`.
- [x] 6.2 Добавить путь базовой линии в LSP как отдельный наблюдаемый файл: изменение, атомарная замена и удаление сбрасывают снимок и повторно публикуют открытые документы и рабочую партию без пересоздания Salsa; проверка: `cargo test -p bsl-analyzer lsp_diagnostics_baseline_reload`.
- [x] 6.3 При ошибке файла публиковать все текущие диагностики без фильтрации и уведомлять один раз на файловый отпечаток; проверка: `cargo test -p bsl-analyzer lsp_diagnostics_baseline_error` покрывает повтор и восстановление.
- [x] 6.4 Добавить LSP-тест паритета отпечатков с CLI/MCP при одинаковом нормализованном пути; проверка: `cargo test -p bsl-analyzer --test diagnostics_baseline_lsp parity`.
- [x] 6.5 Доказать, что анализ одного открытого документа не объявляет глобальные записи исчезнувшими; проверка: `cargo test -p bsl-analyzer --test diagnostics_baseline_lsp partial_document`.

## 7. Документация и итоговые шлюзы

- [x] 7.1 Документировать формат, конфигурацию, команды, CI-сценарий, подавления и отличие от `analysis.diff_base` и `search.baseline` в `docs/configuration/DIAGNOSTICS.md`, `docs/configuration/PROJECT_CONFIGURATION.md`, `docs/CI_REPORTERS.md` и `docs/mcp/TOOLS_AND_EXTENSION.md`; проверка: блоки конфигурации и команд выполняются тестом `cargo test -p bsl-analyzer --test diagnostics_baseline_cli documented_usage`.
- [x] 7.2 Добавить пример миграции: раздел конфигурации → полный `create` → ревью файла → `check` в CI → явный `update`; проверка: `cargo test -p bsl-analyzer --test diagnostics_baseline_cli documented_migration`.
- [x] 7.3 Прогнать целевые проверки изменённых крейтов: `cargo fmt --all -- --check`, `cargo clippy -p ide -p project-model -p bsl-analyzer -p mcp-server --all-targets --all-features -- -D warnings` и `cargo test -p ide -p project-model -p bsl-analyzer -p mcp-server`.
- [x] 7.4 Прогнать `cargo test --all --no-fail-fast` и `openspec validate add-diagnostics-baseline --strict --no-interactive`.
- [x] 7.5 Сверить каждый сценарий `specs/diagnostics-baseline/spec.md` с конкретным автоматическим тестом; проверка: ни один сценарий не остаётся покрытым только ручной проверкой или этой итоговой задачей.

## 8. Независимый обзор и финальная проверка

- [x] 8.1 Провести независимые обзоры соответствия OpenSpec и сложности реализации, устранить все подтверждённые замечания.
- [x] 8.2 Повторить форматирование, Clippy, целевые тесты, полный набор тестов, строгую проверку OpenSpec и `git diff --check` после исправлений.
