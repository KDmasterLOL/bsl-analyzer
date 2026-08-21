## 1. BA-001 — сообщение о неизвестном получателе

- [ ] 1.1 Добавить в `crates/ide-diagnostics/src/handlers/unresolved_method_call.rs` регрессионный fixture неизвестного имени слева от точки; проверить код, диапазон, имя получателя и отсутствие формулировки о модуле, сохранив positive control для известного модуля с отсутствующим методом.
- [ ] 1.2 Изменить только форматирование `ReceiverNotResolved`/`ReceiverNameAbsent` на нейтрального получателя вызова и выполнить целевые тесты `ide-diagnostics` для `UnresolvedMethodCall`.

## 2. BA-002 — локализация неизвестного оператора

- [ ] 2.1 Добавить parser/diagnostics fixture с `КонецЕслли;`, корректным `КонецПроцедуры` и утверждениями о точном диапазоне первичной ошибки и отсутствии EOF-only результата.
- [ ] 2.2 В существующем statement parser и модели структурированной ошибки локализовать самостоятельный неизвестный идентификатор на его токене; восстановиться на границе оператора или известном терминаторе без fuzzy-сопоставления ключевых слов.
- [ ] 2.3 Добавить проверки восстановления: корректный оператор после ошибочного продолжает разбираться, а допустимые присваивания, вызовы и выражения с идентификаторами не получают новый `ParseError`; выполнить целевые тесты `parser`, `syntax` и `ide-diagnostics::parse_error`.

## 3. BA-003 — отсутствующее поле закрытой Структура

- [ ] 3.1 Расширить `StructureFacet` минимальным признаком полноты ключей, сохранив совместимость существующих builders, interning, completion и hover; unit-тестами проверить различие closed/open.
- [ ] 3.2 В `crates/hir-ty/src/structure_keys.rs` помечать форму закрытой только для доказанного локального литерального построения; динамический ключ, неизвестная мутация, передача по ссылке, escape или переопределение должны консервативно оставлять её открытой.
- [ ] 3.3 Подключить отсутствие поля закрытой формы к существующему `InferenceDiagnostic::UnresolvedField`, не меняя мягкое поведение открытой и бесключевой `Структура`.
- [ ] 3.4 Добавить inference/IDE fixtures для существующего и отсутствующего ключа прямого литерала, динамического ключа и escape, а также регрессию completion/hover; выполнить целевые тесты `hir-ty` и `ide`.

## 4. BA-008 — контракт аргумента metadata object

- [ ] 4.1 Обновить описание `metadata` в `crates/mcp-server/src/lib.rs` и `docs/mcp/TOOLS_AND_EXTENSION.md`: singular `object_type` для source и plural collection для infobase/auto с `connection`, без изменения JSON Schema и runtime-нормализации.
- [ ] 4.2 Обновить contract snapshot/интеграционный тест `tools/list` и routing tests для source, infobase и неверной формы; выполнить целевые тесты `mcp-server` и `onec-client`.

## 5. BA-009 — defaults без ложного warning

- [ ] 5.1 Изменить `DiagnosticsConfig::from_project_json`: `null` возвращает defaults с locale без warning, пустой объект остаётся валидным, явно некорректное ненулевое значение выдаёт один warning и fallback.
- [ ] 5.2 Дополнить unit-тесты `crates/ide-diagnostics/src/config.rs` и CLI integration test запуском `analyze` без конфигурации и с явно некорректной конфигурацией; проверить stderr, успешное завершение и единые semantics CLI/LSP/MCP.

## 6. Трассируемость и финальные шлюзы

- [ ] 6.1 Сопоставить каждый scenario трёх delta specs с прямым автоматическим тестом; любое непокрытое обязательное поведение оформить как явно согласованное исключение до завершения change.
- [ ] 6.2 Выполнить `cargo fmt --all -- --check`, целевой `cargo clippy` для затронутых crates с `-D warnings` и `git diff --check`.
- [ ] 6.3 Выполнить целевые тесты `parser`, `syntax`, `hir-ty`, `ide`, `ide-diagnostics`, `mcp-server`, `onec-client` и CLI integration tests.
- [ ] 6.4 Выполнить `cargo test --all --no-fail-fast`, `./scripts/check-invariants.sh` и `openspec validate fix-ba001-ba002-ba003-ba008-ba009 --strict`; зафиксировать проверенную версию без закрытия BA до прохождения соответствующего regression evidence.
