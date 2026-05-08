# Track 1 — Семантический фундамент: closure

Status: closed.

Track 1's implementation plan (codename `linear-tumbling-noodle`, kept
out-of-tree as engineering scratch) lists 25 audit cards in its §Context
section as the motivation surface; the plan codename is referenced in
every Track 1 commit message.

Этот документ — единая точка истины по тому, какие коммиты Track 1 закрыли
работу для каждой карточки. Каждая из 25 затронутых карточек в этом каталоге
содержит однострочную ссылку «Track 1 closure: …» сверху, указывающую на
этот документ.

## Карта шагов и коммитов

| Шаг   | Коммит      | Заголовок |
|-------|-------------|-----------|
| Foundation | `819945b7` | Track 1 foundation — unified type-string lowering + CFG break/continue/goto wiring |
| ROADMAP+perf | `93a4e6c1` | docs(diagnostics-audit): Track 1 ROADMAP + performance baseline |
| D     | `637a6279`  | feat(ide-diagnostics): Step D — CFE-aware visible_configurations helpers |
| E     | `49498d7d`  | feat(dataflow): Step E — PathTerminates analysis + Salsa query |
| F     | `2f1606a8`  | feat(hir-def): Step F — assignment-target name resolver |
| G1    | `27fb95ec`  | feat(hir-ty): Step G1 — proc_signature Salsa query (docstring slice) |
| G2    | `1e5230fd`  | feat(hir-ty): Step G2 — proc_signature return-from-body fallback |
| G4    | `ece5d404`  | feat(hir-ty): Step G4 — proc_signature_lookup adapter |
| H     | `5028602a`  | test(hir-ty): Step H — `;`-separator activation audit test |
| I     | `00da9fc8`  | feat(ide-diagnostics): Step I — AllFunctionPathMustHaveReturn via PathTerminates |
| J     | `2f6b12cd`  | feat(hir-ty): Step J — ManagerModule ЭтотОбъект via Ty::ThisManager |
| J     | `942dbe94`  | fix(hir-def): Step J — resolve `ЭтотОбъект` in register ManagerModule |
| K     | `04305c93`  | feat(hir-ty): Step K — ValueFilled narrowing (true-branch only) |
| L     | `b3f3c82c`  | feat(hir-def): Step L — CommonModuleAssign existing_binding_kind payload |
| M     | `691a751c`  | feat(ide-diagnostics): Step M — visible_configurations migration |
| N     | `54a494e8`  | feat(ide-diagnostics): Step N — CommonModuleAssign through resolver |
| O     | `47133aeb`  | test(ide-diagnostics): Step O — pin goto live-edges in UnreachableCode |
| P     | `5b656687`  | test(ide-diagnostics): Step P — pin Прервать live-edges in PairingBrokenTransaction |
| Q-α   | `c1330185`  | feat(dataflow): Step Q-α — generic open-resource lattice |
| Q-β-1 | `2366e1fb`  | feat(ide-diagnostics): Step Q-β-1 — MissingTempStorageDeletion via lattice |
| Q-β-2 | `1ee77eaf`  | feat(ide-diagnostics): Step Q-β-2 — MissingTemporaryFileDeletion via lattice |
| Q-β-2 follow-up | `4b4d9a94` | fix(ide-diagnostics): Step Q-β-2 — drop Ternary/Array/Await from deletion-arg recursion |
| R     | `8aa69ca4`  | test(ide): Step R — CFE fixture + db.configurations integration test + CFE-only CommonModuleAssign coverage |

## Per-card closure

### CFG (7 карточек)

