#!/bin/sh
# SyncVibe installer
# Usage: curl -fsSL https://syncvibe.online/install.sh | sh
set -eu

REPO="Curious1008/syncvibe"
VERSION="${SYNCVIBE_VERSION:-}"
TMPDIR_BASE="${TMPDIR:-/tmp}"
TMP_DIR=""
HAS_TTY=false

if [ -e /dev/tty ]; then
    HAS_TTY=true
fi

# ── Color palette (matches SyncVibe TUI) ─────────────────────────

TEAL='\033[38;2;78;205;196m'
DIM_TEAL='\033[38;2;50;100;95m'
GREEN='\033[38;2;80;200;120m'
RED='\033[38;2;255;100;100m'
YELLOW='\033[38;2;255;214;102m'
DIM='\033[38;2;100;100;115m'
BRIGHT='\033[38;2;225;225;235m'
B='\033[1m'
R='\033[0m'

# ── Helpers ──────────────────────────────────────────────────────

cleanup() {
    if [ -n "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}

err() {
    printf "  ${RED}✗${R} %s\n" "$1" >&2
    exit 1
}

info() {
    printf "  ${TEAL}◆${R} %s\n" "$1"
}

ok() {
    printf "  ${GREEN}✓${R} %s\n" "$1"
}

dim() {
    printf "    ${DIM}%s${R}\n" "$1"
}

warn() {
    printf "  ${YELLOW}!${R} %s\n" "$1"
}

# If GITHUB_TOKEN is set and URL targets api.github.com, emit an auth header flag.
# Echoes a single arg suitable for eval (quoted). Empty if not applicable.
_auth_header() {
    case "$1" in
        https://api.github.com/*)
            if [ -n "${GITHUB_TOKEN:-}" ]; then
                printf '%s' "Authorization: Bearer ${GITHUB_TOKEN}"
            fi
            ;;
    esac
}

# Fetch a URL to stdout (curl preferred, wget fallback)
fetch() {
    hdr=$(_auth_header "$1")
    if command -v curl >/dev/null 2>&1; then
        if [ -n "$hdr" ]; then curl -fsSL -H "$hdr" "$1"; else curl -fsSL "$1"; fi
    elif command -v wget >/dev/null 2>&1; then
        if [ -n "$hdr" ]; then wget -qO- --header="$hdr" "$1"; else wget -qO- "$1"; fi
    else
        err "curl or wget is required"
    fi
}

# Fetch a URL to a file
fetch_to() {
    hdr=$(_auth_header "$1")
    if command -v curl >/dev/null 2>&1; then
        if [ -n "$hdr" ]; then curl -fsSL -H "$hdr" "$1" -o "$2"; else curl -fsSL "$1" -o "$2"; fi
    elif command -v wget >/dev/null 2>&1; then
        if [ -n "$hdr" ]; then wget -q --header="$hdr" "$1" -O "$2"; else wget -q "$1" -O "$2"; fi
    fi
}

# 1/2 confirmation prompt (matches SyncVibe onboarding UI)
# Uses single-keypress (stty raw) when possible, falls back to line read.
confirm() {
    if [ "$HAS_TTY" = false ]; then
        return 0
    fi

    printf "\n  %s\n\n" "$1"
    printf "  ${TEAL}1${R} Yes\n"
    printf "  ${DIM}2${R} No\n\n"

    # Try single-keypress mode via stty raw + dd
    if saved=$(stty -g </dev/tty 2>/dev/null) && [ -n "$saved" ]; then
        printf "  ${DIM}Press 1 or 2:${R} "
        while true; do
            stty raw -echo </dev/tty 2>/dev/null
            key=$(dd bs=1 count=1 </dev/tty 2>/dev/null)
            stty "$saved" </dev/tty 2>/dev/null
            case "$key" in
                1|y|Y) printf "${GREEN}Yes${R}\n"; return 0 ;;
                2|n|N) printf "${DIM}No${R}\n"; return 1 ;;
                *) ;;
            esac
        done
    else
        # Fallback: line-based input (type 1 or 2 then Enter)
        printf "  ${DIM}Enter 1 or 2:${R} "
        while true; do
            read -r key </dev/tty 2>/dev/null || return 1
            case "$key" in
                1|y|Y) printf "\033[A  ${DIM}Enter 1 or 2:${R} ${GREEN}Yes${R}\n"; return 0 ;;
                2|n|N) printf "\033[A  ${DIM}Enter 1 or 2:${R} ${DIM}No${R}\n"; return 1 ;;
                *) printf "  ${DIM}Enter 1 or 2:${R} " ;;
            esac
        done
    fi
}

# Like confirm() but requires interactive TTY — never auto-approves.
# Used for operations that run sudo.
confirm_interactive() {
    if [ "$HAS_TTY" = false ]; then
        warn "Skipping (non-interactive environment)"
        return 1
    fi
    confirm "$1"
}

# ── Banner ───────────────────────────────────────────────────────

# Print one shimmer frame: _sf <title content with escape codes>
_sf() { printf "\r  ${DIM_TEAL}│${R}            %b             ${DIM_TEAL}│${R}" "$1"; sleep 0.035; }

