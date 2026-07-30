#!/usr/bin/env bash

set -euo pipefail

INSTALL_SOURCE="gitlab"
GITLAB_RELEASE_URL="https://dev.runsystems.ru/releases"
GITLAB_PRODUCT="bsl-analyzer"
GITHUB_REPO="itrous/bsl-analyzer"

VERSION="${BSL_VERSION:-}"
INSTALL_DIR="${BSL_INSTALL_DIR:-}"
BINARY_NAME="bsl-analyzer"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

detect_platform() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  OS="linux" ;;
        Darwin) OS="darwin" ;;
        *)      error "Unsupported OS: $os"; exit 1 ;;
    esac

    case "$arch" in
        x86_64)       ARCH="amd64" ;;
        aarch64|arm64) ARCH="arm64" ;;
        *)            error "Unsupported architecture: $arch"; exit 1 ;;
    esac

    PLATFORM="${OS}-${ARCH}"
}

default_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        return
    fi

    case "$OS" in
        linux)  INSTALL_DIR="$HOME/.local/bin" ;;
        darwin) INSTALL_DIR="/usr/local/bin" ;;
    esac
}

check_command() {
    command -v "$1" &>/dev/null
}

fetch_latest_version_github() {
    local url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
    local response

    if check_command curl; then
        response=$(curl -fsSL "$url")
    elif check_command wget; then
        response=$(wget -qO- "$url")
    else
        error "curl or wget is required"
        exit 1
    fi

    echo "$response" | grep '"tag_name"' | sed 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/'
}

fetch_latest_version_gitlab() {
    local url="${GITLAB_RELEASE_URL}/${GITLAB_PRODUCT}/latest"

    if check_command curl; then
        curl -fsSL "$url" | tr -d '[:space:]'
    elif check_command wget; then
        wget -qO- "$url" | tr -d '[:space:]'
    else
        error "curl or wget is required"
        exit 1
    fi
}

download_url_github() {
    local version="$1"
    local file_name="$2"
    echo "https://github.com/${GITHUB_REPO}/releases/download/v${version}/${file_name}"
}

download_url_gitlab() {
    local version="$1"
    local file_name="$2"
    echo "${GITLAB_RELEASE_URL}/${GITLAB_PRODUCT}/${version}/${file_name}"
}

download_file() {
    local url="$1"
    local dest="$2"

    if check_command curl; then
        curl -fsSL -o "$dest" "$url"
    elif check_command wget; then
        wget -qO "$dest" "$url"
    fi
}

file_sha256() {
    local file="$1"

    if check_command sha256sum; then
        sha256sum "$file" | cut -d' ' -f1 | tr '[:upper:]' '[:lower:]'
    elif check_command shasum; then
        shasum -a 256 "$file" | cut -d' ' -f1 | tr '[:upper:]' '[:lower:]'
    fi
}

# sha256 of one asset out of the release manifest the release server publishes.
# Whitespace is stripped first so the value survives both compact and pretty JSON.
manifest_sha256() {
    local manifest="$1"
    local asset="$2"
    local asset_re

    asset_re=$(printf '%s' "$asset" | sed 's/[.[\*^$+?()|]/\\&/g')

    tr -d ' \n\r\t' < "$manifest" \
        | grep -oE "\"${asset_re}\":\{[^}]*\}" \
        | grep -oE '"sha256":"[0-9a-fA-F]{64}"' \
        | head -n1 \
        | cut -d'"' -f4 \
        | tr '[:upper:]' '[:lower:]'
}

# GitHub releases carry no manifest; checksums.txt is what the launcher reads there.
checksums_sha256() {
    local checksums="$1"
    local asset="$2"

    awk -v name="$asset" '{ sub(/^\*/, "", $2); if ($2 == name) { print tolower($1); exit } }' "$checksums"
}

# Expected checksum for the asset, from whichever source this installer is built for.
# Empty output means the release does not let the download be verified.
expected_checksum() {
    local version="$1"
    local asset="$2"

    case "$INSTALL_SOURCE" in
        gitlab)
            local manifest="${TMP_DIR}/manifest.json"
            if download_file "$(download_url_gitlab "$version" "manifest.json")" "$manifest"; then
                manifest_sha256 "$manifest" "$asset"
            fi
            ;;
        github)
            local checksums="${TMP_DIR}/checksums.txt"
            if download_file "$(download_url_github "$version" "checksums.txt")" "$checksums"; then
                checksums_sha256 "$checksums" "$asset"
            fi
            ;;
    esac
}

verify_checksum() {
    local file="$1"
    local expected="$2"
    local actual

    actual=$(file_sha256 "$file")

    if [ -z "$actual" ]; then
        error "sha256sum or shasum is required to verify the download"
        return 1
    fi

    if [ "$actual" != "$expected" ]; then
        error "Checksum mismatch!"
        error "  Expected: $expected"
        error "  Got:      $actual"
        return 1
    fi
}