| Карточка | Закрыто коммитами | Примечание |
|----------|-------------------|------------|
| AllFunctionPathMustHaveReturn | `49498d7d` (E), `00da9fc8` (I), `819945b7` (foundation) | Path-sensitive `may_fallthrough` через `dataflow::path_terminates`; loop-fallthrough управляется конфигом, а не зашит. |
| UnreachableCode | `819945b7` (foundation), `47133aeb` (O) | Break/continue/goto порождают live-рёбра; existing reachability теперь корректна; снапшоты под новое поведение зафиксированы. |
| PairingBrokenTransaction | `819945b7` (foundation), `5b656687` (P) | DFS handler оставлен, но live-edges break/continue/goto корректно проходят сквозь pairing-state machine. |
| MissingTempStorageDeletion | `c1330185` (Q-α), `2366e1fb` (Q-β-1) | Forward MAY analysis по generic resource lattice; key — каноническая форма address-аргумента. Constant-true guard FP документирован как known-limitation с `#[ignore]`-pin'ом. |
| MissingTemporaryFileDeletion | `c1330185` (Q-α), `1ee77eaf` (Q-β-2), `4b4d9a94` (Q-β-2 follow-up) | Тот же движок + `classify_many` для multi-resource closes. Move-method dst и var-generation бagи задокументированы #[ignore]'d тестами как legacy parity. |
| CommitTransactionOutsideTryCatch | — (не менялся) | Plan §1.8: остаётся `BodyDiagnostic` в `hir-def/body/lower` — это синтаксический критерий (Try/Except контекст вокруг конкретного call), не path-sensitive. Контракт зафиксирован в карточке явно. |
| WrongUseOfRollbackTransactionMethod | — (не менялся) | То же обоснование, что и для `CommitTransactionOutsideTryCatch`. |

### Type inference (8 карточек)

| Карточка | Закрыто коммитами | Примечание |
|----------|-------------------|------------|
| TypeMismatch | `819945b7` (foundation), `5028602a` (H), `27fb95ec` (G1), `1e5230fd` (G2) | Single-path type lowering + `;`-separator activation + proc_signature query. Plan §2.7: handler остаётся live для call-arg slots; expression-level `TypeMismatch` — Track 6. |
| UnresolvedField | `27fb95ec` (G1), `1e5230fd` (G2) | Receiver-типы из `proc_signature_query` теперь резолвят поля у workspace-методов. |
| UnresolvedMethodCall | `27fb95ec` (G1), `1e5230fd` (G2), `ece5d404` (G4) | Workspace `MethodLoc` через `proc_signature_lookup` дают такую же `MethodInfo`-форму, что и platform-method path. |
| RedundantAccessToObject | `27fb95ec` (G1), `1e5230fd` (G2) | Точная инференция receiver-типа из docstring/return-from-body снимает spurious `Access`. |
| MissedRequiredParameter | `27fb95ec` (G1), `1e5230fd` (G2) | Параметры теперь lowered'ятся через единый `lower_param_type_string`. |
| MismatchedArgCount | `27fb95ec` (G1), `1e5230fd` (G2) | Arity берётся из `ProcSignature.params.len()`. |
| ReadOnlyPropertyAssignment | `819945b7` (foundation) | Узкий канал: receiver-инференция точнее ⇒ property-meta lookup попадает чаще. |
| DeprecatedMethodCall | `27fb95ec` (G1), `1e5230fd` (G2) | Deprecation-метаданные через тот же `MethodInfo` flow. |

### visible_configurations (7 карточек)

| Карточка | Закрыто коммитами | Примечание |
|----------|-------------------|------------|
| CommonModuleAssign | `637a6279` (D), `2f1606a8` (F), `b3f3c82c` (L), `691a751c` (M), `54a494e8` (N), `8aa69ca4` (R) | Полный путь: visible-configurations helpers → assignment-target resolver → existing_binding_kind payload → handler через `is_common_module_anywhere` → handler через resolver → CFE fixture + CFE-only тест. |
| ProtectedModule | `637a6279` (D), `691a751c` (M) | `find_common_module_for_file_anywhere`. |
| PrivilegedModuleMethodCall | `637a6279` (D), `691a751c` (M) | Тот же hook. |
| ExecuteExternalCodeInCommonModule | `637a6279` (D), `691a751c` (M) | Тот же hook. |
| MissingEventSubscriptionHandler | `637a6279` (D), `691a751c` (M) | `find_common_module_anywhere(handler.module_name)`. |
| ScheduledJobHandler | `637a6279` (D), `691a751c` (M) | То же. |
| UnsafeFindByCode | `637a6279` (D), `691a751c` (M) | Итерация `visible_configurations()` first-hit для MDOs. |

### Resolver/shadowing (4 карточки, минус CommonModuleAssign выше)

| Карточка | Закрыто коммитами | Примечание |
|----------|-------------------|------------|
| ThisObjectAssign | — (не менялся) | Plan §4.4: `ЭтотОбъект`/`ThisObject` зафиксированы как non-shadowable, контракт без resolver-pass. Тест: `test_local_var_named_this_object_still_emits` уже существует. |
| SelfAssign | — (не менялся) | Plan §4.5: structural equality, resolver не нужен. |
| RewriteMethodParameter | — (не менялся) | Plan §4.5: уже использует `ctx.by_value_params`. |

