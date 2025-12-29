# Scripts

Вспомогательные скрипты для разработки bsl-analyzer.

## setup-hooks.sh

Установка git pre-commit hooks.

**Использование:**

```bash
./scripts/setup-hooks.sh
```

**Что делает:**
- Устанавливает pre-commit hook в `.git/hooks/`
- Hook автоматически запускает перед каждым коммитом:
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`

**Обход hook:**
```bash
git commit --no-verify  # пропустить проверки (не рекомендуется)
```

---

## ci-status.sh

Проверка статуса GitLab CI/CD pipeline.

**Требования:**
- `jq` — для парсинга JSON
- GitLab API token в `git config --global gitlab.token`

**Использование:**

```bash
# Показать статус последнего pipeline
./scripts/ci-status.sh

# Показать статус конкретного pipeline
./scripts/ci-status.sh 564
```

**Вывод:**
- Общая информация о pipeline (статус, ветка, коммит)
- Таблица всех jobs с их статусами и длительностью
- Логи упавших jobs (последние 50 строк)

**Статусы:**
- ✓ success — job выполнен успешно
- ✗ failed — job упал
- ⟳ running — job выполняется
- ⧖ pending — job ожидает выполнения
- ⊝ skipped — job пропущен
- ⊗ canceled — job отменён
