#!/bin/bash
# macOS build script for bsl-analyzer
# Run on MacBook to build and publish macOS binaries
#
# Собирает два артефакта для каждой архитектуры:
#   - bsl-launcher-darwin-{amd64,arm64}     - launcher
#   - bsl-analyzer-app-darwin-{amd64,arm64} - LSP сервер
#
# Usage:
#   ./scripts/build-macos.sh              # Build latest tag
#   ./scripts/build-macos.sh v0.1.0       # Build specific tag
#   ./scripts/build-macos.sh --watch      # Watch for new tags
#
# Required environment variables:
#   GITLAB_TOKEN        - GitLab Personal Access Token (api scope)
#   GITLAB_PROJECT_ID   - GitLab project ID
#
# Optional:
#   GITLAB_URL          - GitLab URL (default: https://gitlab.com)
#   RELEASE_SERVER_URL  - Custom release server URL
#   RELEASE_UPLOAD_TOKEN - Auth token for custom release server

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
STATE_FILE="$HOME/.bsl-analyzer-build-state"

cd "$REPO_DIR"

GITLAB_URL="${GITLAB_URL:-https://gitlab.com}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

log() { echo -e "${GREEN}[BUILD]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1" >&2; }

check_env() {
    if [[ -z "${GITLAB_TOKEN:-}" ]]; then
        error "GITLAB_TOKEN not set (need Personal Access Token with 'api' scope)"
        exit 1
    fi
    if [[ -z "${GITLAB_PROJECT_ID:-}" ]]; then
        error "GITLAB_PROJECT_ID not set (e.g., '12345' or 'group/project')"
        exit 1
    fi
}

get_latest_tag() {
    git fetch --tags --quiet
    git describe --tags --abbrev=0 2>/dev/null || echo ""
}

get_last_built_tag() {
    [[ -f "$STATE_FILE" ]] && cat "$STATE_FILE" || echo ""
}

save_built_tag() {
    echo "$1" > "$STATE_FILE"
}

build_target() {
    local target=$1

    log "Building for $target..."
    cargo build --release --target "$target"

    local ok=true
    for bin in bsl-analyzer bsl-analyzer-app; do
        if [[ -f "target/$target/release/$bin" ]]; then
            log "Built: target/$target/release/$bin"
        else
            error "Build failed: $bin for $target"
            ok=false
        fi
    done

    $ok
}

urlencode() {
    local string="$1"
    printf '%s' "$string" | jq -sRr @uri
}

upload_to_gitlab_registry() {
    local version=$1
    local binary_name=$2
    local binary_path=$3

    local project_id_encoded
    project_id_encoded=$(urlencode "$GITLAB_PROJECT_ID")

    log "Uploading $binary_name to GitLab Package Registry..."

    local url="${GITLAB_URL}/api/v4/projects/${project_id_encoded}/packages/generic/bsl-analyzer/${version}/${binary_name}"

    curl -sf --upload-file "$binary_path" \
        -H "PRIVATE-TOKEN: ${GITLAB_TOKEN}" \
        "$url" > /dev/null || {
        error "Failed to upload $binary_name to GitLab Package Registry"
        return 1
    }

    log "Uploaded $binary_name to Package Registry"
    echo "$url"
}

add_release_link() {
    local tag=$1
    local binary_name=$2
    local url=$3

    local project_id_encoded
    project_id_encoded=$(urlencode "$GITLAB_PROJECT_ID")

    log "Adding $binary_name to GitLab Release..."

    local existing
    existing=$(curl -sf \
        -H "PRIVATE-TOKEN: ${GITLAB_TOKEN}" \
        "${GITLAB_URL}/api/v4/projects/${project_id_encoded}/releases/${tag}/assets/links" \
        | jq -r ".[] | select(.name == \"$binary_name\") | .id" 2>/dev/null || echo "")

    if [[ -n "$existing" ]]; then
        curl -sf -X DELETE \
            -H "PRIVATE-TOKEN: ${GITLAB_TOKEN}" \
            "${GITLAB_URL}/api/v4/projects/${project_id_encoded}/releases/${tag}/assets/links/${existing}" > /dev/null
    fi

    curl -sf -X POST \
        -H "PRIVATE-TOKEN: ${GITLAB_TOKEN}" \
        -H "Content-Type: application/json" \
        -d "{\"name\": \"$binary_name\", \"url\": \"$url\", \"link_type\": \"package\"}" \
        "${GITLAB_URL}/api/v4/projects/${project_id_encoded}/releases/${tag}/assets/links" > /dev/null || {
        error "Failed to add $binary_name to release"
        return 1
    }

    log "Added $binary_name to Release"
}

