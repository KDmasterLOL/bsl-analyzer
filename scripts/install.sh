#!/usr/bin/env bash
# Install script for bsl-analyzer LSP server
#
# Usage:
#   curl -fsSL <url>/install.sh | bash
#   curl -fsSL <url>/install.sh | bash -s -- --version 0.1.38
#   curl -fsSL <url>/install.sh | bash -s -- --install-dir /usr/local/bin
#
# Environment variables:
#   BSL_INSTALL_DIR - installation directory (default: ~/.local/bin on Linux, /usr/local/bin on macOS)
#   BSL_VERSION     - version to install (default: latest)

set -euo pipefail

# --- Source configuration (replaced by github-sync for GitHub builds) ---
# INSTALL_SOURCE:gitlab
INSTALL_SOURCE="gitlab"
GITLAB_RELEASE_URL="https://dev.runsystems.ru/releases"
GITLAB_PRODUCT="bsl-analyzer"
GITHUB_REPO="itrous/bsl-analyzer"
# --- End source configuration ---

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

verify_checksum() {
    local file="$1"
    local expected="$2"
    local actual

    if check_command sha256sum; then
        actual=$(sha256sum "$file" | cut -d' ' -f1)
    elif check_command shasum; then
        actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    else
        warn "sha256sum/shasum not found, skipping checksum verification"
        return 0
    fi

    if [ "$actual" != "$expected" ]; then
        error "Checksum mismatch!"
        error "  Expected: $expected"
        error "  Got:      $actual"
        return 1
    fi
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

    # Fetch latest version if not specified
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

    # Check if already installed
    local existing="${INSTALL_DIR}/${BINARY_NAME}"
    if [ -f "$existing" ]; then
        local current
        current=$("$existing" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")
        if [ "$current" = "$VERSION" ]; then
            ok "bsl-analyzer ${VERSION} is already installed"
            exit 0
        fi
        info "Upgrading from ${current} to ${VERSION}"
    fi

    # Build download URL
    local file_name="bsl-analyzer-app-${PLATFORM}"
    local url
    case "$INSTALL_SOURCE" in
        github) url=$(download_url_github "$VERSION" "$file_name") ;;
        gitlab) url=$(download_url_gitlab "$VERSION" "$file_name") ;;
    esac

    # Download
    local tmp_dir
    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    local tmp_file="${tmp_dir}/${file_name}"

    info "Downloading ${file_name}..."
    if ! download_file "$url" "$tmp_file"; then
        error "Failed to download from ${url}"
        exit 1
    fi

    # Verify checksum (if checksums available)
    local checksums_file="${tmp_dir}/checksums.txt"
    local checksums_url
    case "$INSTALL_SOURCE" in
        github)
            checksums_url=$(download_url_github "$VERSION" "checksums.txt")
            ;;
        gitlab)
            checksums_url=$(download_url_gitlab "$VERSION" "checksums.txt")
            ;;
    esac

    if download_file "$checksums_url" "$checksums_file" 2>/dev/null; then
        local expected_checksum
        expected_checksum=$(grep "$file_name" "$checksums_file" | awk '{print $1}')
        if [ -n "$expected_checksum" ]; then
            info "Verifying checksum..."
            verify_checksum "$tmp_file" "$expected_checksum"
            ok "Checksum verified"
        fi
    fi

    # Install
    chmod +x "$tmp_file"

    mkdir -p "$INSTALL_DIR"

    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmp_file" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        info "Elevated permissions required for ${INSTALL_DIR}"
        sudo mv "$tmp_file" "${INSTALL_DIR}/${BINARY_NAME}"
    fi

    ok "Installed bsl-analyzer ${VERSION} to ${INSTALL_DIR}/${BINARY_NAME}"

    # Check PATH
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

    # Verify installation
    if check_command "$BINARY_NAME"; then
        local installed_version
        installed_version=$("$BINARY_NAME" --version 2>/dev/null || echo "")
        if [ -n "$installed_version" ]; then
            ok "$installed_version"
        fi
    fi

    echo ""
}

main "$@"
