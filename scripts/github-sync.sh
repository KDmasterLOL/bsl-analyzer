#!/usr/bin/env bash
# Синхронизация с GitHub — чистое зеркало без истории коммитов
#
# Использование:
#   ./scripts/github-sync.sh              # синк текущей версии из Cargo.toml
#   ./scripts/github-sync.sh v0.1.33      # синк с указанным тегом
#   ./scripts/github-sync.sh --dry-run    # только показать что будет сделано
#
# Требования:
#   - SSH ключ с доступом к GitHub (локально)
#   - Или переменная GITHUB_SSH_KEY (в CI)

set -euo pipefail

# --- Конфигурация ---
GITHUB_REPO="git@github.com:itrous/bsl-analyzer.git"
GITHUB_REPO_SLUG="itrous/bsl-analyzer"
GITHUB_BRANCH="develop"

# Файлы и директории, исключаемые из GitHub-зеркала
EXCLUDE_PATTERNS=(
    ".gitlab-ci.yml"
    ".cargo/config.toml"
    "scripts/ci-status.sh"
    ".omc/"
    ".claude/"
    "crates/bsl-launcher/release-source.github.json"
)

# --- Цвета ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# --- Переменные ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
WORK_DIR=""
DRY_RUN=false
TAG=""
SSH_KEY_FILE=""

# --- Функции ---

log_info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

cleanup() {
    if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
        rm -rf "$WORK_DIR"
    fi
    if [[ -n "$SSH_KEY_FILE" && -f "$SSH_KEY_FILE" ]]; then
        rm -f "$SSH_KEY_FILE"
    fi
}
trap cleanup EXIT

get_version() {
    grep -m1 'version = ' "$PROJECT_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/'
}

setup_ssh_for_ci() {
    if [[ -z "${GITHUB_SSH_KEY:-}" ]]; then
        return
    fi
    log_info "Настройка SSH для CI..."
    SSH_KEY_FILE=$(mktemp)
    echo "$GITHUB_SSH_KEY" > "$SSH_KEY_FILE"
    chmod 600 "$SSH_KEY_FILE"
    export GIT_SSH_COMMAND="ssh -i $SSH_KEY_FILE -o StrictHostKeyChecking=no"
}

build_rsync_excludes() {
    local excludes=()
    for pattern in "${EXCLUDE_PATTERNS[@]}"; do
        excludes+=(--exclude "$pattern")
    done
    # Всегда исключаем .git и временные файлы
    excludes+=(--exclude ".git")
    excludes+=(--exclude ".git/")
    excludes+=(--exclude "target/")
    echo "${excludes[@]}"
}

sync_files() {
    local src="$1"
    local dst="$2"

    log_info "Синхронизация файлов..."

    # Удаляем всё в целевой директории (кроме .git) чтобы отслеживать удалённые файлы
    find "$dst" -mindepth 1 -maxdepth 1 ! -name '.git' -exec rm -rf {} +

    # Копируем файлы с исключениями
    local excludes
    excludes=$(build_rsync_excludes)
    # shellcheck disable=SC2086
    rsync -a $excludes "$src/" "$dst/"
}

# --- Парсинг аргументов ---

for arg in "$@"; do
    case "$arg" in
        --dry-run)
            DRY_RUN=true
            ;;
        v*)
            TAG="$arg"
            ;;
        *)
            log_error "Неизвестный аргумент: $arg"
            exit 1
            ;;
    esac
done

# --- Основная логика ---

VERSION=$(get_version)
if [[ -n "$TAG" ]]; then
    COMMIT_TAG="$TAG"
else
    COMMIT_TAG="v$VERSION"
fi

log_info "Проект: bsl-analyzer"
log_info "Версия: $VERSION"
log_info "Тег: $COMMIT_TAG"
log_info "GitHub: $GITHUB_REPO"

if $DRY_RUN; then
    log_warn "DRY RUN — изменения не будут отправлены"
    echo ""
    log_info "Исключаемые файлы:"
    for pattern in "${EXCLUDE_PATTERNS[@]}"; do
        echo "  - $pattern"
    done
    echo ""
    log_info "Будет создан коммит: \"Release $COMMIT_TAG\""
    exit 0
