# monk

A cross-platform focus & distraction blocker built in Rust. One binary, one daemon, zero nonsense — block apps and websites, commit to hard-mode sessions, and get your attention back.

> 🇷🇺 Русская версия: [README.ru.md](./README.ru.md)

```
  ┏┳┓ ┏┓ ┏┓ ┃┏
  ┃┃┃ ┃┃ ┃┃ ┣┻┓
  ┛ ┗ ┗┛ ┛┗ ┛ ┗
  focus, weaponized.
```

## Highlights

- **Real app blocking** — scans installed applications on macOS, Linux and Windows, so you pick from a curated list instead of guessing process names.
- **Curated site presets** — bundled `global` and `ru` site groups (social, video, news, chat, shopping, games) with subdomain expansion baked in.
- **Hard mode** — tamper-evident session lock signed with BLAKE3 keyed HMAC. No `monk stop`, no config edits, no daemon kill can shortcut it.
- **Background daemon** — Unix domain / local socket IPC, fail-closed reconciliation loop, SIGTERM-safe cleanup, systemd / launchd / Windows Service install.
- **Interactive TUI** — ratatui-powered dashboard for sessions, stats and profile editing.
- **Localized** — English and Русский out of the box via `rust-i18n`.
- **Zero unsafe** — `#![deny(unsafe_code)]` in the main crate.

## How it works

monk runs a small always-on daemon (`monkd`) that owns the block state. The CLI and TUI talk to it over a local socket. When you start a session:

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

### From source

```sh
git clone https://github.com/mdportnov/monk-cli
cd monk-cli
cargo install --path .
```

### cargo-binstall

```sh
cargo binstall monk
```

### Package managers

- **Debian / Ubuntu**: `cargo deb` produces a `.deb` wired up for systemd user units.
- **Fedora / RHEL**: `cargo generate-rpm` produces an `.rpm`.
- **macOS**: Homebrew tap coming soon.
- **Windows**: MSI / Scoop manifest coming soon.

### Requirements

- Rust 1.82+ (only for building from source)
- Root / admin access once, to let monk write the `hosts` file
- Linux: `systemd` (user session) for `monk daemon install`
- Windows: nothing — the daemon registers as a per-user service

## Quick start

```sh
monk init                       # interactive onboarding wizard
monk start deepwork -d 50m      # start a 50-minute session
monk start deepwork --hard      # commit — no stop until it's over
monk status                     # what's running, what's left
monk stop                       # end the session (soft mode only)
monk tui                        # full dashboard
```

## Commands

### Sessions

| Command                         | What it does                                   |
| ------------------------------- | ---------------------------------------------- |
| `monk start [profile] [-d DUR]` | Start a focus session                          |
| `monk start … --hard`           | Start a hard-mode session (irreversible)       |
| `monk stop`                     | End the active session                         |
| `monk panic [--phrase …]`       | Request a delayed hard-mode escape             |
| `monk status`                   | Daemon + session status                        |

### Profiles & apps

| Command                                    | What it does                             |
| ------------------------------------------ | ---------------------------------------- |
| `monk profiles`                            | List profiles                            |
| `monk profile create NAME`                 | Create an empty profile                  |
| `monk profile delete NAME`                 | Remove a profile                         |
| `monk profile edit NAME`                   | Interactive picker — apps, groups, hosts |
| `monk profile edit NAME --add/--remove ID` | Scriptable profile edits                 |
| `monk apps list [--refresh]`               | Show the installed-app cache             |
| `monk apps scan`                           | Force a rescan of installed applications |

### Daemon

| Command                 | What it does                                    |
| ----------------------- | ----------------------------------------------- |
| `monk daemon start`     | Launch the background daemon                    |
| `monk daemon stop`      | Shut it down cleanly                            |
| `monk daemon status`    | Same as `monk status`                           |
| `monk daemon install`   | Install as systemd / launchd / Windows service  |
| `monk daemon uninstall` | Remove the service                              |

### Config & diagnostics

| Command                | What it does                                      |
| ---------------------- | -------------------------------------------------- |
| `monk doctor`          | Environment, permissions and daemon health check   |
| `monk config path`     | Print the config file path                         |
| `monk config export`   | Dump the current config                            |
| `monk config import F` | Validate and import a config                       |
| `monk lang en\|ru`     | Switch interface language                          |
| `monk completions SH`  | Emit shell completions (bash/zsh/fish/powershell)  |

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

Dual-licensed under MIT or Apache-2.0.
