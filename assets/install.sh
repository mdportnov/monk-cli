#!/usr/bin/env bash
set -euo pipefail

REPO="mdportnov/monk-cli"
BIN="monk"
INSTALL_DIR="${MONK_INSTALL_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$*" >&2; exit 1; }

# Set by main(), removed by the EXIT trap. Must be a global: the trap fires
# after main()'s locals are gone, and `set -u` would abort on an unset one.
tmp=""
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

    local target version url
    target="$(detect_target)"
    version="${MONK_VERSION:-latest}"

    if [ "$version" = "latest" ]; then
        version="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
            | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)"
        [ -n "$version" ] || err "could not resolve latest version (github api rate limit? set MONK_VERSION=vX.Y.Z)"
    fi
    # Release tags and asset names carry the `v`; accept MONK_VERSION with or
    # without it instead of building a 404 URL.
    case "$version" in v*) ;; *) version="v${version}" ;; esac

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

    maybe_run_setup
}

next_step_hint() {
    case "$(uname -s)" in
        Darwin) msg "next: run \`$BIN setup\` — one shot; it asks for your admin password once" ;;
        Linux)  msg "next: run \`$BIN setup\` — installs a systemd *user* unit, no sudo needed" ;;
        *)      msg "next: run \`$BIN setup\`" ;;
    esac
}

# Finish the job: an installed binary alone blocks nothing until the service
# is wired up. `curl … | bash` leaves stdin on the pipe, so reopen the
# terminal — otherwise the wizard would take its non-interactive path and ask
# nothing. MONK_SETUP=1 forces it (unattended), MONK_SETUP=0 skips it.
maybe_run_setup() {
    local bin="$INSTALL_DIR/$BIN" reply=""
    case "${MONK_SETUP:-}" in
        0|no|false)
            next_step_hint
            return
            ;;
        1|yes|true)
            if has_tty; then
                "$bin" setup < /dev/tty || warn_setup
            else
                "$bin" setup --yes || warn_setup
            fi
            return
            ;;
    esac

    if ! has_tty; then
        next_step_hint
        return
    fi

    next_step_hint
    printf '==> run "%s setup" now? [Y/n] ' "$BIN" > /dev/tty
    read -r reply < /dev/tty || reply=""
    case "$reply" in
        [nN]*) return ;;
        *) "$bin" setup < /dev/tty || warn_setup ;;
    esac
}

# `[ -r /dev/tty ]` is not enough: inside a container the node exists but
# opening it fails with ENXIO. Try the open for real.
has_tty() {
    [ -e /dev/tty ] || return 1
    (exec 3<>/dev/tty) 2>/dev/null || return 1
    return 0
}

warn_setup() {
    printf 'warning: "%s setup" did not finish — run it again later\n' "$BIN" >&2
}

main "$@"
