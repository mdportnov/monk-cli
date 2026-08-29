# AGENTS.md

Agent instructions for the `monk` repository.

## Project Overview

Cross-platform CLI focus & distraction blocker built in Rust. A single binary (`monk`); the background daemon is the same binary run as `monk daemon run` and registered with the OS service manager under the name `monkd`. The daemon owns the block state and talks to clients over a local socket.

- Repository: `https://github.com/mdportnov/monk-cli`
- License: MIT OR Apache-2.0
- Edition 2021, Rust 1.82+
- Targets: macOS, Linux, Windows

## Stack

| Layer         | Crate / tech                                                       |
| ------------- | ------------------------------------------------------------------ |
| CLI           | `clap` v4 derive, `clap_complete`, `inquire`                       |
| TUI           | `ratatui`, `crossterm`, `tui-big-text`, `tachyonfx`                |
| Async         | `tokio` multi-thread, `tokio-util`, `futures`                      |
| IPC           | `interprocess` (Unix domain / Windows named pipe)                  |
| Persistence   | `toml` config, `rusqlite` (bundled), atomic `fs-err` writes        |
| Integrity     | `blake3` keyed HMAC, `machine-uid`                                 |
| Process model | `sysinfo`; `nix` on Unix, `windows` crate on Windows               |
| App discovery | `plist` (macOS), freedesktop `.desktop` parser (Linux), `lnk` (Windows) |
| Errors        | `thiserror` + `miette`                                             |
| Observability | `tracing`, `tracing-subscriber`, `tracing-appender`                |
| i18n          | `rust-i18n`, `sys-locale`                                          |

## Layout

```
src/
  main.rs                  entry point
  lib.rs                   library root
  cli/                     clap command definitions and dispatch
  blocker/                 host-file injection + process kill loop
    {macos,linux,windows}.rs   per-platform implementations
    dns_server.rs, process.rs
  brands/                  app-id resolution + bundled site presets
  config/                  toml schema, load/save
  doctor/                  health checks + self-repair actions
  platform/                OS-specific app discovery, paths, hosts file
  menubar/                 macOS menu bar companion (tray icon, launch agent)
  tui/                     ratatui dashboard (app, view, widgets, theme)
  audit.rs, telemetry.rs, clock.rs, paths.rs, error.rs
tests/                     integration tests
locales/                   rust-i18n YAML (en.yml, ru.yml)
docs/                      design docs
assets/                    install.sh, systemd units
```

## Build, Test, Lint

Use the `justfile`:

```sh
just fmt        # rustfmt
just lint       # cargo clippy -- -D warnings
just test       # cargo test
just run <args> # cargo run -- <args>
```

Before any commit: `just fmt && just lint && just test`. CI runs all three on Linux, macOS, Windows.

## Code Conventions

- `unsafe_code = "deny"` is enforced in the main crate — no `unsafe` blocks.
- `clippy::all` is warn-by-default; CI treats warnings as errors. Fix lints, do not silence them.
- Errors: define module errors with `thiserror::Error`; surface user-facing errors as `miette` reports.
- Async: never block in `async fn` — use `tokio::task::spawn_blocking` for sync work.
- Logging: `tracing` macros only (`info!`, `warn!`, `error!`, `debug!`). No `println!` outside the CLI/TUI render layer.
- Paths: never hardcode platform paths; route through `src/paths.rs` (uses `directories`).
- File writes that mutate shared state (hosts file, session lock, config) must be atomic and go through `fs-err` for context-rich errors.

## Platform-Specific Code

- Conditional compilation via `#[cfg(target_os = "...")]` or `cfg(unix)` / `cfg(windows)`.
- Per-platform implementations live in `src/{module}/{macos,linux,windows}.rs` with a `mod.rs` selecting the right one.
- When touching platform code, state in the PR which OSes were tested manually; CI covers the rest.

## Hard Mode Invariants (do not weaken)

- The session lock is signed with BLAKE3 keyed HMAC; the key derives from `machine-uid` so a copied lock from another machine fails verification.
- The daemon ignores `SIGTERM`/`SIGINT` while a hard session is active.
- The CLI must refuse `monk stop` during a hard session — never add a bypass flag or env override.
- Tampered or missing lock files must keep the block active (fail-closed).
- `monk panic` is the only escape; it schedules a delayed release with a configurable cooldown.

See `docs/hard-mode.md` for the full threat model.

## i18n

- All user-facing strings go through `rust-i18n`'s `t!` macro.
- New keys must be added to **both** `locales/en.yml` and `locales/ru.yml`.
- `tests/i18n_keys.rs` enforces key parity across locales — keep it passing.

## Reference Docs

- `docs/hard-mode.md` — hard-mode threat model and invariants
- `docs/PRD-schedules-brands.md` — schedules & brands feature design

## Commit & PR Guidelines

- Conventional commit prefixes: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`.
- Keep PRs scoped. Cross-platform changes should list which OSes were tested.
- Do not commit anything under `target/`.
