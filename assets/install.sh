#!/usr/bin/env bash
set -euo pipefail

REPO="mdportnov/monk-cli"
BIN="monk"
INSTALL_DIR="${MONK_INSTALL_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }
msg() { printf '==> %s\n' "$*"; }

detect_target() {
    local uname_s uname_m
    uname_s="$(uname -s)"
    uname_m="$(uname -m)"
    case "$uname_s" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *) err "unsupported OS: $uname_s" ;;
    esac
    case "$uname_m" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *) err "unsupported arch: $uname_m" ;;
    esac
    echo "${arch}-${os}"
}

main() {
    command -v curl >/dev/null || err "curl is required"
    command -v tar  >/dev/null || err "tar is required"

    local target version url tmp
    target="$(detect_target)"
    version="${MONK_VERSION:-latest}"

    if [ "$version" = "latest" ]; then
        version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)"
        [ -n "$version" ] || err "could not resolve latest version"
    fi

    url="https://github.com/${REPO}/releases/download/${version}/${BIN}-${version}-${target}.tar.gz"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT

    local archive="${BIN}-${version}-${target}.tar.gz"
    local sums_url="https://github.com/${REPO}/releases/download/${version}/SHA256SUMS.txt"

    msg "downloading $url"
    curl -fsSL "$url" -o "$tmp/$archive"
    curl -fsSL "$sums_url" -o "$tmp/SHA256SUMS.txt"

    msg "verifying checksum"
    (cd "$tmp" && grep -F "$archive" SHA256SUMS.txt | sha256sum -c --quiet 2>/dev/null \
        || grep -F "$archive" SHA256SUMS.txt | shasum -a 256 -c --quiet) \
        || err "checksum verification failed"

    tar -xzf "$tmp/$archive" -C "$tmp"

    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$tmp/${BIN}-${version}-${target}/${BIN}" "$INSTALL_DIR/${BIN}"

    if [ "$(uname -s)" = "Darwin" ]; then
        if command -v xattr >/dev/null; then
            xattr -dr com.apple.quarantine "$INSTALL_DIR/${BIN}" 2>/dev/null || true
        fi
    fi

    msg "installed $BIN $version to $INSTALL_DIR/$BIN"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            msg "add $INSTALL_DIR to your PATH"
            shell_name="$(basename "${SHELL:-}")"
            case "$shell_name" in
                zsh)  msg "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.zshrc && source ~/.zshrc" ;;
                bash) msg "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.bashrc && source ~/.bashrc" ;;
                fish) msg "  fish_add_path $INSTALL_DIR" ;;
                *)    msg "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
            esac
            ;;
    esac

    if [ "$(uname -s)" = "Darwin" ]; then
        msg "next: run \`$BIN setup\` for a one-shot install (will prompt for admin password once)"
    fi
}

main "$@"
