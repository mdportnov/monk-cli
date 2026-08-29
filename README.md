<p align="center">
  <img src="assets/logo.svg" width="116" alt="monk logo" />
</p>

<h1 align="center">monk</h1>

<p align="center">
  <b>focus, weaponized.</b><br/>
  A cross-platform CLI focus & distraction blocker built in Rust.<br/>
  One binary, one daemon, zero nonsense – block apps and websites,<br/>
  commit to hard-mode sessions, and get your attention back.
</p>

<p align="center">
  <a href="https://github.com/mdportnov/monk-cli/actions/workflows/ci.yml"><img src="https://github.com/mdportnov/monk-cli/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/mdportnov/monk-cli/actions/workflows/release.yml"><img src="https://github.com/mdportnov/monk-cli/actions/workflows/release.yml/badge.svg" alt="Release" /></a>
  <img src="https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-blue.svg" alt="License: MIT OR Apache-2.0" />
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-5a5a5a" alt="Platforms" />
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-1.82%2B-DEA584?logo=rust&logoColor=black" alt="Rust" />
  <img src="https://img.shields.io/badge/TUI-ratatui-7aa2f7" alt="ratatui" />
  <img src="https://img.shields.io/badge/async-tokio-orange" alt="tokio" />
  <img src="https://img.shields.io/badge/stats-SQLite-044a64?logo=sqlite&logoColor=white" alt="SQLite" />
  <img src="https://img.shields.io/badge/unsafe-forbidden-success" alt="deny(unsafe_code)" />
  <img src="https://img.shields.io/badge/i18n-EN%20%2F%20RU-bb9af7" alt="EN/RU" />
</p>

<p align="center">
  🇷🇺 Русская версия: <a href="./README.ru.md">README.ru.md</a>
</p>

---

## Highlights

- **Real app blocking** — scans installed applications on macOS, Linux (native, Flatpak and Snap) and Windows, so you pick from a curated list instead of guessing process names.
- **Curated site presets** — bundled `global` and `ru` site groups (social, video, news, chat, shopping, games) with subdomain expansion baked in.
- **Hard mode** — tamper-evident session lock signed with BLAKE3 keyed HMAC. No `monk stop`, no config edits, no daemon kill can shortcut it.
- **Background daemon** — Unix domain / local socket IPC, fail-closed reconciliation loop, SIGTERM-safe cleanup, systemd / launchd / Windows Task Scheduler install.
- **Interactive TUI** — ratatui-powered dashboard for sessions, live stats, profile editing, type-to-filter mode search, daily-cap progress, and a high-contrast hard-mode badge.
- **Menu bar app (macOS)** — native status item with the session countdown and one-click mode start/stop; set up automatically by the wizard, or via `monk menubar install`.
- **Localized** — English and Русский out of the box via `rust-i18n`.
- **Zero unsafe** — `#![deny(unsafe_code)]` in the main crate.

## How it works

monk runs a small always-on daemon — the same `monk` binary launched as `monk daemon run`, registered with your OS service manager under the name `monkd`. It owns the block state; the CLI and TUI talk to it over a local socket. When you start a session:

1. The requested profile is resolved to a concrete set of hosts + installed apps.
2. Hosts are injected into the system `hosts` file (atomic write, signed block).
3. Matching processes are killed on a tick loop and kept down for the session.
4. In hard mode, a signed session lock is written to disk and verified on every tick — corrupting or deleting it keeps the block active.

## Tech stack

| Layer          | Crate / tech                                                        |
| -------------- | ------------------------------------------------------------------- |
| CLI            | `clap` v4 derive, `clap_complete`, `inquire` for interactive prompts |
| TUI            | `ratatui`, `crossterm`, `tui-big-text`, `tachyonfx`                 |
| Async runtime  | `tokio` multi-thread, `tokio-util`, `futures`                       |
| IPC            | `interprocess` local sockets (Unix domain / Windows named pipe)     |
| Persistence    | `toml` config, `rusqlite` (bundled) for stats, atomic `fs-err` writes |
| Integrity      | `blake3` keyed HMAC, canonical binary serializer, `machine-uid`     |
| Process model  | `sysinfo`, `nix` signals on Unix, `windows` crate on Windows        |
| App discovery  | `plist` (macOS bundles), freedesktop `.desktop` parser (Linux), `lnk` (Windows) |
| Observability  | `tracing`, `tracing-subscriber`, `tracing-appender`                 |
| i18n           | `rust-i18n`, `sys-locale`                                           |
| Errors         | `thiserror` + `miette` fancy reports                                |

## Installation

### Quick install (script)

Downloads the matching release binary, verifies its checksum, and drops it on
your `PATH`. Then run `monk setup`.

```sh
# Linux / macOS  → installs to ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/mdportnov/monk-cli/master/assets/install.sh | bash
```