print_banner() {
    WH='\033[38;2;255;255;255m'
    HR="────────────────────────────────────────"
    BL="                                        "

    printf "\n"
    printf "  ${DIM_TEAL}╭${HR}╮${R}\n"
    printf "  ${DIM_TEAL}│${R}${BL}${DIM_TEAL}│${R}\n"

    # Shimmer: dim → left-to-right white sweep → teal
    printf "  ${DIM_TEAL}│${R}            ${DIM}S y n c V i b e${R}             ${DIM_TEAL}│${R}"
    sleep 0.15
    _sf "${WH}${B}S${R}${DIM} y n c V i b e${R}"
    _sf "${TEAL}${B}S ${R}${WH}${B}y${R}${DIM} n c V i b e${R}"
    _sf "${TEAL}${B}S y ${R}${WH}${B}n${R}${DIM} c V i b e${R}"
    _sf "${TEAL}${B}S y n ${R}${WH}${B}c${R}${DIM} V i b e${R}"
    _sf "${TEAL}${B}S y n c ${R}${WH}${B}V${R}${DIM} i b e${R}"
    _sf "${TEAL}${B}S y n c V ${R}${WH}${B}i${R}${DIM} b e${R}"
    _sf "${TEAL}${B}S y n c V i ${R}${WH}${B}b${R}${DIM} e${R}"
    _sf "${TEAL}${B}S y n c V i b ${R}${WH}${B}e${R}"
    printf "\r  ${DIM_TEAL}│${R}            ${TEAL}${B}S y n c V i b e${R}             ${DIM_TEAL}│${R}\n"

    printf "  ${DIM_TEAL}│${R}       ${DIM}collaborate in the terminal${R}      ${DIM_TEAL}│${R}\n"
    printf "  ${DIM_TEAL}│${R}${BL}${DIM_TEAL}│${R}\n"
    printf "  ${DIM_TEAL}╰${HR}╯${R}\n"
    printf "\n"
}

# ── Smart path selection ─────────────────────────────────────────

detect_install_dir() {
    # User override via env var
    if [ -n "${SYNCVIBE_DIR:-}" ]; then
        INSTALL_DIR="$SYNCVIBE_DIR"
        return
    fi

    # Prefer /usr/local/bin (common, usually in PATH)
    if [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
        INSTALL_DIR="/usr/local/bin"
        return
    fi

    # Fallback to ~/.local/bin
    INSTALL_DIR="$HOME/.local/bin"
}

# ── Platform detection ───────────────────────────────────────────

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Darwin) TARGET="darwin-universal" ;;
        Linux)
            case "$ARCH" in
                x86_64|amd64)   TARGET="x86_64-unknown-linux-gnu" ;;
                aarch64|arm64)  TARGET="aarch64-unknown-linux-gnu" ;;
                *)              err "Unsupported architecture: $ARCH" ;;
            esac
            ;;
        *) err "Unsupported OS: $OS" ;;
    esac
}

# ── Version & download ───────────────────────────────────────────

get_version() {
    if [ -n "$VERSION" ]; then
        return
    fi

    info "Fetching latest version..."

    RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"
    RESPONSE=$(fetch "$RELEASE_URL") || err "Failed to fetch release info. Check your internet connection."

    VERSION=$(printf '%s' "$RESPONSE" | grep '"tag_name"' | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')

    if [ -z "$VERSION" ]; then
        err "Could not determine latest version"
    fi

    # Sanitize: version must match vN.N.N pattern
    case "$VERSION" in
        v[0-9]*.[0-9]*.[0-9]*) ;;
        *) err "Invalid version format: $VERSION" ;;
    esac
}

download() {
    FILENAME="syncvibe-${TARGET}.tar.gz"
    URL="https://github.com/${REPO}/releases/download/${VERSION}/${FILENAME}"
    CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

    info "Downloading syncvibe ${VERSION} for ${TARGET}..."

    TMP_DIR=$(mktemp -d "${TMPDIR_BASE}/syncvibe-install.XXXXXX")
    trap cleanup EXIT

    fetch_to "$URL" "${TMP_DIR}/${FILENAME}" || err "Download failed. Is ${VERSION} a valid release?"

    # Verify SHA256 checksum — hard fail if we can't actually verify.
    # The only way to bypass is SYNCVIBE_SKIP_CHECKSUM=1 (explicit, logged).
    fetch_to "$CHECKSUM_URL" "${TMP_DIR}/checksums.txt" 2>/dev/null \
        || err "Could not fetch checksums.txt from ${CHECKSUM_URL}. Refusing to install unverified binary. Set SYNCVIBE_SKIP_CHECKSUM=1 to override (not recommended)."
    EXPECTED=$(grep "${FILENAME}" "${TMP_DIR}/checksums.txt" | cut -d ' ' -f1)
    [ -n "$EXPECTED" ] \
        || err "No checksum entry for ${FILENAME} in checksums.txt. Refusing to install unverified binary."
    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL=$(sha256sum "${TMP_DIR}/${FILENAME}" | cut -d ' ' -f1)
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL=$(shasum -a 256 "${TMP_DIR}/${FILENAME}" | cut -d ' ' -f1)
    elif [ "${SYNCVIBE_SKIP_CHECKSUM:-0}" = "1" ]; then
        warn "SYNCVIBE_SKIP_CHECKSUM=1 set — installing without checksum verification"
        ACTUAL="$EXPECTED"
    else
        err "Neither sha256sum nor shasum is available. Install one, or set SYNCVIBE_SKIP_CHECKSUM=1 to override (not recommended)."
    fi
    if [ "$ACTUAL" != "$EXPECTED" ]; then
        err "Checksum mismatch! Expected ${EXPECTED}, got ${ACTUAL}. Download may be corrupted or tampered."
    fi
    ok "Checksum verified"
}

