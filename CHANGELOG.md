# Changelog

All notable changes to `monk` are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2] - 2026-08-01

### Added

- **Schedules** — profiles auto-start on a weekly recurring schedule
  (weekday mask, HH:MM window, timezone with `local` following the laptop,
  cross-midnight windows, DST-safe). New TUI screen (menu → Schedules, `w`):
  next-run status per mode, upcoming-window preview, space to toggle,
  structured edit form. Home shows the next scheduled window; scheduled
  sessions carry a badge and end time. Scheduled presets: new
  `morning` (daily 06:00–09:00) and `sleep` (daily 22:30–07:00).
- **Self-update** — `monk update` downloads the latest GitHub release for
  the current platform, verifies its SHA-256 against the signed manifest,
  and atomically swaps the binary (with rollback). `monk update --check`
  and the `u` key in the TUI check without installing; the TUI footer shows
  the running version plus a badge when a newer release exists (checks are
  cached for 24h).


- **`monk setup` first-run wizard** — detects the platform, installs the
  background daemon service, installs shell completions for the current shell,
  runs the doctor health checks, and prints next steps. Safe to run
  non-interactively (falls back to sensible defaults when stdin is not a TTY).
- **Shell completions packaging** — bash/zsh/fish completions are generated into
  `assets/completions/` (via a new `just completions` recipe) and shipped in the
  `.deb` and `.rpm` packages at the conventional system locations.
- **Windows installer** — `assets/install.ps1`, a dependency-free PowerShell
  installer mirroring the existing `install.sh` (arch detection, checksum
  verification, PATH setup).
- **TUI quality-of-life**:
  - Narrow-terminal guard — shows a clear "resize" message below 80×20 instead
    of a broken layout.
  - Prominent hard-mode badge on the session screen (high-contrast pill,
    readable on light terminals).
  - Daily-cap progress on the confirm screen — see remaining budget before
    starting a session.
  - Type-to-filter search in the mode picker.

### Changed

- **Full TUI internationalization** — 82 previously hard-coded strings across the
  menu, editor, settings, picker, confirm, doctor, home, panic, and companion
  screens now go through `rust-i18n`, with English and Russian translations.
  Fixed several unnatural Russian calques.
- **Consistent TUI key conventions** — Enter activates the primary action
  everywhere; Space is reserved for toggles (multi-select). On-screen hints and
  help overlays updated to match.

### Fixed

- Schedules with `start == end` are rejected instead of silently becoming
  a 24-hour window.

- **Hosts-write durability** — `atomic_write` now `fsync`s both the file and its
  parent directory around the rename, so a crash or power loss can no longer
  leave a truncated hosts file and silently drop the block (fail-closed).
- **Flatpak/Snap app blocking** — the application id was discarded while parsing
  `.desktop` files, so sandboxed apps never matched and were never blocked. The
  id is now parsed (`flatpak run [opts] APP_ID` / `snap run NAME` /
  `/snap/bin/NAME`) and matched via the process cgroup.
- **Process kill-loop correctness** — graceful-quit tracking is now pinned to a
  process's start time, so a recycled PID can't cause a stale escalation; and
  Linux process matching accounts for the kernel's 15-byte `/proc/<pid>/comm`
  truncation so long binary names no longer slip through.
- **DNS cache flush** — the flush routines were dead code and never ran on Linux
  or Windows after a hosts change; they are now invoked on apply/revert, check
  the tool exit status, and try multiple resolvers (`resolvectl` →
  `systemd-resolve` → `nscd` on Linux). All best-effort: a flush failure never
  breaks an applied block.
- **Windows build** — corrected `LocalFree` usage for windows-rs 0.58
  (`P: Param<HLOCAL>`), unblocking the Windows compile that had been failing.
- **Cross-platform CI** — resolved clippy lints across Linux, macOS, and Windows
  (including the newer toolchain's `unnecessary_sort_by`), restored the MSRV
  (1.82) check, and quieted an unmaintained-advisory notice for the transitive,
  build-time `proc-macro-error2` dependency. All CI jobs are green.

[Unreleased]: https://github.com/mdportnov/monk-cli/commits/main
