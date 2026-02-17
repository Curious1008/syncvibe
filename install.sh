#!/bin/sh
# SyncVibe installer
# Usage: curl -fsSL https://raw.githubusercontent.com/Curious1008/syncvibe/main/install.sh | sh
set -eu

REPO="Curious1008/syncvibe"
INSTALL_DIR="${SYNCVIBE_DIR:-$HOME/.local/bin}"
VERSION="${SYNCVIBE_VERSION:-}"
TMPDIR_BASE="${TMPDIR:-/tmp}"
TMP_DIR=""

cleanup() {
    if [ -n "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

err() {
    printf "\033[31merror:\033[0m %s\n" "$1" >&2
    exit 1
}

info() {
    printf "\033[34m::\033[0m %s\n" "$1"
}

ok() {
    printf "\033[32m::\033[0m %s\n" "$1"
}

# Detect OS and architecture
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Darwin)
            TARGET="darwin-universal"
            ;;
        Linux)
            case "$ARCH" in
                x86_64|amd64)
                    TARGET="x86_64-unknown-linux-gnu"
                    ;;
                aarch64|arm64)
                    TARGET="aarch64-unknown-linux-gnu"
                    ;;
                *)
                    err "Unsupported architecture: $ARCH"
                    ;;
            esac
            ;;
        *)
            err "Unsupported OS: $OS"
            ;;
    esac
}

# Get latest version from GitHub API
get_version() {
    if [ -n "$VERSION" ]; then
        return
    fi

    info "Fetching latest version..."

    RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"

    if command -v curl >/dev/null 2>&1; then
        RESPONSE=$(curl -fsSL "$RELEASE_URL") || err "Failed to fetch release info. Check your internet connection."
    elif command -v wget >/dev/null 2>&1; then
        RESPONSE=$(wget -qO- "$RELEASE_URL") || err "Failed to fetch release info. Check your internet connection."
    else
        err "curl or wget is required"
    fi

    # Parse tag_name without jq
    VERSION=$(printf '%s' "$RESPONSE" | grep '"tag_name"' | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')

    if [ -z "$VERSION" ]; then
        err "Could not determine latest version"
    fi
}

download() {
    FILENAME="syncvibe-${TARGET}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/${VERSION}/${FILENAME}"

    info "Downloading syncvibe ${VERSION} for ${TARGET}..."

    TMP_DIR=$(mktemp -d "${TMPDIR_BASE}/syncvibe-install.XXXXXX")
    trap cleanup EXIT

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$URL" -o "${TMP_DIR}/${FILENAME}" || err "Download failed. Is ${VERSION} a valid release?"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$URL" -O "${TMP_DIR}/${FILENAME}" || err "Download failed. Is ${VERSION} a valid release?"
    fi
}

install() {
    info "Installing to ${INSTALL_DIR}..."

    mkdir -p "$INSTALL_DIR"
    tar xzf "${TMP_DIR}/${FILENAME}" -C "$TMP_DIR"
    mv "${TMP_DIR}/syncvibe" "${INSTALL_DIR}/syncvibe"
    chmod +x "${INSTALL_DIR}/syncvibe"
}

check_path() {
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*)
            return
            ;;
    esac

    printf "\n"
    printf "\033[33mwarning:\033[0m %s is not in your PATH\n" "$INSTALL_DIR"
    printf "Add it by running:\n\n"

    SHELL_NAME="$(basename "${SHELL:-sh}")"
    case "$SHELL_NAME" in
        zsh)
            printf "  echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc && source ~/.zshrc\n" "$INSTALL_DIR"
            ;;
        bash)
            printf "  echo 'export PATH=\"%s:\$PATH\"' >> ~/.bashrc && source ~/.bashrc\n" "$INSTALL_DIR"
            ;;
        fish)
            printf "  fish_add_path %s\n" "$INSTALL_DIR"
            ;;
        *)
            printf "  export PATH=\"%s:\$PATH\"\n" "$INSTALL_DIR"
            ;;
    esac
    printf "\n"
}

verify() {
    if "${INSTALL_DIR}/syncvibe" --help >/dev/null 2>&1; then
        ok "syncvibe ${VERSION} installed successfully!"
    else
        err "Installation verification failed"
    fi
}

main() {
    detect_platform
    get_version
    download
    install
    check_path
    verify

    printf "\n"
    info "Get started:"
    printf "  syncvibe init     # Start a new room\n"
    printf "  syncvibe join     # Join with an invite code\n"
    printf "\n"
}

main
