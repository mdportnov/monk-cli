# PRD — Hard Mode

## Goal

Let a user commit to a focus session they cannot cancel on a whim, while keeping the escape hatch honest enough that the tool is still an instrument and not a trap.

## Non-goals

- Defending against a determined attacker with full root who has studied the source. Hard mode defends a person from their weaker self, not from a forensic adversary.
- Blocking distractions on other devices (phone, another laptop).
- Surviving a reboot into recovery mode, or a fresh OS install.
- Running as a system-level service installed with sudo. Distribution is plain GitHub release binaries installed with `curl | sh` or `install.ps1`, no package manager, no privileged installers. Everything runs user-space.

## Threat model (what we do defend against)

Ranked from cheap to expensive obstacles:

1. `monk stop`, Ctrl-C, TUI hotkey.
2. `kill <pid>` / Activity Monitor / Task Manager.
3. Editing `/etc/hosts` by hand.
4. Deleting `~/.config/monk` or state files.
5. Rewinding the system clock.
6. Disabling the autostart service so a killed daemon doesn't come back.

We do not defend against (7) recovery-mode reboots, (8) running another untouched device, (9) patching the binary with a hex editor. Those are outside the envelope.

## Design principles

- **Fail-closed.** If anything is ambiguous (tampered lock, lost state, clock jump) we keep the block on, never off.
- **State lives on disk, not in memory.** The daemon is a reconciler: it reads a desired-state file every tick and makes the world match. Killing the daemon doesn't change intent, only delays enforcement.
- **Wall clock is untrusted.** We track elapsed time via a monotonic counter plus a boot-id anchor, so rewinding the clock doesn't help.
- **Tamper-evident, not tamper-proof.** The lock file is HMAC-signed. Edits don't silently pass; they trigger a fail-closed response (extend the session, not drop it).
- **Honest escape hatch.** `monk panic` exists, but it costs time and friction. No escape hatch means bugs and real emergencies become disasters; that kills adoption.
- **Committing to hard mode is a ceremony.** Starting it requires explicit confirmation and typing a phrase, so it never happens by accident.

## Levels

- `off` — current soft behaviour. `stop` works.
- `hard` — stop is denied, escape only through `monk panic` with a delay and a phrase. This is the MVP.
- `ultra` — removed from scope for now. Required a privileged system installer which our distribution model does not support.

## Architecture

### Session lock file

`~/.local/share/monk/session.lock` (plus two backup copies in runtime and config dirs). Content:

```
schema_version
id                  (uuid)
profile
started_at_wall     (rfc3339 utc)
started_at_boot_ms  (monotonic, ms since boot)
boot_id             (stable per boot, see below)
duration_ms
progressed_ms       (updated every tick — survives reboots)
hard_mode           (bool)
panic_requested_at  (option<rfc3339>)
panic_phrase        (the one the user needs to type)
reason              (option<string>)
mac                 (hex hmac-sha256 over the body)
```

HMAC key derivation: `blake3(machine_id || session_id)`. `machine_id` comes from `/etc/machine-id` (Linux), `IOPlatformUUID` (macOS), `MachineGuid` registry (Windows). This is deterministic on purpose: we do not defend against source readers, we defend against casual hex editing. Anyone editing the file by hand without also recomputing the MAC produces a tamper signal.

### Boot identity and time

- Linux: `/proc/sys/kernel/random/boot_id`.
- macOS: `sysctl kern.boottime` (seconds since epoch at boot, stable across the boot).
- Windows: `GetTickCount64()` delta from `GetSystemTimeAsFileTime()` at first sample; we derive a synthetic boot-time stamp and hash it.

Tick logic every 1s:

1. Read lock, verify MAC.
2. Compute `delta = monotonic_now - last_tick_monotonic` (bounded to `[0, 5s]` to avoid huge jumps on wake from sleep).
3. `progressed_ms += delta`.
4. `remaining = max(0, duration_ms - progressed_ms)`.
5. If boot id changed, fall back to wall clock between ticks but keep `progressed_ms` as ground truth.
6. If wall clock jumped backward or more than an hour forward, log an audit event but do not change `progressed_ms`.
7. Rewrite lock with new `progressed_ms` and fresh MAC.
8. If `remaining == 0`, release the block and delete lock + backups.

Key property: the session advances only with real elapsed time. Rewinding the clock or rebooting shortens nothing and extends nothing.

### Reconciler

The supervisor becomes a loop that does not hold session state in memory as a source of truth. Every tick:

- Load `SessionLock` from disk (or quorum of backups).
- If lock exists and valid, reconcile: ensure hosts block is applied, kill banned processes, refresh progressed_ms.
- If lock is missing and we have no active session intent, idle.
- If lock has `panic_requested_at` and panic delay has elapsed, end the session.

Killing the daemon does not change the lock. The service manager restarts the daemon, and on restart it reads the lock and continues reconciling. In the gap, hosts may be unblocked by a manual edit, but the next tick (seconds later) restores it.

### Tamper response

If the lock's MAC does not verify:

1. Log the event to the audit log.
2. Extend the session by a penalty (`+15m`), rewrite the lock with a valid MAC at the new `ends_at`. Tampering costs time.
3. Continue reconciling.

If the lock file is missing but backups exist:

1. Pick the backup with the greatest `ends_at`.
2. Restore it to the primary location.
3. Log.

If all copies are missing:

1. We have no active intent. Accept as released. This is a known gap — the user could `rm` all three files. Backups live in three directories owned by the user so nothing short of `rm` stops them, which is the cost we accept given no-sudo distribution.

### Process hardening

While a hard-mode lock is active:

