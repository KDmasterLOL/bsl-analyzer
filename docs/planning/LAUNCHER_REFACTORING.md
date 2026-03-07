# Рефакторинг Launcher: Clean Architecture

## Цель

Поддержка двух источников обновлений:
- **Release Server** (корпоративный) — `dev.runsystems.ru/releases`
- **GitHub Releases** (opensource) — `github.com/itrous/bsl-analyzer/releases`

Источник определяется конфигом `release-source.json`, встроенным при компиляции. Sync-скрипт подменяет конфиг для GitHub-сборки.

## Текущее состояние

Монолитный `main.rs` (902 строки), всё в одном файле:
- URL release-сервера захардкожен как константа
- Логика загрузки, верификации и установки переплетена
- Формат URL зашит в строковые шаблоны по всему файлу

## Архитектура (Clean Architecture)

```
┌─────────────────────────────────────────────┐
│              main.rs (Frameworks)            │
│  CLI, process::Command, запуск bsl-analyzer │
├─────────────────────────────────────────────┤
│         Interface Adapters (провайдеры)      │
│  ┌───────────────┐  ┌────────────────────┐  │
│  │ ServerProvider │  │  GitHubProvider    │  │
│  │ manifest.json  │  │  GitHub REST API  │  │
│  │ manifest.sig   │  │  Release assets   │  │
│  │ ed25519 verify │  │  SHA256 checksums │  │
│  └───────────────┘  └────────────────────┘  │
├─────────────────────────────────────────────┤
│            Use Cases (бизнес-логика)         │
│  check_update, download, verify, install    │
│  self_update, cleanup                       │
├─────────────────────────────────────────────┤
│            Entities (доменные объекты)        │
│  Release, BinaryInfo, Platform, Version     │
└─────────────────────────────────────────────┘
```

### Entities (`entities.rs`)

Чистые структуры данных без зависимостей:

```rust
pub struct Release {
    pub version: String,
    pub binaries: HashMap<Platform, BinaryInfo>,
}

pub struct BinaryInfo {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub signature: Option<Vec<u8>>,  // None для GitHub
}

pub enum Platform {
    LinuxAmd64,
    WindowsAmd64,
    DarwinArm64,
}
```

### Use Cases (`use_cases.rs`)

Бизнес-логика, зависит только от trait `ReleaseProvider`:

```rust
pub fn check_for_update(
    provider: &dyn ReleaseProvider,
    current: Option<&str>,
) -> Result<Option<Release>>;

pub fn download_and_install(
    provider: &dyn ReleaseProvider,
    version: &str,
    cache_dir: &Path,
) -> Result<PathBuf>;

pub fn verify_installation(
    provider: &dyn ReleaseProvider,
    version: &str,
    cache_dir: &Path,
) -> Result<bool>;
```

### Interface Adapters — trait `ReleaseProvider` (`provider.rs`)

```rust
pub trait ReleaseProvider {
    /// Получить последнюю доступную версию
    fn fetch_latest_version(&self) -> Result<String>;

    /// Получить информацию о релизе (бинарники, хеши, подписи)
    fn fetch_release(&self, version: &str) -> Result<Release>;

    /// Скачать бинарник для платформы
    fn download_binary(&self, binary: &BinaryInfo) -> Result<Vec<u8>>;

    /// Проверить целостность (подпись или checksum)
    fn verify(&self, data: &[u8], binary: &BinaryInfo) -> Result<()>;
}
```

### ServerProvider (`provider/server.rs`)

Текущая логика release-сервера:

```rust
pub struct ServerProvider {
    base_url: String,      // "https://dev.runsystems.ru/releases"
    product: String,       // "bsl-analyzer"
    public_key: [u8; 32],  // ed25519
}
```

- `fetch_latest_version` → `GET {base_url}/{product}/latest` → plain text
- `fetch_release` → `GET {base_url}/{product}/{ver}/manifest.json` + `.sig`, ed25519 verify
- `download_binary` → `GET {base_url}/{product}/{ver}/{platform_binary}`
- `verify` → SHA256 из manifest + ed25519 подпись manifest

### GitHubProvider (`provider/github.rs`)

```rust
pub struct GitHubProvider {
    repo: String,  // "itrous/bsl-analyzer"
}
```

- `fetch_latest_version` → `GET api.github.com/repos/{repo}/releases/latest` → JSON → `tag_name` → strip `v`
- `fetch_release` → парсинг `assets` из JSON ответа GitHub API
- `download_binary` → `GET github.com/{repo}/releases/download/v{ver}/{binary}`
- `verify` → SHA256 checksum (из файла `checksums.txt` в assets)

## Конфигурация

### Файл `crates/bsl-launcher/release-source.json`

**GitLab (корпоративный):**
```json
{
    "provider": "server",
    "url": "https://dev.runsystems.ru/releases",
    "product": "bsl-analyzer",
    "public_key": "a2618f20b4a0d270b627c164f5b8bcecc7559f85a25489620d7ab614cc8efbe8"
}
```

**GitHub (opensource):**
```json
{
    "provider": "github",
    "repo": "itrous/bsl-analyzer"
}
```

### Встраивание при компиляции

```rust
const RELEASE_CONFIG: &str = include_str!("../release-source.json");
```

### Подмена в sync-скрипте

```bash
# В github-sync.sh после rsync
cp "$PROJECT_ROOT/crates/bsl-launcher/release-source.github.json" \
   "$WORK_DIR/github/crates/bsl-launcher/release-source.json"
```

Добавить `release-source.github.json` в `EXCLUDE_PATTERNS` скрипта.

## GitHub Actions: подписи и checksums

В `release.yml` после сборки добавить шаг:

```yaml
- name: Generate checksums
  run: |
    cd artifacts
    sha256sum * > checksums.txt
```

Файл `checksums.txt` загружается как release asset. GitHubProvider использует его для верификации.

## Структура файлов после рефакторинга

```
crates/bsl-launcher/
├── release-source.json          # GitLab конфиг (include_str!)
├── release-source.github.json   # GitHub конфиг (подменяется sync-скриптом)
└── src/
    ├── main.rs                  # CLI, запуск, оркестрация
    ├── entities.rs              # Release, BinaryInfo, Platform
    ├── use_cases.rs             # check_update, download, install, verify
    ├── provider.rs              # trait ReleaseProvider
    ├── provider/
    │   ├── server.rs            # ServerProvider (release server)
    │   └── github.rs            # GitHubProvider (GitHub Releases API)
    ├── cache.rs                 # Управление кэшем версий
    └── messages.rs              # i18n сообщения (ru/en)
```

## Порядок работы

1. **Выделить entities** — структуры Release, BinaryInfo, Platform из текущих Manifest/FileInfo
2. **Выделить trait ReleaseProvider** — абстрагировать текущую логику
3. **Реализовать ServerProvider** — перенести текущий код без изменения поведения
4. **Реализовать GitHubProvider** — новая реализация для GitHub Releases API
5. **Добавить конфиг** — `release-source.json` + фабрика провайдера
6. **Обновить sync-скрипт** — подмена конфига при копировании
7. **Обновить GitHub Actions** — генерация `checksums.txt`
8. **Тесты** — unit-тесты провайдеров с мок-ответами

## Совместимость

- `BSL_RELEASE_URL` env var продолжает работать как override для ServerProvider
- Формат кэша `~/.bsl-analyzer/bin/` не меняется
- CLI интерфейс не меняется