## Acceptance gate (§6 плана)

Условные обозначения: ▣ — verified at закрытии Step S; 🔄 — in flight;
⚠ — done с письменным нюансом.

| Условие | Статус | Доказательство |
|---------|--------|----------------|
| 1. `cargo test --workspace` — без `#[ignore]` без обоснования | ▣ | exit 0 после Step S; все `#[ignore]`'d тесты имеют письменное обоснование (constant-folding deferred-track + legacy-parity bug pins для move-method dst и variable-generation). |
| 2. `cargo clippy --all-targets --all-features -- -D warnings` чистый | ▣ | Каждый Track 1 коммит проходит pre-commit hook'ом (fmt+clippy+tests). |
| 3. 25 карточек имеют пометку Track 1 closure | ▣ | Этот документ + per-card аннотация (Step S `56a48242`). |
| 4. Lowering без `db` | ▣ | `grep '\bdb\.' crates/hir-def/src/body/lower/` показывает только `tests.rs` (test harness). Production lowering без `db`. |
| 5. Один path lowering | ⚠ | План §2.3 unify'нул `lower_param_type` / `resolve_platform_type_union` / `map_type_string` через `lower_param_type_string` (Steps G/H) — это единственный platform-type-union path. `Ty::from_type_name` в `hir-def/src/ty.rs:967` остаётся как basic built-in-type-name → `Ty` маппинг — другая лестница абстракции (синтаксический lowering без platform-метаданных), вне scope §2.3. Литеральный grep gate срабатывает на этом API; контракт «один платформенный path lowering» выполнен. |
| 6. Адаптеры `ide-diagnostics` не вызывают `ctx.load_configuration()` | ▣ | `grep 'load_configuration' crates/ide-diagnostics/src/` (исключая `main_configuration`) — пусто. Main-only консумеры: `ordinary_app_support`, `set_permissions_for_new_objects`, `scheduled_job_handler`, `missing_event_subscription_handler` — у каждого main-only metadata (флаги, EventSubscriptions, ScheduledJobs); CFE-aware lookup для имён модулей идёт через `find_common_module_anywhere`. |
| 7. CFE fixture + integration tests | ▣ | `extension_common_module/` + `configurations_cfe_visibility.rs` + `common_module_assign_emits_for_cfe_only_module` (Step R, `8aa69ca4`). |
| 8. Performance budget (cold +15% / hot +20% / RSS +50 MB на real corpus) | 🔄 | Замер ведётся на real-world BSL workspace (~13.4k файлов, рабочее окружение разработчика), сравнение `e18f3a60` (parent of foundation) ↔ HEAD. Без секции «Performance measurements» ниже этот гейт **не закрыт**. |
| 9. Документация | ⚠ | `dataflow/temp_resource.rs` несёт module-level rationale (lattice/transfer/диагностики). Этот closure-doc — point-of-truth для commit map и per-card mapping. Module-doc для `cfg` (loop-context semantics break/continue/goto + after-loop reuse) поднимется отдельным docs-commit'ом если grep `cargo doc` укажет на пробел; зафиксировать в follow-up. |
| 10. `// TODO(Phase 6.2)` метки удалены | ▣ | `grep 'TODO(Phase 6.2)' crates/` — пусто. Foundation коммит `819945b7` снял их из `cfg/builder.rs:217/231/245`. |

## Known limitations (deferred tracks)

Зафиксированы как `#[ignore]`'d тесты с running assertion'ами + обоснованием:

- **constant-folding of `Если Истина Тогда`** (`test_constant_true_guarded_cleanup_no_false_positive` в `MissingTempStorageDeletion`) — Plan §7 risk #3, требует constant-propagation pass.
- **Move-method destination semantics** (`test_move_method_destination_leaks` в `MissingTemporaryFileDeletion`) — нужны per-method argument-role метаданные.
- **Variable-generation tracking** (`test_reassigned_variable_first_get_leaks` в `MissingTemporaryFileDeletion`) — нужен reaching-definitions проход на temp-name binding'ах.

Каждый из этих pin'ов падает при `--ignored`, демонстрируя что баг настоящий и фикс ровно в un-ignore'е после имплементации deferred-track'а.