fi

# Настройка SSH для CI
setup_ssh_for_ci

# Создаём рабочую директорию
WORK_DIR=$(mktemp -d)
log_info "Рабочая директория: $WORK_DIR"

# Клонируем GitHub-репо (или создаём orphan если пустой)
log_info "Клонирование GitHub-репозитория..."
if git ls-remote "$GITHUB_REPO" HEAD &>/dev/null 2>&1; then
    # Проверяем есть ли коммиты
    REMOTE_HEAD=$(git ls-remote "$GITHUB_REPO" HEAD 2>/dev/null | awk '{print $1}')
    if [[ -n "$REMOTE_HEAD" ]]; then
        git clone --depth 1 --branch "$GITHUB_BRANCH" "$GITHUB_REPO" "$WORK_DIR/github" 2>/dev/null || {
            # Ветка main не существует — создаём orphan
            log_info "Ветка $GITHUB_BRANCH не найдена, создаём новую..."
            mkdir -p "$WORK_DIR/github"
            cd "$WORK_DIR/github"
            git init
            git checkout --orphan "$GITHUB_BRANCH"
            git remote add origin "$GITHUB_REPO"
        }
    else
        # Пустой репозиторий
        log_info "Пустой репозиторий, создаём начальный коммит..."
        mkdir -p "$WORK_DIR/github"
        cd "$WORK_DIR/github"
        git init
        git checkout --orphan "$GITHUB_BRANCH"
        git remote add origin "$GITHUB_REPO"
    fi
else
    log_error "Не удалось подключиться к $GITHUB_REPO"
    exit 1
fi

cd "$WORK_DIR/github"

# Синхронизируем файлы
sync_files "$PROJECT_ROOT" "$WORK_DIR/github"

# Подмена конфига для GitHub-сборки
cp "$PROJECT_ROOT/crates/bsl-launcher/release-source.github.json" \
   "$WORK_DIR/github/crates/bsl-launcher/release-source.json"

# Подмена источника в install.sh для GitHub
sed -i.bak 's/^INSTALL_SOURCE="gitlab"/INSTALL_SOURCE="github"/' \
    "$WORK_DIR/github/scripts/install.sh"
rm -f "$WORK_DIR/github/scripts/install.sh.bak"

# Подмена URL установки в README для GitHub
sed -i.bak '/<!-- INSTALL_URL:gitlab -->/,/<!-- \/INSTALL_URL -->/c\
<!-- INSTALL_URL:github -->\
```bash\
curl -fsSL https://raw.githubusercontent.com/'"$GITHUB_REPO_SLUG"'/develop/scripts/install.sh | bash\
```\
\
Или с указанием версии:\
\
```bash\
curl -fsSL https://raw.githubusercontent.com/'"$GITHUB_REPO_SLUG"'/develop/scripts/install.sh | bash -s -- --version 0.1.38\
```\
<!-- /INSTALL_URL -->' "$WORK_DIR/github/README.md"
rm -f "$WORK_DIR/github/README.md.bak"

# Проверяем есть ли изменения
git add -A
if git diff --cached --quiet 2>/dev/null; then
    log_ok "Нет изменений для синхронизации"
    exit 0
fi

# Показываем статистику
log_info "Изменения:"
git diff --cached --stat | tail -1

# Создаём коммит
git commit -m "Release $COMMIT_TAG" \
    --author="BSL Analyzer <bsl-analyzer@users.noreply.github.com>"

# Пушим коммит
log_info "Отправка в GitHub..."
git push -u origin "$GITHUB_BRANCH"

# Создаём тег (если ещё не существует)
if git ls-remote --tags origin "$COMMIT_TAG" | grep -q "$COMMIT_TAG"; then
    log_warn "Тег $COMMIT_TAG уже существует, пропускаем"
else
    git tag -a "$COMMIT_TAG" -m "Release $COMMIT_TAG"
    git push origin "$COMMIT_TAG"
    log_ok "Тег $COMMIT_TAG создан"
fi

log_ok "Синхронизация завершена!"
log_ok "Коммит: Release $COMMIT_TAG"
log_ok "URL: https://github.com/itrous/bsl-analyzer"