# ── Install binary ───────────────────────────────────────────────

install_binary() {
    info "Installing to ${INSTALL_DIR}..."

    mkdir -p "$INSTALL_DIR"
    tar xzf "${TMP_DIR}/${FILENAME}" --no-same-owner -C "$TMP_DIR"

    # Remove old binary first (macOS caches code signature rejections)
    rm -f "${INSTALL_DIR}/syncvibe"
    mv "${TMP_DIR}/syncvibe" "${INSTALL_DIR}/syncvibe"
    chmod +x "${INSTALL_DIR}/syncvibe"
}

# ── PATH check ───────────────────────────────────────────────────

check_path() {
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) return ;;
    esac

    printf "\n"
    warn "${INSTALL_DIR} is not in your PATH"
    dim "Add it by running:"
    printf "\n"

    SHELL_NAME="$(basename "${SHELL:-sh}")"
    case "$SHELL_NAME" in
        zsh)  dim "echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.zshrc && source ~/.zshrc" ;;
        bash) dim "echo 'export PATH=\"${INSTALL_DIR}:\$PATH\"' >> ~/.bashrc && source ~/.bashrc" ;;
        fish) dim "fish_add_path ${INSTALL_DIR}" ;;
        *)    dim "export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
    esac
    printf "\n"
}

# ── Verify installation ─────────────────────────────────────────

verify() {
    if "${INSTALL_DIR}/syncvibe" --help >/dev/null 2>&1; then
        ok "syncvibe ${VERSION} installed successfully!"
    else
        err "Installation verification failed"
    fi
}

# ── tmux dependency ──────────────────────────────────────────────

check_tmux() {
    if command -v tmux >/dev/null 2>&1; then
        return
    fi

    printf "\n"
    warn "tmux is not installed"
    dim "SyncVibe uses tmux for split-terminal collaboration"
    dim "with AI agents. Install tmux for the complete experience."

    # Use confirm_interactive — never auto-approve sudo in non-interactive mode
    if confirm_interactive "Install tmux?"; then
        install_tmux
    else
        printf "\n"
        dim "You can install tmux later for the full experience."
    fi
}

install_tmux() {
    printf "\n"

    if command -v brew >/dev/null 2>&1; then
        info "Installing tmux via Homebrew..."
        brew install tmux
    elif command -v apt-get >/dev/null 2>&1; then
        info "Installing tmux via apt..."
        sudo apt-get install -y tmux
    elif command -v dnf >/dev/null 2>&1; then
        info "Installing tmux via dnf..."
        sudo dnf install -y tmux
    elif command -v pacman >/dev/null 2>&1; then
        info "Installing tmux via pacman..."
        sudo pacman -S --noconfirm tmux
    elif command -v apk >/dev/null 2>&1; then
        info "Installing tmux via apk..."
        sudo apk add tmux
    else
        warn "Could not detect package manager"
        dim "Please install tmux manually."
        return
    fi

    if command -v tmux >/dev/null 2>&1; then
        ok "tmux installed successfully!"
    else
        warn "tmux installation may have failed"
        dim "Please install tmux manually."
    fi
}

# ── Getting started ──────────────────────────────────────────────

print_getting_started() {
    printf "\n"
    printf "  ${TEAL}◆${R} ${TEAL}${B}Get started${R}\n"
    printf "  ${DIM_TEAL}──────────────────────────────────────${R}\n"
    printf "\n"
    printf "    ${BRIGHT}syncvibe${R}            ${DIM}Launch SyncVibe${R}\n"
    printf "    ${BRIGHT}syncvibe init${R}       ${DIM}Start a new room${R}\n"
    printf "    ${BRIGHT}syncvibe join${R}       ${DIM}Join with invite code${R}\n"
    printf "\n"
}

# ── Main ─────────────────────────────────────────────────────────

main() {
    print_banner
    detect_platform
    detect_install_dir
    get_version
    download
    install_binary
    check_path
    verify
    check_tmux

    # Launch SyncVibe immediately after install (skip in CI / non-interactive)
    if [ "${SYNCVIBE_NO_LAUNCH:-0}" = "1" ] || [ -n "${CI:-}" ] || [ ! -t 0 ]; then
        printf "\n"
        ok "Installed. Run 'syncvibe' to start."
        return
    fi
    printf "\n"
    info "Launching SyncVibe...\n"
    exec "${INSTALL_DIR}/syncvibe"
}

main
