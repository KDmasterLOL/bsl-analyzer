# DeprecatedPlatformApi

Статус: `done`, `folded-platform-deprecations`

Дата актуализации: 2026-06-27

## Суть правила

Единая активная диагностика для устаревшего API платформы. Она сохранила
покрытие старых platform-deprecation правил и заменила активные public codes:
`DeprecatedAttributes8312`, `DeprecatedCurrentDate`, `DeprecatedFind`,
`DeprecatedMessage`, `DeprecatedMethods8310`, `DeprecatedMethods8317`,
`DeprecatedTypeManagedForm`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/deprecated_platform_api.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_current_date.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_find.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_message.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_method.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_attributes_8312.rs`
- `crates/ide-diagnostics/src/handlers/deprecated_type_managed_form.rs`
- `crates/ide-diagnostics/docs/{ru,en}/DeprecatedPlatformApi.md`
- `docs/legal/diagnostics/DeprecatedPlatformApi.md`

## Как реализовано

Handler'ы старых семейств продолжают использовать свои локальные HIR/facts
детекторы, но эмитят единый `DiagnosticCode::DeprecatedPlatformApi`.
Метаданные диагностики имеют теги `Standard` и `Deprecated`, чтобы LSP/UI
получали `DiagnosticTag::Deprecated`.

## Исторические карточки

Старые audit/legal карточки оставлены только как provenance history. Их коды не
являются активными `DiagnosticCode` и не должны попадать в executable tooling
или live audit index как отдельные diagnostics.
