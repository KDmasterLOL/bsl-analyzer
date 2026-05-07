# UnknownPreprocessorSymbol

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит неизвестные символы в условиях препроцессора `#Если` / `#If`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/unknown_preprocessor_symbol.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/UnknownPreprocessorSymbol.md`

## Как реализовано

AST single-pass node handler проверяет `SyntaxKind::PRE_SYMBOL` через `preprocessor_symbols::is_known_symbol`.

## Что покрыто

Покрыты русские/английские условия, complex expressions с `И/ИЛИ/НЕ`, OS symbols и неизвестные идентификаторы.

## Пробелы и ограничения

Нет suggestions для похожих символов. Качество зависит от полноты списка известных symbols.

## Может ли инфраструктура улучшить качество

Да. Нужен source of truth по символам препроцессора и fuzzy suggestions.

## Возможное объединение

Близко к `SeveralCompilerDirectives`, `CompilationDirectiveLost`, `CompilationDirectiveNeedLess`. Общий preprocessor analyzer нужен.

## Вывод

Правило точное, но UX можно улучшить подсказками корректных символов.
