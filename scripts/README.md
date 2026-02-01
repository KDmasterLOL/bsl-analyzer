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

---

## release.sh

Локальная сборка и публикация релиза на release server.

**Использование:**

```bash
# Только сборка (без публикации)
./scripts/release.sh 0.1.0

# Сборка и публикация
RELEASE_SERVER_URL=https://releases.example.com \
RELEASE_UPLOAD_TOKEN=secret \
./scripts/release.sh 0.1.0 --upload
```

**Переменные окружения:**
- `RELEASE_SERVER_URL` — URL сервера релизов (default: http://localhost:18080)
- `RELEASE_UPLOAD_TOKEN` — токен для загрузки (обязателен для --upload)

**Что делает:**
- Собирает `bsl-analyzer` в release mode
- Определяет платформу (linux/darwin/windows, amd64/arm64)
- Вычисляет SHA256 checksum
- Загружает бинарник на release server (с --upload)
- Публикует версию (генерирует manifest.json и подпись)

---

## build-macos.sh

Сборка macOS бинарников на MacBook с публикацией в GitLab.

**Использование:**

```bash
# Сборка последнего тега
GITLAB_TOKEN=xxx GITLAB_PROJECT_ID=group/project ./scripts/build-macos.sh

# Сборка конкретного тега
GITLAB_TOKEN=xxx GITLAB_PROJECT_ID=group/project ./scripts/build-macos.sh v0.1.0

# Watch mode (автосборка новых тегов каждые 5 минут)
GITLAB_TOKEN=xxx GITLAB_PROJECT_ID=group/project ./scripts/build-macos.sh --watch
```

**Переменные окружения:**
- `GITLAB_TOKEN` — GitLab Personal Access Token с `api` scope (обязательно)
- `GITLAB_PROJECT_ID` — ID проекта или путь `group/project` (обязательно)
- `GITLAB_URL` — URL GitLab (default: https://gitlab.com)
- `RELEASE_SERVER_URL` — URL custom release server (опционально)
- `RELEASE_UPLOAD_TOKEN` — токен для custom server (опционально)

**Что делает:**
- Собирает для `x86_64-apple-darwin` и `aarch64-apple-darwin`
- Загружает в GitLab Package Registry
- Добавляет ссылки на бинарники в GitLab Release
- Опционально загружает на custom release server

---

## Release Pipeline

### Архитектура распространения

```
┌─────────────────────────────────────────────────────────────────┐
│  Repository / IDE                                               │
│  └── bsl-analyzer (launcher, ~2 MB)                            │
│         │                                                       │
│         ▼ скачивает при первом запуске                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Release Server (releases-server)                        │   │
│  │  └── /bsl-analyzer/{version}/                           │   │
│  │      ├── manifest.json + manifest.sig (Ed25519)         │   │
│  │      ├── bsl-launcher-{platform}                         │   │
│  │      └── bsl-analyzer-app-{platform}                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│         │                                                       │
│         ▼ кеширует в                                           │
│  ~/.bsl-analyzer/bin/                                          │
│  ├── current -> bsl-analyzer-0.1.0                             │
│  ├── bsl-analyzer-0.1.0                                        │
│  └── .last_check                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Артефакты

| Артефакт | Описание | Распространение |
|----------|----------|-----------------|
| `bsl-launcher-{platform}` | Минимальный launcher (~2 MB) | В репозиториях, IDE extensions |
| `bsl-analyzer-app-{platform}` | LSP сервер (полный) | Скачивается с release-server |

### Полный процесс релиза

1. **Обновить версию** в `Cargo.toml` (workspace.package.version)
2. **Создать тег:** `git tag v0.1.0 && git push origin v0.1.0`
3. **GitLab CI автоматически:**
   - Запускает тесты
   - Собирает Linux и Windows (launcher + app)
   - Загружает на release server (если настроен)
   - Создаёт GitLab Release
4. **На MacBook:** запустить `./scripts/build-macos.sh v0.1.0`

### Команды launcher

```bash
bsl-analyzer --launcher-version      # Версия launcher
bsl-analyzer --launcher-update       # Обновить LSP сервер
bsl-analyzer --launcher-verify       # Проверить целостность
bsl-analyzer --launcher-self-update  # Обновить сам launcher
```

### CI/CD Variables (в Settings → CI/CD → Variables)

- `RELEASE_SERVER_URL` — URL release server
- `RELEASE_UPLOAD_TOKEN` — Bearer token (masked)

### Переменные окружения

- `BSL_RELEASE_URL` — переопределить URL release server (для launcher)