```powershell
# Windows (PowerShell 5+)  → installs to %LOCALAPPDATA%\monk\bin
irm https://raw.githubusercontent.com/mdportnov/monk-cli/master/assets/install.ps1 | iex
```

### From source

Needs the Rust toolchain (1.82+) — install via [rustup](https://rustup.rs). Works the same on Linux, macOS, and Windows.

```sh
git clone https://github.com/mdportnov/monk-cli
cd monk-cli

# Option A — build the release binary and put it on your PATH (recommended)
cargo install --path .     # → ~/.cargo/bin/monk  (%USERPROFILE%\.cargo\bin\monk.exe on Windows)

# Option B — build the release binary in-tree, run it by path
cargo build --release      # → target/release/monk  (target\release\monk.exe on Windows)
```

A plain `cargo build` (no `--release`) produces a slower debug binary at `target/debug/monk` — use it only for development.

#### One-shot setup script

Prefer not to run the steps by hand? From a clone, this script checks for the Rust
toolchain (and offers to install it), builds and installs `monk` onto your `PATH`,
and finally runs `monk setup` — narrating each step.

**Prerequisites:** [git](https://git-scm.com/downloads) to clone the repo, plus the C
linker that the Rust build needs — [Xcode Command Line Tools](https://developer.apple.com/xcode/resources/)
on macOS (`xcode-select --install`), `build-essential` on Linux, or the
[Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
on Windows. The [Rust toolchain](https://rustup.rs) itself (1.82+) is installed by the
script if it is missing.

```sh
# Linux / macOS
git clone https://github.com/mdportnov/monk-cli && cd monk-cli
./scripts/setup.sh
```

```powershell
# Windows (PowerShell 5+) — open the terminal as Administrator for the privileged step
git clone https://github.com/mdportnov/monk-cli; cd monk-cli
powershell -ExecutionPolicy Bypass -File .\scripts\setup.ps1
```

### cargo-binstall

```sh
cargo binstall monk
```

### Package managers

- **Debian / Ubuntu**: `cargo deb` produces a `.deb` wired up for systemd user units; bash/zsh/fish completions are bundled.
- **Fedora / RHEL**: `cargo generate-rpm` produces an `.rpm` (completions bundled).
- **Windows**: `assets/install.ps1` (above). MSI / Scoop manifest coming soon.
- **macOS**: a `.pkg` is attached to every release; it drops `monk` into `/usr/local/bin` and nothing else — you still run `monk setup` afterwards. Unless the release was built with a Developer ID identity the package is **unsigned**, so Gatekeeper blocks a double-click: right-click → *Open*, or use the install script above. Homebrew tap coming soon.

### Requirements

- A terminal and admin rights on your machine — blocking edits the system `hosts` file, which is privileged. You grant this **once**, during setup.
- Rust 1.82+ only if you build from source.

`monk setup` wires the privileges up for you; what happens under the hood differs per OS:

- **macOS** — the daemon is installed as a system service that runs as **root** and owns `/etc/hosts`. Setup shows a native macOS password prompt; the equivalent CLI command is `sudo monk service install`.
- **Linux** — the daemon runs as **you** via a `systemd` *user* unit (no `sudo`). For blocking to work monk must be able to write `/etc/hosts` (or use a `systemd-resolved` backend) — `monk doctor` tells you if it can't.
- **Windows** — the daemon is a logon **scheduled task** that needs elevation, so open your terminal **as Administrator** before running `monk setup` / `monk daemon install`.

## Quick start

Run these in order. On **Windows** open PowerShell **as Administrator** first; on **macOS** you'll get a password prompt during setup.

```sh
monk setup                  # 1. first-run wizard: language, profile, daemon, completions, health check
monk doctor                 # 2. confirm it's wired up (daemon running, hosts writable)
monk start deepwork -d 50m  # 3. start a 50-minute focus session
monk status                 # 4. what's blocked and how long is left
monk stop                   # 5. end it — soft mode only; hard mode can't be stopped
```

Prefer a UI? `monk tui` opens the full dashboard. Start a committed session you can't quit with `monk start deepwork --hard`.

## Commands

### Sessions

| Command                                              | What it does                                  |
| ---------------------------------------------------- | --------------------------------------------- |
| `monk start [PROFILE] [-d DUR] [--hard] [--reason …]` | Start a focus session (`--hard` = irreversible) |
| `monk stop`                                          | End the active session (soft mode only)       |
| `monk panic [--phrase …] [--cancel]`                 | Request — or cancel — a delayed hard-mode escape |
| `monk status`                                        | Daemon + session status                       |
| `monk stats`                                         | Session statistics                            |
| `monk tui`                                           | Open the interactive dashboard                |

### Profiles & apps

| Command                                                          | What it does                              |
| --------------------------------------------------------------- | ----------------------------------------- |
| `monk profiles`                                                 | List profiles                             |
| `monk profile show NAME [--json]`                               | Show a profile's full configuration       |
| `monk profile create NAME [--preset P]`                         | Create an empty profile or seed from a preset |
| `monk profile duplicate SOURCE [TARGET]`                        | Copy a profile under a new name           |
| `monk profile edit NAME`                                        | Interactive picker — apps, groups, hosts  |
| `monk profile edit NAME --add/--remove ID`                      | Scriptable profile edits                  |
| `monk profile limits NAME [--max/--min/--cooldown/--daily-cap] [--clear]` | Set or clear time limits        |
| `monk profile delete NAME`                                      | Remove a profile                          |
| `monk apps list [--refresh]`                                    | Show the installed-app cache              |
| `monk apps scan`                                                | Force a rescan of installed applications  |

Built-in presets for `--preset`: `deepwork`, `study`, `detox`, `sleep`, `sober`, `lockdown`, `no-social`, `no-video`, `no-news`, `no-games`, `no-chat`, `no-shopping`, `no-adult`, `no-gambling`, `no-dating`, `no-ai`.

### Daemon

`monk service` is an alias for `monk daemon`. `install` and `uninstall` need elevation: on macOS they show the native admin prompt when you didn't start them with `sudo`; on Windows use an Administrator terminal; on Linux it's a `systemd` user unit and needs no `sudo`. After `monk update` the service is refreshed automatically so the daemon never keeps running an older binary — `monk doctor` warns if it ever does.

| Command                            | What it does                                          |
| ---------------------------------- | ----------------------------------------------------- |
| `monk daemon start`                | Launch the background daemon                          |
| `monk daemon stop`                 | Shut it down cleanly                                  |
| `monk daemon status`               | Same as `monk status`                                 |
| `monk daemon run`                  | Run in the foreground (used by the service manager; not normally invoked manually) |
| `monk daemon install [--reinstall]` | Install as systemd / launchd / Windows scheduled task |
| `monk daemon uninstall [--purge]`  | Remove the service (`--purge` also wipes config + data) |

### Menu bar (macOS)

A native status item next to the clock: a ring when idle, a filled dot with a countdown while a session runs (🔒 in hard mode). Every profile gets a submenu with blocked-site/app counts, duration choices and a hard-mode start; hard sessions can't be stopped from the menu — `monk panic` in a terminal stays the only escape. The setup wizard offers it on macOS (skip with `--no-menubar`); if you use a menu bar manager like Ice or Bartender, the icon may start in the hidden section — Cmd-drag it out once.

| Command                  | What it does                                            |
| ------------------------ | ------------------------------------------------------- |
| `monk menubar`           | Run the menu bar app in the foreground                  |
| `monk menubar install`   | Register it as a login item and start it now            |
| `monk menubar uninstall` | Stop the login-item instance and remove the registration |

### Config & diagnostics

| Command                                | What it does                                          |
| -------------------------------------- | ----------------------------------------------------- |
| `monk setup` / `monk init [--quick] [--reset] [-y]` | First-run wizard: config, daemon, completions, menu bar (macOS), doctor |
| `monk doctor [--json] [--fix]`         | Environment, permissions and daemon health check; `--fix` self-repairs common issues |
| `monk config path`                     | Print the config file path                            |
| `monk config export`                   | Dump the current config                               |
| `monk config import FILE`              | Validate and import a config                          |
| `monk lang en\|ru`                     | Switch interface language                             |
| `monk completions SHELL`               | Emit shell completions (bash/zsh/fish/powershell/elvish) |

## Configuration

Config lives at:

- Linux: `~/.config/monk/config.toml`
- macOS: `~/Library/Application Support/monk/config.toml`
- Windows: `%APPDATA%\monk\config.toml`

```toml
[general]
default_profile = "deepwork"
default_duration = "50m"
hard_mode = false
autostart = true
locale = "en"

[profiles.deepwork]
site_groups = ["global.social", "global.video", "global.news", "ru.social", "ru.news"]
sites = ["example.com"]
apps  = ["com.tinyspeck.slackmacgap", "com.hnc.Discord"]
```

App ids are stable identifiers produced by the scanner: macOS bundle ids, Linux `.desktop` ids, Windows shortcut targets.

## Hard mode

Hard mode is the whole point. Once you start a hard session:

- The CLI refuses `monk stop`.
- The daemon ignores SIGTERM/SIGINT until the session is over.
- The session lock is signed with a key derived from a stable machine identity; tampering with the file is detected and treated as an active block.
- `monk panic` schedules a delayed release (configurable cooldown) so you can opt out of a runaway session without making it a one-tap escape hatch.

Use it deliberately.

## Development

```sh
just fmt        # rustfmt
just lint       # clippy -D warnings
just test       # cargo test
just run init   # cargo run -- init
```

The repo enforces `unsafe_code = "deny"` and a strict clippy profile. CI runs on Linux, macOS and Windows.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

---

<p align="center">
  Built by <a href="https://mikeportnov.com/en/projects">Mike Portnov</a> · <a href="https://github.com/mdportnov">@mdportnov</a>
</p>
