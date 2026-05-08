# DeprecatedMethodCall

Статус: `done`, `needs-code-work`
Track 1 closure: G1 `27fb95ec`, G2 `1e5230fd` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.

Дата разбора: 2026-05-07

## Суть правила

Запрещает вызовы пользовательских методов, помеченных в документации как
`Устарела` / `Deprecated`, из неустаревшего кода. Deprecated method может
вызывать deprecated method.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_method_call.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DeprecatedMethodCall.md`
- `docs/legal/diagnostics/DeprecatedMethodCall.md`

## Как реализовано

HIR lowering эмитит candidates для local calls и qualified common-module calls.
Handler проверяет docs вызывающего метода, затем разрешает callee через
`symbol_tree` или `module_index.resolve_common_module()`, читает `MethodDocs`
и диагностирует deprecated callee.

## Что покрыто

Тесты покрывают local deprecated call, deprecated-to-deprecated exception,
non-deprecated call, deprecation info и часть qualified scenarios.

## Пробелы и ограничения

- Qualified resolution покрывает только common modules и только export methods.
- Bare-call candidate подавляется, если имя совпадает с local var/param;
  qualified candidate подавляется, если receiver — local var/param. Type-aware
  object methods не поддерживаются.
- Deprecated marker зависит от parser'а docs; нужно проверить multilingual
  форматы, markdown и `@deprecated`-подобные варианты.
- Нет quick-fix, хотя deprecation info может содержать replacement.

## Инфраструктурные улучшения

Нужен общий symbol/type resolver для методов и структурированный deprecation
metadata: replacement method, since, removal version, explanation.

## Возможное объединение

Не объединять с platform deprecated API как внешний код: здесь пользовательский
API и docs. Но оба семейства должны использовать общий deprecation model.

## Вывод

Это уже семантическая диагностика, полезнее простых name checks. Главный
лимит - разрешение только local/common-module методов.

