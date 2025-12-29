# Contributing to bsl-analyzer

Спасибо за интерес к проекту! Этот документ описывает процесс контрибуции.

## Быстрый старт

### 1. Настройка окружения

**Требования:**
- Rust 1.75+ (`rustup install stable`)
- Git
- jq (для скрипта проверки CI)

**Клонирование:**
```bash
git clone http://gitlab.runsystems.ru/proit/bsl-analyzer.git
cd bsl-analyzer
```

**Установка pre-commit hooks:**
```bash
./scripts/setup-hooks.sh
```

Это автоматически запустит `cargo fmt` и `cargo clippy` перед каждым коммитом.

### 2. Проверка работоспособности

```bash
# Сборка
cargo build

# Тесты
cargo test --all

# Проверка форматирования
cargo fmt --all -- --check

# Clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### 3. Внесение изменений

1. Создайте ветку от `main`:
   ```bash
   git checkout -b feature/my-feature
   ```

2. Вносите изменения согласно [правилам разработки](docs/contributing/DEVELOPMENT_RULES.md)

3. Коммитьте с осмысленными сообщениями:
   ```bash
   git commit -m "feat: add new diagnostic for empty code blocks"
   ```

4. Пушьте и создавайте Merge Request:
   ```bash
   git push -u origin feature/my-feature
   ```

## Правила разработки

### Обязательно к прочтению

- **[DEVELOPMENT_RULES.md](docs/contributing/DEVELOPMENT_RULES.md)** — правила написания кода
- **[VERSIONING.md](docs/contributing/VERSIONING.md)** — политика версионирования
- **[ROADMAP.md](docs/planning/ROADMAP.md)** — план разработки
- **[SOURCES.md](docs/planning/SOURCES.md)** — проекты-источники

### Ключевые правила

1. **Перед использованием библиотеки** — изучите актуальную документацию (Context7 плагин для AI)
2. **Код без warnings** — `cargo clippy` должен проходить без ошибок
3. **Форматирование** — `cargo fmt` перед коммитом
4. **Тесты обязательны** — новый функционал = новые тесты
5. **Не ломайте старые тесты** — если тест упал, разберитесь почему

## Структура коммитов

Используем [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — новая функциональность
- `fix:` — исправление бага
- `docs:` — изменения документации
- `style:` — форматирование (не влияет на код)
- `refactor:` — рефакторинг
- `test:` — добавление тестов
- `chore:` — вспомогательные задачи (CI, build, etc.)

**Примеры:**
```
feat: implement CanonicalSpellingKeywords diagnostic
fix: handle empty files in parser
docs: update ROADMAP with iteration 5 details
test: add snapshot tests for expression parsing
```

## Процесс Merge Request

1. **Убедитесь, что CI проходит:**
   ```bash
   ./scripts/ci-status.sh
   ```

2. **Merge Request должен содержать:**
   - Описание изменений
   - Ссылку на issue (если есть)
   - Скриншоты (для UI изменений)

3. **Review:**
   - Минимум 1 approver
   - Все комментарии должны быть разрешены
   - CI должен быть зелёным

4. **Merge:**
   - Используем "Squash and merge" для чистой истории
   - Удаляем ветку после merge

## Работа с итерациями

Проект разбит на 30 итераций (см. [ITERATIONS.md](docs/planning/ITERATIONS.md)).

**Перед началом работы над итерацией:**

1. Прочитайте описание итерации
2. Изучите проекты-источники для данной итерации
3. Создайте issue для итерации
4. Разбейте на подзадачи если нужно

**При завершении итерации:**

1. Все тесты проходят
2. Документация обновлена
3. CHANGELOG.md обновлён
4. CI зелёный

## Разработка диагностик

См. [DIAGNOSTICS_MIGRATION.md](docs/planning/DIAGNOSTICS_MIGRATION.md) для детального плана миграции 181 диагностики.

**Шаблон для новой диагностики:**

1. Создайте файл в `crates/ide-diagnostics/src/handlers/`
2. Добавьте код диагностики в `DiagnosticCode` enum
3. Реализуйте функцию `check(ctx: &DiagnosticsContext) -> Vec<Diagnostic>`
4. Добавьте тесты с тестовыми данными из bsl-language-server
5. Обновите документацию

## Вопросы и помощь

- **Issues:** https://gitlab.runsystems.ru/proit/bsl-analyzer/-/issues
- **Merge Requests:** https://gitlab.runsystems.ru/proit/bsl-analyzer/-/merge_requests

## Лицензия

Контрибутируя в проект, вы соглашаетесь с тем, что ваш вклад будет лицензирован под MIT OR Apache-2.0.
