## 1. Контракт проекта и ownership plan

- [x] 1.1 Расширить `ProjectDiagnosticsConfig` взаимоисключающими `path`/`directory` и
  optional groups, сохранив прежний TOML/JSON `path`; проверка:
  `cargo test -p project-model partitioned_baseline_config_contract`.
- [x] 1.2 Построить из `ExtensionTopology` детерминированные ids/identities и таблицу
  root owner, отклоняя неизвестные members, case-fold collisions, повторное членство и
  пересекающиеся roots; проверка:
  `cargo test -p project-model diagnostics_partition_identity_and_ownership`.
- [x] 1.3 Реализовать безопасное разрешение `directory`, manifest/object/temporary
  и migration paths относительно origin config через закреплённые directory handles,
  без links/reparse points/check-then-swap, с hashed portable partition keys; проверка:
  `cargo test -p project-model partitioned_baseline_path_security`.

## 2. Форматы, компактный set snapshot и миграция

- [x] 2.1 Добавить manifest schema v1 с единым portable scope fingerprint и partition
  schema v2 с точной identity и детерминированной сериализацией; оставить одиночный
  schema v1 parser неизменным; проверка: `cargo test -p ide partitioned_baseline_schema_v2`.
- [x] 2.2 Реализовать потоковый loader всех partition в один компактный индекс с
  binary fingerprints, interning, cross-partition validation и fail-closed error model;
  проверка: `cargo test -p ide partitioned_baseline_set_loader`.
- [x] 2.3 Расширить общий classifier root-owner routing, общей/per-partition summary и
  per-partition `Full`/`Partial`/`resolved`, не меняя fingerprint/protected diagnostics;
  проверка: `cargo test -p ide partitioned_baseline_classification_and_summary`.
- [x] 2.4 Реализовать чистое преобразование проверенного schema v1 snapshot в полный
  schema v2 set без принятия текущих new diagnostics и изменения входных bytes;
  проверка:
  `cargo test -p ide partitioned_baseline_v1_migration`.

## 3. CLI и многофайловая транзакция

- [x] 3.1 Добавить `--partition` к `create|check|update`, а `--from-v1`
  только к `create`, сохранив прежние команды и машинные поля legacy-mode;
  проверка: `cargo test -p bsl-analyzer cli_contract_partitioned_baseline`.
- [x] 3.2 Реализовать immutable content-addressed objects, межпроцессную блокировку,
  проверку generation перед atomically published manifest и fault-safe cleanup;
  проверка:
  `cargo test -p bsl-analyzer partitioned_baseline_transaction` плюс Windows CI.
- [x] 3.3 Реализовать all/selected semantics: первый набор только полный, read-only
  selected check, selected create только для byte-identical repair, selected update с
  reuse неизменившихся paths/hashes и полный update при изменении topology; проверка:
  `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli operations`.
- [x] 3.4 Добавить end-to-end безопасную миграцию/rollback v1 и проверку
  добавления/удаления/переименования extension/group без heuristic carry-over; проверка:
  `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli`.

## 4. Обычный analyze и репортёры

- [x] 4.1 Маршрутизировать post-suppression diagnostics из одного полного semantic run
  через общий set classifier до presentation filters; проверка:
  `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_semantics`.
- [x] 4.2 Добавить общую/per-partition summary в console, JSON, JSONL, SARIF и JUnit,
  в зафиксированные существующие поля, сохранив прежние поля/counts и Partial semantics;
  проверка:
  `cargo test -p bsl-analyzer partitioned_baseline_reporters`.
- [x] 4.3 Сохранить GitLab Code Quality root array и золотые fingerprints без
  служебных элементов; проверка: `cargo test -p bsl-analyzer codequality`.

## 5. MCP

- [x] 5.1 Заменить один diagnostics baseline snapshot на атомарный set snapshot с
  per-partition `Arc` reuse, manifest/file observations и set epoch без Salsa rebuild;
  проверка: `cargo test -p mcp-server partitioned_baseline_reload`.
- [x] 5.2 Повысить diagnostics schema/outputSchema до 14, расширить file/workspace
  classification, `result_id`, structured/text schema и
  bounded summaries с totals/returned/truncated общей/per-partition ветвями
  success/error; проверка:
  `cargo test -p mcp-server diagnostics_partitioned_baseline_response`.
- [x] 5.3 Добавить интеграционный тест единого semantic snapshot, partial/stale,
  missing/corrupt/recovery/cancellation; проверка:
  `cargo test -p mcp-server --test partitioned_diagnostics_baseline`.

## 6. LSP

- [x] 6.1 Загружать тот же set snapshot, публиковать new/protected owner diagnostics и
  fail-open все текущие diagnostics при ошибке любого partition; проверка:
  `cargo test -p bsl-analyzer lsp_partitioned_baseline_publish`.
- [x] 6.2 Наблюдать manifest/active files, переиспользовать неизменившиеся snapshots,
  при смене epoch повторно публиковать все открытые документы/workspace batch без
  замены Salsa и уведомлять один раз на partition error epoch; проверка:
  `cargo test -p bsl-analyzer lsp_partitioned_baseline_reload`.