check_path() {
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*)
            ;;
        *)
            echo ""
            warn "${INSTALL_DIR} is not in your PATH"
            echo ""
            echo "  Add it to your shell profile:"
            echo ""
            if [ "$OS" = "linux" ]; then
                echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc"
                echo "    # or for zsh:"
                echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc"
            else
                echo "    echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc"
            fi
            echo ""
            ;;
    esac
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --version|-v)
                VERSION="$2"
                shift 2
                ;;
            --install-dir|-d)
                INSTALL_DIR="$2"
                shift 2
                ;;
            --help|-h)
                echo "Install bsl-analyzer LSP server"
                echo ""
                echo "Usage: install.sh [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  --version, -v VERSION    Version to install (default: latest)"
                echo "  --install-dir, -d DIR    Installation directory"
                echo "  --help, -h               Show this help"
                echo ""
                echo "Environment variables:"
                echo "  BSL_INSTALL_DIR          Installation directory"
                echo "  BSL_VERSION              Version to install"
                exit 0
                ;;
            *)
                error "Unknown option: $1"
                exit 1
                ;;
        esac
    done
}

main() {
    parse_args "$@"

    echo ""
    echo "  bsl-analyzer installer"
    echo ""

    detect_platform
    default_install_dir

    info "Platform: ${PLATFORM}"
    info "Source: ${INSTALL_SOURCE}"
    info "Install dir: ${INSTALL_DIR}"

    if [ -z "$VERSION" ]; then
        info "Fetching latest version..."
        case "$INSTALL_SOURCE" in
            github) VERSION=$(fetch_latest_version_github) ;;
            gitlab) VERSION=$(fetch_latest_version_gitlab) ;;
        esac

        if [ -z "$VERSION" ]; then
            error "Failed to determine latest version"
            exit 1
        fi
    fi

    info "Version: ${VERSION}"

    # The launcher, not the app: it keeps the working binary up to date by itself, and
    # docs/mcp/SETUP.md makes it the single entry point that belongs on PATH.
    local file_name="bsl-analyzer-${PLATFORM}"

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    local expected
    expected=$(expected_checksum "$VERSION" "$file_name")
    if [ -z "$expected" ]; then
        error "Release ${VERSION} publishes no checksum for ${file_name}, refusing to install unverified"
        exit 1
    fi

    # Identity by content, not by version number: the release source is compiled into the
    # launcher, so a GitHub and a release-server build share a version yet differ, and
    # skipping on the number alone would silently keep the other source's binary.
    local existing="${INSTALL_DIR}/${BINARY_NAME}"
    if [ -f "$existing" ] && [ "$(file_sha256 "$existing")" = "$expected" ]; then
        ok "bsl-analyzer ${VERSION} is already installed"
        check_path
        exit 0
    fi

    if [ -f "$existing" ]; then
        local current
        # --launcher-version answers from the launcher itself; plain --version would make
        # it fetch the whole app binary just to report a number.
        # The whole trailing token, so a prerelease suffix survives: matching only three
        # numeric parts would read 0.3.0-beta.1 as 0.3.0 and reinstall it on every run.
        current=$("$existing" --launcher-version 2>/dev/null | awk 'NR==1{print $NF}' | grep -E '^[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")
        info "Upgrading from ${current} to ${VERSION}"
    fi

    local url
    case "$INSTALL_SOURCE" in
        github) url=$(download_url_github "$VERSION" "$file_name") ;;
        gitlab) url=$(download_url_gitlab "$VERSION" "$file_name") ;;
    esac

    local tmp_file="${TMP_DIR}/${file_name}"

    info "Downloading ${file_name}..."
    if ! download_file "$url" "$tmp_file"; then
        error "Failed to download from ${url}"
        exit 1
    fi

    info "Verifying checksum..."
    verify_checksum "$tmp_file" "$expected"
    ok "Checksum verified"

    chmod +x "$tmp_file"

    mkdir -p "$INSTALL_DIR"

    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmp_file" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        info "Elevated permissions required for ${INSTALL_DIR}"
        sudo mv "$tmp_file" "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    ok "Installed bsl-analyzer ${VERSION} to ${INSTALL_DIR}/${BINARY_NAME}"

    check_path

    # Report the binary that was just installed: another bsl-analyzer earlier in PATH
    # would otherwise make the installer confirm a version it did not put there.
    local installed_version
    installed_version=$("${INSTALL_DIR}/${BINARY_NAME}" --launcher-version 2>/dev/null || echo "")
    if [ -n "$installed_version" ]; then
        ok "$installed_version"
    fi

    info "The analyzer itself is fetched on first run; force it now with: ${BINARY_NAME} --launcher-update"

    echo ""
}

main "$@"