upload_to_custom_server() {
    local version=$1
    local binary_name=$2
    local binary_path=$3

    if [[ -z "${RELEASE_SERVER_URL:-}" ]] || [[ -z "${RELEASE_UPLOAD_TOKEN:-}" ]]; then
        return 0
    fi

    log "Uploading $binary_name to custom release server..."

    curl -sf -X POST "${RELEASE_SERVER_URL}/upload" \
        -H "Authorization: Bearer ${RELEASE_UPLOAD_TOKEN}" \
        -H "X-Product: bsl-analyzer" \
        -H "X-Version: ${version}" \
        -H "X-Platform: ${binary_name}" \
        --data-binary "@${binary_path}" > /dev/null || {
        warn "Failed to upload to custom server (non-fatal)"
        return 0
    }

    log "Uploaded $binary_name to custom server"
}

upload_binaries() {
    local version=$1
    local tag=$2
    local target=$3
    local platform_suffix=$4

    local launcher_name="bsl-launcher-${platform_suffix}"
    local app_name="bsl-analyzer-app-${platform_suffix}"
    local launcher_path="target/$target/release/bsl-analyzer"
    local app_path="target/$target/release/bsl-analyzer-app"

    # Upload launcher
    local url
    url=$(upload_to_gitlab_registry "$version" "$launcher_name" "$launcher_path")
    add_release_link "$tag" "$launcher_name" "$url"
    upload_to_custom_server "$version" "$launcher_name" "$launcher_path"

    # Upload app
    url=$(upload_to_gitlab_registry "$version" "$app_name" "$app_path")
    add_release_link "$tag" "$app_name" "$url"
    upload_to_custom_server "$version" "$app_name" "$app_path"
}

build_and_upload() {
    local tag=$1
    local version="${tag#v}"

    check_env

    log "Building version $version (tag: $tag)"

    git checkout --quiet "$tag"

    build_target "x86_64-apple-darwin"
    build_target "aarch64-apple-darwin"

    upload_binaries "$version" "$tag" "x86_64-apple-darwin" "darwin-amd64"
    upload_binaries "$version" "$tag" "aarch64-apple-darwin" "darwin-arm64"

    # Publish to custom server if configured
    if [[ -n "${RELEASE_SERVER_URL:-}" ]] && [[ -n "${RELEASE_UPLOAD_TOKEN:-}" ]]; then
        log "Publishing version to custom release server..."
        curl -sf -X POST "${RELEASE_SERVER_URL}/publish/bsl-analyzer/${version}" \
            -H "Authorization: Bearer ${RELEASE_UPLOAD_TOKEN}" > /dev/null || {
            warn "Failed to publish to custom server (non-fatal)"
        }
    fi

    git checkout --quiet -

    save_built_tag "$tag"
    log "Done! Version $version published (4 macOS binaries: 2 launchers + 2 apps)."
}

watch_mode() {
    check_env
    log "Watching for new tags... (Ctrl+C to stop)"

    while true; do
        local latest_tag
        latest_tag=$(get_latest_tag)
        local last_built
        last_built=$(get_last_built_tag)

        if [[ -n "$latest_tag" && "$latest_tag" != "$last_built" ]]; then
            log "New tag found: $latest_tag"
            build_and_upload "$latest_tag"
        fi

        sleep 300
    done
}

case "${1:-}" in
    --watch|-w)
        watch_mode
        ;;
    --help|-h)
        echo "Usage: $0 [OPTIONS] [TAG]"
        echo ""
        echo "Builds both launcher and app for macOS (amd64 + arm64)."
        echo ""
        echo "Options:"
        echo "  --watch, -w    Watch for new tags and build automatically"
        echo "  --help, -h     Show this help"
        echo ""
        echo "Environment variables:"
        echo "  GITLAB_TOKEN        GitLab Personal Access Token (required)"
        echo "  GITLAB_PROJECT_ID   GitLab project ID (required)"
        echo "  GITLAB_URL          GitLab URL (default: https://gitlab.com)"
        echo "  RELEASE_SERVER_URL  Custom release server URL (optional)"
        echo "  RELEASE_UPLOAD_TOKEN Token for custom release server (optional)"
        echo ""
        echo "Examples:"
        echo "  $0              Build latest tag"
        echo "  $0 v0.1.0       Build specific tag"
        echo "  $0 --watch      Watch mode (runs every 5 min)"
        ;;
    v*)
        build_and_upload "$1"
        ;;
    "")
        latest=$(get_latest_tag)
        if [[ -z "$latest" ]]; then
            error "No tags found"
            exit 1
        fi
        build_and_upload "$latest"
        ;;
    *)
        error "Unknown option: $1"
        exit 1
        ;;
esac
