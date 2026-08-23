## 0. Входной шлюз

- [x] 0.1 Перед реализацией синхронизировать ветку с актуальным `upstream/develop`, повторно проверить owner paths и выполнить strict validation change.

## 1. BA-001 — сообщение о неизвестном получателе

- [x] 1.1 В `crates/ide-diagnostics/src/handlers/unresolved_method_call.rs` изменить только форматирование `ReceiverNotResolved`/`ReceiverNameAbsent` на нейтрального получателя вызова; прямыми unit cases проверить обе причины, имя и отсутствие утверждения о модуле.
- [x] 1.2 Добавить end-to-end fixture неизвестного имени слева от точки с проверкой прежних кода и диапазона, а также positive control известного модуля с отсутствующим методом; выполнить целевые тесты `ide-diagnostics` и существующий HIR-тест классификации причины.

## 2. BA-002 — локализация неизвестного оператора

- [x] 2.1 В `crates/parser/src/grammar/statements.rs` сохранить существующий `RecoverySpan` неизвестного самостоятельного identifier на его точном token и передать минимальный recovery-признак ближайшему statement list; parser regression с `КонецЕслли;` проверяет точный range.
- [x] 2.2 При достижении внешнего terminator не потреблять его и подавлять только производную ошибку отсутствующего terminator ближайшего уже помеченного внутреннего блока; nested regressions проверяют nearest-only поведение, сохранение более ранних ошибок и отсутствие производных ошибок на `КонецПроцедуры`/EOF.
- [x] 2.3 Проверить, что реально пропущенный `КонецЕсли` без отмеченного неизвестного оператора по-прежнему диагностируется, следующий корректный оператор разбирается, а допустимые assignment/call/expression не получают новый `ParseError`; выполнить целевые тесты `parser`, `syntax` и `ide-diagnostics::parse_error`.

## 3. BA-003 — отсутствующее поле закрытой Структура

- [x] 3.1 В `crates/bsl-types/src/facet.rs` и `builders.rs` добавить минимальный closed/open признак `StructureFacet`: существующие builders остаются open, явный доказанный путь создаёт closed; unit-тесты проверяют обе формы, interning и независимую полноту вложенных facets.
- [x] 3.2 В существующем whole-body проходе `crates/hir-ty/src/structure_keys.rs` устанавливать closed только после доказательства хотя бы одного литерального ключа из конструктора или `.Вставить`; dynamic key, неизвестный вызов/мутация, alias, reassignment, передача не в доказанный параметр `Знач`, return и escape консервативно открывают форму для всех использований; unit-тесты закрепляют каждую ветвь без нового dataflow-прохода.
- [x] 3.3 В существующем predicate выдачи inference-диагностики добавить `InferenceDiagnostic::UnresolvedField` только для miss непустой закрытой `StructureFacet`, не меняя мягкий field lookup открытых и бесключевых форм; tests проверяют существующий и отсутствующий литеральный ключ и пустой литерал.
- [x] 3.4 Добавить inference/diagnostics fixtures для пустого литерала, literal insert, dynamic key, неизвестного вызова, alias/reassignment, return/escape, параметра `Знач` и открытой typed/doc-comment структуры; проверить отсутствие false positive, а для закрытого miss — код, range и count `UnresolvedField`.
- [x] 3.5 Добавить IDE regressions неизменных completion и hover доказанных ключей закрытой и открытой формы; выполнить целевые тесты `bsl-types`, `hir-ty`, `ide` и `ide-diagnostics`.

## 4. BA-008 — контракт аргумента metadata object

- [x] 4.1 Обновить авторитетный doc comment поля `MetadataParams.object_type`, summary инструмента `metadata` и `docs/mcp/TOOLS_AND_EXTENSION.md`: singular для source и auto без connection, plural для infobase и auto с connection; `form` остаётся source-only, JSON Schema, actions и response shape не меняются.
- [x] 4.2 Обновить `tools/list` contract assertion и проверить неизменность schema/actions; описание должно публиковать те же mode-dependent правила и примеры, что репозиторная документация.
- [x] 4.3 В существующих `mcp-server` routing tests проверить source, infobase, auto с connection и auto без connection; локальный HTTP fixture подтверждает неизменную передачу plural и неверной singular формы, отсутствие повторного нормализованного запроса и возврат существующей ошибки; выполнить целевые тесты `mcp-server` и существующий serialization test `onec-client`.

