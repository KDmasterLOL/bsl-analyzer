# Verification

Дата: 2026-08-21. Все команды выполнены из корня репозитория без archive, commit или push.

## Сценарии спецификации

| Scenario | Автоматическое доказательство |
|---|---|
| Чужой токен замечен обычной проверкой | `workspace_lease::tests::the_newest_claim_owns_the_workspace`, `observed_foreign_owner_is_permanent` |
| Чужой токен первым замечен публикационным барьером | `workspace_lease::tests::publish_fence_latches_supersession` |
| Новый владелец завершился после замеченного вытеснения | `workspace_lease::tests::observed_foreign_owner_is_permanent`, `graph::build::tests::fresh_generation_reuses_completed_cache` |
| Повторный захват уже ожидает блокировку | `workspace_lease::tests::observed_owner_race_cannot_reclaim` |
| Временное отсутствие владения не является вытеснением | `workspace_lease::tests::transient_unclaimed_is_not_superseded`, `non_witness_states_remain_reclaimable` |
| Краткий захват прошёл между проверками | `workspace_lease::tests::non_witness_states_remain_reclaimable` (`brief` control) |
| Сборка графа выполнялась во время вытеснения | `graph::build::tests::superseded_build_is_discarded_after_owner_release`, `superseded_fused_writer_stops_mutating` |
| Overlay потерял аренду | `state::overlay_retry::tests::observed_supersession_never_resumes`, `takeover_during_a_pass_never_reports_ready` |
| Вытеснение произошло внутри поздней публикации | `graph::state::tests::superseded_late_publish_is_fenced` |
| Векторный проход потерял аренду | `bsl_search::engine::tests::embedding_publish_fence` (включая отказ standalone overlay batch) |
| Вытеснение произошло перед swap векторного индекса | `state::embed::tests::embedding_fence_distinguishes_retry_from_supersession` |
| Общая транзакция поиска потеряла аренду | `state::bootstrap::tests::superseded_bootstrap_stops_mutating`, `state::sync::tests::superseded_daemon_cannot_mutate_shared_search`, `tools::search::hybrid::tests::search_code_does_not_write_after_supersession`, `tools::search::status::tests::search_status_does_not_consume_overlay_dirty_state` |
| Таблица workspace roots изменилась между подготовкой и apply | `state::sync::tests::drift_keys_are_replanned_after_workspace_roots_change` |
| Публикационный барьер временно недоступен | `state::bootstrap::tests::startup_constructor_fence_distinguishes_retry_terminal_and_error`, `state::sync::tests::superseded_daemon_cannot_mutate_shared_search`, `state::embed::tests::embedding_fence_distinguishes_retry_from_supersession` |
| Shutdown остановил bootstrap во время повторной попытки | `state::bootstrap::tests::startup_constructor_fence_distinguishes_retry_terminal_and_error` |
| Собственный дескриптор доступен в пуле | `graph::snapshot::tests::superseded_graph_serves_only_preopened_snapshot`, `graph::state::tests::superseded_status_truth_table` |
| Дескриптор занят уже выполняющимся запросом | `graph::snapshot::tests::superseded_graph_serves_only_preopened_snapshot`, `graph::state::tests::superseded_status_truth_table` |
| Файл заменён при пустом пуле | `graph::snapshot::tests::superseded_graph_serves_only_preopened_snapshot`, `superseded_daemon_lifecycle` |
| Блокировка снимка временно занята | `graph::snapshot::tests::superseded_graph_serves_only_preopened_snapshot` |
| Вытеснение произошло до публикации снимка | `graph::state::tests::a_superseded_daemon_builds_no_graph`, `superseded_status_truth_table` |
| Содержательное действие не имеет собственного снимка | `graph_supersession_contract::graph_actions_fail_when_superseded_snapshot_is_gone` |
| Активный сеанс старого демона | `broker::superseded_backend_lifecycle`, live smoke ниже |
| Первоначальный захват временно не выполнен | `broker::superseded_backend_lifecycle`, `state::tests::terminal_supersession_is_not_transient_nonownership` |
| Переподключение после обновления бинарника | `graph::build::tests::fresh_generation_reuses_completed_cache`, `superseded_daemon_lifecycle`, live smoke ниже |
| Обновление контракта статуса | `tools::graph::tests::schema_advertises_the_current_contract_shape`, оба SQLite version tests ниже |

## Автоматические проверки

- Все точечные команды задач 5.1–5.2 прошли; `actionlint .github/workflows/ci.yml` успешен.
- `cargo fmt --all -- --check` — успешно.
- `cargo clippy -p bsl-search -p mcp-server --all-targets --all-features -- -D warnings` — успешно.
- `cargo test -p bsl-search --no-fail-fast` — 382 passed, 28 ignored, 0 failed.
- `cargo test -p mcp-server --no-fail-fast` — lib 707 passed, 1 ignored; все integration targets прошли.
- `cargo test -p mcp-server graph_db::tests::writes_and_reads_back_nodes_and_edges` — 1 passed.
- `cargo test -p bsl-search store::tests::open_stamps_current_schema_version` — 1 passed.
- `Cargo.toml`, `Cargo.lock` и cache-layout не изменены; новый runtime-формат или dependency не добавлены.
- `git diff --check` — успешно.
- `openspec validate prevent-superseded-daemon-reclaim --strict --no-interactive` — valid.

## Live broker smoke

Штатный `target/debug/bsl-analyzer-app mcp serve --mode broker` запускался тремя поколениями над одним малым временным workspace/cache. Broker key разводился несекретным `EMBEDDING_MODEL=smoke-{1,2,3}`; idle TTL был 60 секунд. MCP handshake и вызовы `graph(status|overview)`/`search(search_code)` шли через proxy/backend transport.

- PID поколений: `1947599`, `1947672`, `1949323`; после smoke ни один процесс не существует.
- Старое поколение при активном сеансе ответило `ready + superseded`.
- Три повторных `overview`/`search_code` старого поколения не изменили graph и основной файл `search.db`: inode, size, mtime_ns и SHA-256 совпали до и после. Этот smoke не сохранял отдельную identity для `search.db-wal`; неизменность пары DB/WAL проверяет Unix-интеграция `superseded_daemon_lifecycle`.
- После последнего disconnect старый backend завершился за 14 486 мс при idle TTL 60 000 мс.
- Третье поколение ответило `ready`, а graph identity не изменился: готовый кэш принят без rewrite/build.
- Временный workspace, cache, daemon log и все дочерние процессы гарантированно удалены; повторная проверка `ps` и `/tmp/bsl-superseded-smoke-*` дала пустой результат.

Очищенная идентичность файлов:

- graph: inode `1919549`, size `53248`, mtime_ns `1787325890020619064`, SHA-256 `478d104665475a9f5891098331fcc3c18cfb7a51b09271fefb536955ab975f51`;
- search: inode `1919545`, size `4096`, mtime_ns `1787325890007992665`, SHA-256 `0ab48b25cba617ed3a4acca0161813b314c095ef63544bb4af60769eb1012977`.
