#!/bin/bash
set -euo pipefail

# Скрипт для локальной сборки и публикации релиза bsl-analyzer
# Использование: ./scripts/release.sh <version> [--upload]
#
# Собирает два артефакта:
#   - bsl-launcher-{platform}     - launcher для IDE/репозиториев
#   - bsl-analyzer-app-{platform} - LSP сервер (скачивается с release-server)
#
# Переменные окружения:
#   RELEASE_SERVER_URL   - URL сервера релизов (default: http://localhost:18080)
#   RELEASE_UPLOAD_TOKEN - токен для загрузки (обязателен для --upload)

VERSION=${1:-}
UPLOAD=${2:-}

if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version> [--upload]"
    echo "Example: $0 0.1.0 --upload"
    echo ""
    echo "Builds:"
    echo "  - bsl-launcher-{platform}     (for IDE/repos)"
    echo "  - bsl-analyzer-app-{platform} (LSP server)"
    echo ""
    echo "Environment variables:"
    echo "  RELEASE_SERVER_URL   - Release server URL (default: http://localhost:18080)"
    echo "  RELEASE_UPLOAD_TOKEN - Upload token (required for --upload)"
    exit 1
fi

RELEASE_SERVER_URL=${RELEASE_SERVER_URL:-http://localhost:18080}
RELEASE_UPLOAD_TOKEN=${RELEASE_UPLOAD_TOKEN:-}

echo "=== Building bsl-analyzer ${VERSION} ==="

# Определяем текущую платформу
case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)   PLATFORM_SUFFIX="linux-amd64" ;;
    Linux-aarch64)  PLATFORM_SUFFIX="linux-arm64" ;;
    Darwin-x86_64)  PLATFORM_SUFFIX="darwin-amd64" ;;
    Darwin-arm64)   PLATFORM_SUFFIX="darwin-arm64" ;;
    MINGW*|MSYS*)   PLATFORM_SUFFIX="windows-amd64"; BINARY_EXT=".exe" ;;
    *)              echo "Unsupported platform: $(uname -s)-$(uname -m)"; exit 1 ;;
esac

BINARY_EXT=${BINARY_EXT:-}
LAUNCHER_NAME="bsl-launcher-${PLATFORM_SUFFIX}${BINARY_EXT}"
APP_NAME="bsl-analyzer-app-${PLATFORM_SUFFIX}${BINARY_EXT}"

echo "Platform: ${PLATFORM_SUFFIX}"

# Сборка
echo "Building release..."
cargo build --release

LAUNCHER_PATH="target/release/bsl-analyzer${BINARY_EXT}"
APP_PATH="target/release/bsl-analyzer-app${BINARY_EXT}"

if [ ! -f "$LAUNCHER_PATH" ]; then
    echo "Error: Launcher not found at ${LAUNCHER_PATH}"
    exit 1
fi

if [ ! -f "$APP_PATH" ]; then
    echo "Error: App not found at ${APP_PATH}"
    exit 1
fi

# Размеры бинарников
get_size() {
    if command -v stat &> /dev/null; then
        stat -c%s "$1" 2>/dev/null || stat -f%z "$1"
    else
        wc -c < "$1" | tr -d ' '
    fi
}

LAUNCHER_SIZE=$(get_size "$LAUNCHER_PATH")
APP_SIZE=$(get_size "$APP_PATH")
echo "Launcher size: ${LAUNCHER_SIZE} bytes ($(echo "scale=2; ${LAUNCHER_SIZE}/1048576" | bc) MB)"
echo "App size: ${APP_SIZE} bytes ($(echo "scale=2; ${APP_SIZE}/1048576" | bc) MB)"

# SHA256
get_sha256() {
    if command -v sha256sum &> /dev/null; then
        sha256sum "$1" | cut -d' ' -f1
    elif command -v shasum &> /dev/null; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        echo "Error: sha256sum or shasum not found"
        exit 1
    fi
}

LAUNCHER_SHA256=$(get_sha256 "$LAUNCHER_PATH")
APP_SHA256=$(get_sha256 "$APP_PATH")
echo "Launcher SHA256: ${LAUNCHER_SHA256}"
echo "App SHA256: ${APP_SHA256}"

if [ "$UPLOAD" = "--upload" ]; then
    if [ -z "$RELEASE_UPLOAD_TOKEN" ]; then
        echo "Error: RELEASE_UPLOAD_TOKEN not set"
        exit 1
    fi

    echo ""
    echo "=== Uploading to ${RELEASE_SERVER_URL} ==="

    # Загрузка launcher
    echo "Uploading ${LAUNCHER_NAME}..."
    curl -sf -X POST "${RELEASE_SERVER_URL}/upload" \
        -H "Authorization: Bearer ${RELEASE_UPLOAD_TOKEN}" \
        -H "X-Product: bsl-analyzer" \
        -H "X-Version: ${VERSION}" \
        -H "X-Platform: ${LAUNCHER_NAME}" \
        --data-binary "@${LAUNCHER_PATH}"
    echo " done"

    # Загрузка app
    echo "Uploading ${APP_NAME}..."
    curl -sf -X POST "${RELEASE_SERVER_URL}/upload" \
        -H "Authorization: Bearer ${RELEASE_UPLOAD_TOKEN}" \
        -H "X-Product: bsl-analyzer" \
        -H "X-Version: ${VERSION}" \
        -H "X-Platform: ${APP_NAME}" \
        --data-binary "@${APP_PATH}"
    echo " done"

    # Публикация версии
    echo ""
    echo "Publishing version ${VERSION}..."
    curl -sf -X POST "${RELEASE_SERVER_URL}/publish/bsl-analyzer/${VERSION}" \
        -H "Authorization: Bearer ${RELEASE_UPLOAD_TOKEN}"
    echo " done"

    echo ""
    echo "=== Release ${VERSION} published ==="
    echo "Latest version:"
    curl -sf "${RELEASE_SERVER_URL}/bsl-analyzer/latest" || echo "(failed to get latest)"
    echo ""
else
    echo ""
    echo "=== Build complete ==="
    echo "Launcher: ${LAUNCHER_PATH} -> ${LAUNCHER_NAME}"
    echo "App:      ${APP_PATH} -> ${APP_NAME}"
    echo ""
    echo "To upload to release server:"
    echo "  RELEASE_SERVER_URL=... RELEASE_UPLOAD_TOKEN=... $0 ${VERSION} --upload"
fi