## 5. BA-009 — defaults без ложного warning

- [x] 5.1 В `DiagnosticsConfig::from_project_json` добавить ранний возврат для `null` с defaults и разрешённой locale, не меняя десериализацию остальных значений; unit-тесты с capture tracing проверяют 0 warnings для `null` и `{}`, ровно 1 для invalid string, fallback и сохранение корректных parameters/locale.
- [x] 5.2 В CLI integration tests запустить `analyze` без конфигурации и с явно некорректным значением при `BSL_LOG=warn`; проверить успешное завершение и соответственно 0 или ровно 1 вхождение warning в stderr, а также подтвердить, что существующие CLI/LSP/MCP callsites продолжают использовать общий `from_project_json` без runtime-specific обходов.

## 6. Трассируемость и финальные шлюзы

- [x] 6.1 Для каждого scenario трёх delta specs указать прямой test identifier в verification evidence; если обязательное поведение невозможно проверить, сначала согласованно изменить нормативный spec, а не завершать change с исключением.
- [x] 6.2 Выполнить `cargo fmt --all -- --check`, целевой `cargo clippy` для затронутых `bsl-types`, `parser`, `syntax`, `hir-ty`, `ide`, `ide-diagnostics`, `mcp-server`, `onec-client` и `bsl-analyzer` с `-D warnings`, а также `git diff --check`.
- [x] 6.3 Выполнить целевые тесты этих же crates и CLI integration suites, перечисленные в BA-блоках.
- [x] 6.4 Выполнить `cargo test --all --no-fail-fast`, `./scripts/check-invariants.sh` и `openspec validate fix-ba001-ba002-ba003-ba008-ba009 --strict`; BA закрываются только после прямого regression evidence соответствующих scenarios.

## Расширение финального шлюза

- [x] 7.1 Восстановить отсутствующий авторитетный `docs/legal/bsl-clean-room-slice-b3.md`, согласовать его checklist с новой grammar-функцией и выполнить весь `grammar_attestation` suite.
- [x] 7.2 Закрыть четыре существующих facade-boundary violations `PlatformData::instance()` в `crates/ide` минимальным способом, разрешённым invariant gate, и выполнить адресные IDE tests плюс `check-invariants.sh`.
- [x] 7.3 Повторить `cargo test --all --no-fail-fast`, strict OpenSpec validation, fmt, clippy и diff checks; после успеха закрыть 6.4 и перейти к независимому ревью.

## Разрывы ревью

- [x] 8.1 Консервативно открывать форму `Структура` при escape в любых выражениях statement (условия, границы циклов, Raise/Execute/AddHandler и вложенные вызовы), добавить прямые false-positive regression tests и повторить BA-003 проверки.
- [x] 8.2 Упростить facade-boundary repair: удалить одноразовый публичный `PlatformMembers` и оставить один минимальный accessor существующих platform data; выполнить IDE tests и invariant gate.
- [x] 8.3 Восстановить в `bsl-clean-room-slice-b3.md` содержательные findings `D1`–`D10`, на которые ссылается parser provenance, и повторить `grammar_attestation`.
- [x] 8.4 Открывать форму при whole-value escape через wrapper expressions (wrapped Return, ternary alias, constructor/container argument), не открывая простое чтение поля; добавить прямые regressions.
- [x] 8.5 Удалить дублированный match по всем `Expr`, переиспользовав существующий child-walker `narrow::for_each_expr_child`; повторить BA-003 tests и clippy.
- [x] 8.6 Удалить дублированный проход аргументов `invalidate_call_escapes`, оставив receiver mutation в общем expression walker; убрать эквивалентные прямые invalidation и повторить BA-003 tests/clippy.
