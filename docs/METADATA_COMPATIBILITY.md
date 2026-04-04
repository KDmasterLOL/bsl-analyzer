# Совместимость metadata диагностик

Этот документ описывает текущее состояние metadata-слоя для диагностик в
`ide-diagnostics` и то, что именно сейчас проверяется тестами.

## Текущее состояние

На текущий момент в проекте:

- **180 диагностик** в `DiagnosticCode`;
- **180/180 диагностик** имеют metadata в `handlers::get_metadata()`;
- **180/180 диагностик** имеют rule-документацию в `crates/ide-diagnostics/docs/ru` и `crates/ide-diagnostics/docs/en`.

Это подтверждается локальными проверками и тестами в `crates/ide-diagnostics`.

## Что входит в metadata

Для каждой диагностики metadata-слой задаёт как минимум:

- тип (`Error`, `Vulnerability`, `CodeSmell`, `SecurityHotspot`);
- severity-уровень;
- `minutes_to_fix`;
- `activated_by_default`;
- scope;
- теги;
- дополнительные признаки вроде `can_locate_on_project`.

## Что проверяется автоматически

Полезные точки проверки:

- `crates/ide-diagnostics/src/handlers.rs` — тест `test_all_diagnostics_have_metadata()`;
- `crates/ide-diagnostics/build.rs` — сборка списка документированных правил;
- `crates/ide-diagnostics/src/docs.rs` — доступ к встроенной документации правил.

Практически это покрывает такие инварианты:

- у каждого `DiagnosticCode` есть metadata;
- metadata не теряются при добавлении новых правил;
- rule-документация в `docs/ru` и `docs/en` существует для всех кодов;
- mapping severity и тегов остаётся согласованным.

## Mapping severity

### Тип диагностики -> итоговая severity

| Тип диагностики | Итоговая LSP/Rust severity |
|-----------------|----------------------------|
| `Error` | error-подобная severity |
| `Vulnerability` | error-подобная severity |
| `CodeSmell` | hint / information / warning |
| `SecurityHotspot` | warning |

### Как это трактуется в коде

`CodeSmell` может понижаться до `Hint` или `Information`, тогда как
`Error`/`Vulnerability` остаются в error-классе severity.

Это важно, потому что metadata влияет не только на описание правила, но и на то,
как оно выглядит в LSP-клиенте и в экспортируемых форматах.

## Mapping тегов

Теги metadata сопоставляются внутренним enum-вариантам Rust, например:

- `STANDARD` -> `Standard`
- `BADPRACTICE` -> `Badpractice`
- `BRAINOVERLOAD` -> `Brainoverload`
- `PERFORMANCE` -> `Performance`
- `SQL` -> `Sql`
- `DEPRECATED` -> `Deprecated`
- `UNUSED` -> `Unused`

## Что это даёт на практике

Наличие полного metadata-слоя позволяет:

- централизованно управлять severity и tags;
- не дублировать эти значения в каждом handler'е;
- надёжно экспортировать правила в внешние форматы;
- поддерживать совместимость между LSP, CLI и rule-документацией.

## Полезные команды

Проверить metadata и связанные тесты:

```bash
cargo test -p ide-diagnostics metadata_tests -- --nocapture
```

Проверить встроенную документацию правил:

```bash
cargo test -p ide-diagnostics docs::tests -- --nocapture
```

## Итог

Сейчас metadata-слой можно считать полностью покрытым для всех существующих
диагностик проекта: **180 кодов, 180 metadata-описаний, 180 документированных правил**.
