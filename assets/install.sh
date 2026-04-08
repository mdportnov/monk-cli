#!/usr/bin/env bash
set -euo pipefail

REPO="mihail/monk"
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

    msg "downloading $url"
    curl -fsSL "$url" -o "$tmp/monk.tar.gz"
    tar -xzf "$tmp/monk.tar.gz" -C "$tmp"

    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$tmp/${BIN}" "$INSTALL_DIR/${BIN}"

    msg "installed $BIN $version to $INSTALL_DIR/$BIN"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) msg "add $INSTALL_DIR to your PATH" ;;
    esac
}

main "$@"