- Ignore `SIGINT`, `SIGTERM`, `SIGHUP` in the daemon. `SIGKILL` still works; the service manager brings us back.
- `setsid` / own process group on unix so a closed terminal doesn't take us down.
- On Windows set `SetConsoleCtrlHandler` to swallow `CTRL_C_EVENT` and `CTRL_CLOSE_EVENT`.

The CLI front-end (`monk stop`, `monk daemon stop`, `monk init --reset`, `monk daemon uninstall`) refuses with a typed error when a hard lock is active. The IPC protocol grows `Response::HardModeActive { ends_at, remaining, reason, panic_phrase }`.

### Service manager hooks

- systemd user unit: `Restart=always, RestartSec=1s, StartLimitBurst=0`.
- launchd plist: `KeepAlive=true` (unconditional).
- Windows Scheduled Task: `ON_IDLE` + `ON_LOGON` + a wrapper that relaunches on exit. We already install these in `daemon install`; we just need to update the templates.

These are best-effort. If the user `systemctl --user disable monk` they stop the restart loop. Hard mode does not prevent that action; it only prevents `monk stop`. This is a deliberate trade — we are not racing a determined unix user, we are adding friction during weak moments.

### Escape hatch: `monk panic`

Flow:

1. User runs `monk panic`.
2. CLI shows remaining time, the phrase they must type (localized), and asks for confirmation.
3. User types the phrase exactly. Mismatched input is not retried silently — it is logged and the command exits with a clear error.
4. On match, CLI sets `panic_requested_at` in the lock and exits, printing the wall time at which the session will actually end.
5. During the delay window, `monk panic --cancel` clears `panic_requested_at`. This is free.
6. When the reconciler tick observes `now >= panic_requested_at + panic_delay`, the session ends normally.

Defaults: `panic_delay = 15m`, `panic_phrase` generated from a short wordlist at hard-session start (not a static string — stored in lock so each session has its own).

### Audit log

Append-only `data_dir/audit.log`, one line per event, structured as JSON:

- `session_started`, `session_completed`, `session_panicked`
- `tamper_detected`
- `stop_denied`, `uninstall_denied`, `reset_denied`
- `daemon_crashed`, `daemon_restarted`
- `hosts_repaired` (we found the hosts file edited, we put our block back)
- `clock_anomaly`

Surfaced via `monk stats --audit`. The existence of the log is itself a mild deterrent; users see "I tried six times" and that alone helps.

### i18n

Every user-facing string added by this feature goes through `t!`. Keys under `hard.*` and `panic.*` in `locales/en.yml` and `locales/ru.yml`.

## CLI surface

- `monk start --hard [--reason "..."]` — prompts confirmation, prints phrase, starts hard session.
- `monk stop` — returns `HardModeActive` error when hard lock is on.
- `monk panic` — starts the escape delay.
- `monk panic --cancel` — cancels a pending escape.
- `monk stats --audit` — shows audit log.
- `monk doctor` — reports hard-mode state and warns if service manager autostart is missing.

TUI: a prominent "HARD MODE" header, disabled stop hotkey, countdown to the nearest of `session_ends` and `panic_ends`.

## Out of scope in phase 1

Explicitly deferred:

- DNS proxy as a blocker backend (phase 3).
- Dual watchdog process (phase 3).
- eBPF / EndpointSecurity app blocking (phase 3).
- System-level service install, immutable lock via `chattr`/`chflags` (was phase 2, dropped).
- Defeating a user who disables the systemd unit during a session.
- Cryptographic secrecy of the HMAC key (deterministic derivation is by design).

## Success criteria

- Running `monk stop` during a hard session prints a clear, localized refusal with remaining time.
- `kill -9` on the daemon followed by manual revert of `/etc/hosts` results in both the daemon and the hosts block being restored within a few seconds, every time, without user action.
- Rewinding the system clock by an hour does not reduce `remaining` for an active session.
- Editing the lock file by hand produces a logged tamper event and a penalty extension, never a silent unlock.
- `monk panic` takes at least `panic_delay` wall time to actually release the block.
- All new strings render in both English and Russian.

## Phase 1 subtasks

1. Add `SessionLock` struct, MAC, serde, disk IO, backups. Unit tests for roundtrip, MAC verify, tamper, quorum.
2. Add boot-id and monotonic sampling per OS behind a `clock` module. Unit tests where possible.
3. Extend `config.toml` schema with `hard_mode_level`, `panic_delay`, audit file path. Migration bump.
4. Rewrite `Supervisor::tick` as a reconciler driven by the lock file. Integration test with a fake hosts file.
5. Extend IPC protocol with `HardModeActive` response and `Panic { cancel: bool }` request.
6. Enforce hard mode in the daemon: deny `Stop`, deny `Shutdown`-as-exit, ignore `SIGINT/SIGTERM`, swallow console close on Windows.
7. Restore lock on daemon startup: read, verify, apply block, resume reconciler.
8. Implement `monk start --hard` ceremony: confirmation, phrase generation, write lock.
9. Implement `monk panic` and `monk panic --cancel`.
10. Implement tamper response: penalty extension, audit event.
11. Implement audit log writer and `monk stats --audit` reader.
12. Update service manager templates: systemd `Restart=always`, launchd `KeepAlive=true`, Windows task relaunch wrapper.
13. i18n: add `hard.*` and `panic.*` keys to en/ru.
14. Update `monk doctor` to report hard-mode state and autostart coverage.
15. Update TUI with hard-mode header and disabled stop.
16. End-to-end test: start hard session, kill daemon, observe restore; try stop, observe refusal; run panic, wait delay, observe release.

## Phase 3 (later, planning only)

- DNS-proxy blocker as an alternative backend to hosts file.
- eBPF-based app blocking on Linux, EndpointSecurity on macOS, WFP on Windows.
- Optional dual watchdog pair.
