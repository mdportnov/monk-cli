#!/usr/bin/env bash
# Build monk from source, put it on your PATH, and wire up privileges.
# Linux / macOS. Run from a clone of the repo:  ./assets/setup.sh
set -euo pipefail

BIN="monk"

# ----- pretty output ---------------------------------------------------------
if [ -t 1 ]; then
    BOLD=$'\033[1m'; DIM=$'\033[2m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'
    BLUE=$'\033[34m'; RED=$'\033[31m'; RESET=$'\033[0m'
else
    BOLD=; DIM=; GREEN=; YELLOW=; BLUE=; RED=; RESET=
fi
STEP=0; TOTAL=4
step() { STEP=$((STEP + 1)); printf '\n%s[%d/%d]%s %s%s%s\n' "$BLUE" "$STEP" "$TOTAL" "$RESET" "$BOLD" "$*" "$RESET"; }
ok()   { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
info() { printf '  %s%s%s\n' "$DIM" "$*" "$RESET"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*"; }
err()  { printf '\n%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

ask() {
    # ask "prompt" -> returns 0 for yes. Defaults to yes; non-interactive = yes.
    local reply
    [ -t 0 ] || return 0
    printf '  %s? %s [Y/n] %s' "$YELLOW" "$1" "$RESET"
    read -r reply || return 0
    case "$reply" in [nN]*) return 1 ;; *) return 0 ;; esac
}

# ----- locate repo root ------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
[ -f "$REPO_ROOT/Cargo.toml" ] || err "run this from a clone of monk-cli (Cargo.toml not found)"
cd "$REPO_ROOT"

printf '%s%s monk — build from source %s\n' "$BOLD" "$BLUE" "$RESET"
info "$REPO_ROOT"

# ----- 1. Rust toolchain -----------------------------------------------------
step "Checking the Rust toolchain"
if command -v cargo >/dev/null 2>&1; then
    ok "cargo $(cargo --version | awk '{print $2}') found"
else
    warn "Rust toolchain not found."
    if ask "Install it now via rustup?"; then
        info "downloading rustup..."
        curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y
        # shellcheck source=/dev/null
        . "$HOME/.cargo/env"
        ok "Rust installed"
    else
        err "Rust 1.82+ is required — see https://rustup.rs"
    fi
fi

# ----- 2. Build & install ----------------------------------------------------
# `cargo install` compiles the release binary and copies it into ~/.cargo/bin,
# so there is no separate `cargo build` step (that would just compile twice).
step "Building and installing $BIN (this can take a few minutes)"
cargo install --path . --force
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
INSTALLED="$CARGO_BIN/$BIN"
ok "installed $INSTALLED"

# ----- 3. Ensure it is on PATH -----------------------------------------------
step "Making sure $CARGO_BIN is on your PATH"

if [ "$(uname -s)" = "Darwin" ] && command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "$INSTALLED" 2>/dev/null || true
fi

case ":$PATH:" in
    *":$CARGO_BIN:"*) ok "$CARGO_BIN is already on your PATH" ;;
    *)
        warn "$CARGO_BIN is not on your PATH"
        line="export PATH=\"$CARGO_BIN:\$PATH\""
        case "$(basename "${SHELL:-}")" in
            zsh)  rc="$HOME/.zshrc" ;;
            bash) rc="$HOME/.bashrc" ;;
            fish) rc="" ;;
            *)    rc="" ;;
        esac
        if [ "$(basename "${SHELL:-}")" = "fish" ]; then
            if ask "Run 'fish_add_path $CARGO_BIN'?"; then
                fish -c "fish_add_path $CARGO_BIN" && ok "PATH updated for fish"
            fi
        elif [ -n "$rc" ] && ask "Append it to $rc?"; then
            printf '\n# added by monk setup\n%s\n' "$line" >> "$rc"
            ok "added to $rc — open a new shell or run: source $rc"
        else
            info "add this to your shell profile yourself:"
            info "  $line"
        fi
        ;;
esac

# ----- 4. Privileges / service ----------------------------------------------
step "Wiring up privileges (monk setup)"
case "$(uname -s)" in
    Darwin) info "macOS: installs a root daemon that owns /etc/hosts — expect one password prompt." ;;
    Linux)  info "Linux: installs a systemd *user* unit (no sudo) and checks /etc/hosts access." ;;
esac
if ask "Run '$BIN setup' now?"; then
    "$INSTALLED" setup || warn "'$BIN setup' did not finish — run it later with: $BIN setup"
else
    info "skipped — run it later with: $BIN setup"
fi

printf '\n%s%s✓ Done.%s Try: %smonk --help%s\n' "$BOLD" "$GREEN" "$RESET" "$BOLD" "$RESET"