- [x] 6.3 Добавить process-level LSP parity/partial tests для main, extension и group;
  проверка: `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_lsp`.
- [x] 6.4 Добавить один process-level parity-тест общей fixture/revision/set для CLI,
  MCP file/workspace и LSP с сравнением fingerprint, owner и new/known; проверка:
  `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_parity partitioned_baseline_cli_mcp_lsp_parity`.

## 7. Масштаб, документация и шлюзы

- [x] 7.1 Добавить Linux release-тест 1,6 млн записей с RSS <= 1,5 размера входа и
  счётчиками однократного parse/fingerprint без combined vector, плюс большой
  selected-update без повторной сериализации; проверки:
  `cargo test -p ide --release --test partitioned_baseline_scale -- --ignored` и
  `cargo test -p bsl-analyzer --release --test partitioned_baseline_scale large_selected_update_does_not_reserialize_unchanged_partitions -- --ignored`.
- [x] 7.2 Документировать auto-directory contract, groups, layout, all/selected команды,
  errors, v1 migration/rollback и CI usage; проверка:
  `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli documented_usage`.
- [x] 7.3 Сверить `traceability.md` с `spec.md` и `cargo test -- --list`: каждый
  Requirement/Scenario имеет исполняемый тест или объявленный CI job, ни один пункт не
  покрыт только ручной проверкой.
- [x] 7.4 Выполнить целевые шлюзы:
  `cargo fmt --all -- --check`,
  `cargo clippy -p ide -p ide-host-core -p project-model -p bsl-analyzer -p mcp-server --all-targets --all-features -- -D warnings`,
  `cargo test -p ide -p ide-host-core -p project-model -p bsl-analyzer -p mcp-server`.
- [x] 7.5 Выполнить полный regression/contract gate:
  `cargo test --all --no-fail-fast`, Windows transaction job,
  `openspec validate add-partitioned-diagnostics-baselines --strict --no-interactive`
  и `git diff --check`.

## Разрывы ревью

- [x] R1 Закрыть TOCTOU/no-follow и Windows atomic replace в capability-каталоге,
  прямое reuse неизменившихся objects и очистку временных файлов; проверки:
  `cargo test -p project-model partitioned_baseline_path_security` и
  `cargo test -p bsl-analyzer partitioned_baseline_transaction` плюс Windows CI.
- [x] R2 Сделать loader детерминированным: `missing_partition`/`orphan_partition`/
  identity/scope в заданном порядке, ограниченный повтор manifest-race и O(changed)
  reload без обхода всех reused fingerprints; проверка:
  `cargo test -p ide partitioned_baseline_set_loader`.
- [x] R3 Вычислять реальный per-partition `Full`/`Partial`, исключить общий богатый
  resolved-вектор на масштабе и расширить release-тест на classify; проверки:
  `cargo test -p ide partitioned_baseline_coverage` и release scale gate.
- [x] R4 Исправить selected CLI: полная проверка set перед repair, repair только
  повреждённого ожидаемого object, точные owner diagnostics/counts/partitions/unchanged;
  проверка: `cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli`.
- [x] R5 Защитить error-observation paths, вычислять set/error epoch из наблюдённых
  bytes, хранить детерминированные partition errors и LSP notification ledger;
  проверки: `cargo test -p ide-host-core partitioned` и
  `cargo test -p bsl-analyzer lsp_partitioned_baseline_reload`.
- [x] R6 Применять единый остаточный бюджет MCP ко всему ответу и публиковать истинные
  `errors_total`/первую partition error с bounded details; проверка:
  `cargo test -p mcp-server diagnostics_partitioned_baseline_response`.
- [x] R7 Добавить реальные Linux release scale/classify и Windows transaction CI jobs,
  синхронизировать traceability; проверка: статическая проверка workflow и OpenSpec strict.
- [x] R8 Удалить подтверждённое неиспользуемое состояние и дублирование без широких
  косметических рефакторингов; проверка: Clippy и целевые тесты.
- [x] R9 Исключить ambient-чтение baseline watcher, считать error epoch из bytes и
  гарантировать reload без следования symlink/reparse; проверки:
  `cargo test -p ide-host-core partitioned` и
  `cargo test -p bsl-analyzer lsp_partitioned_baseline_reload`.
- [x] R10 Свести MCP к одному ограничителю полного ответа, сохранить первую
  детерминированную ошибку и покрыть ранние error-ветви; проверка:
  `cargo test -p mcp-server diagnostics_partitioned_baseline_response`.
- [x] R11 Не повышать ограниченный анализ до `Full`, фильтровать selected resolved до
  материализации и исправить исполняемое доказательство PDB-08; проверки:
  `cargo test -p ide partitioned_baseline_coverage` и release scale gate.
- [x] R12 Повторить полные Rust/OpenSpec шлюзы после закрытия ревью и проверить
  итоговый diff без commit/push/archive.
- [x] R13 Канонизировать относительный `-c` до origin-relative baseline resolution,
  перевести `create --from-v1` на однопроходную запись хешированных partition-файлов
  без общего raw/rich vector и повторной загрузки set, согласовать topology error в
  design; проверки: CLI regression, transaction unit, Linux migration RSS gate,
  OpenSpec strict и полные Rust-шлюзы.
