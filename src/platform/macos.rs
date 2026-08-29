use std::path::{Path, PathBuf};
use std::process::Command;

use super::unix_shared;
use crate::{Error, Result};

pub use unix_shared::{apply_sudo_user_home, chown_to_sudo_user, sudo_user_ids};

const LAUNCHD_LABEL: &str = "dev.monk.monkd";
const SYSTEM_LAUNCHD_PLIST: &str = "/Library/LaunchDaemons/dev.monk.monkd.plist";
const SYSTEM_DATA_DIR: &str = "/Library/Application Support/monk";
const SYSTEM_RUNTIME_DIR: &str = "/var/run/monk";
const SYSTEM_BIN_DIR: &str = "/usr/local/libexec/monk";
const SYSTEM_BIN: &str = "/usr/local/libexec/monk/monkd";

/// Stable success marker printed by `install_service` on every successful
/// completion (including no-op reinstalls). The elevated parent uses this to
/// distinguish a working child from an osascript that returned 0 over a
/// silently-failed `monk service install`.
const INSTALL_SUCCESS_MARKER: &str = "monk:service:install:ok";

/// Same idea for the uninstall side — see [`INSTALL_SUCCESS_MARKER`].
const UNINSTALL_SUCCESS_MARKER: &str = "monk:service:uninstall:ok";

/// True when the system-wide LaunchDaemon is installed: all paths resolve to
/// system locations regardless of who's calling.
pub fn system_mode() -> bool {
    Path::new(SYSTEM_LAUNCHD_PLIST).exists()
}

pub fn system_data_dir() -> Option<&'static str> {
    Some(SYSTEM_DATA_DIR)
}

pub fn system_runtime_dir() -> Option<&'static str> {
    Some(SYSTEM_RUNTIME_DIR)
}

#[allow(dead_code)]
pub fn set_acl_current_user(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn legacy_user_config_file(user: &str) -> PathBuf {
    PathBuf::from(format!("/Users/{user}/Library/Application Support/dev.monk.monk/config.toml"))
}

pub fn legacy_user_data_dir(user: &str) -> PathBuf {
    PathBuf::from(format!("/Users/{user}/Library/Application Support/dev.monk.monk"))
}

/// One-shot migration: when the system-mode daemon starts for the first time
/// and the system data dir is empty, copy a legacy per-user config + audit
/// log. Idempotent and never overwrites existing files. Preference order:
/// SUDO_USER (the user who ran `sudo monk service install`), then the user
/// with the most-recently-modified config.toml under `/Users/*`.
pub fn migrate_legacy_if_needed() {
    if !system_mode() {
        return;
    }
    let Ok(sys_dir) = crate::paths::data_dir() else { return };
    let sys_config = sys_dir.join("config.toml");
    if sys_config.exists() {
        return;
    }
    let Some(user) = pick_legacy_user() else { return };
    let legacy_cfg = legacy_user_config_file(&user);
    let legacy_data = legacy_user_data_dir(&user);
    if !legacy_cfg.exists() {
        return;
    }
    tracing::info!(%user, "migrating legacy per-user data to system path");
    if let Err(e) = fs_err::copy(&legacy_cfg, &sys_config) {
        tracing::warn!(?e, "config migrate failed");
        return;
    }
    let _ = checkpoint_legacy_sqlite(&legacy_data);
    for name in [
        "audit.sqlite3",
        "audit.sqlite3-wal",
        "audit.sqlite3-shm",
        "audit.log",
        "session.lock",
        "apps.json",
    ] {
        let src = legacy_data.join(name);
        let dst = sys_dir.join(name);
        if src.exists() && !dst.exists() {
            if let Err(e) = fs_err::copy(&src, &dst) {
                tracing::warn!(?e, file = %src.display(), "migrate copy failed (continuing)");
            }
        }
    }
    let marker = legacy_data.join(".migrated");
    if fs_err::write(&marker, chrono::Utc::now().to_rfc3339()).is_ok() {
        chown_path_to_user(&marker, &user);
    }
}

#[allow(unsafe_code)]
fn chown_path_to_user(path: &Path, user: &str) {
    let Ok(Some(u)) = nix::unistd::User::from_name(user) else { return };
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
    unsafe {
        let _ = libc::chown(c.as_ptr(), u.uid.as_raw(), u.gid.as_raw());
    }
}

fn checkpoint_legacy_sqlite(legacy_data: &Path) -> Result<()> {
    let db = legacy_data.join("audit.sqlite3");
    if !db.exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(&db).map_err(Error::from)?;
    let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    Ok(())
}

fn pick_legacy_user() -> Option<String> {
    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let cfg = legacy_user_config_file(&user);
            if cfg.exists() {
                return Some(user);
            }
        }
    }
    let entries = fs_err::read_dir("/Users").ok()?;
    let mut candidates: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "Shared" {
            continue;
        }
        let cfg = entry.path().join("Library/Application Support/dev.monk.monk/config.toml");
        if let Ok(meta) = fs_err::metadata(&cfg) {
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            candidates.push((mtime, name));
        }
    }
    candidates.sort_by_key(|b| std::cmp::Reverse(b.0));
    candidates.into_iter().next().map(|(_, u)| u)
}

fn require_root() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        Ok(())
    } else {
        Err(Error::Permission(
            "this command must run as root — try `sudo monk service install`".into(),
        ))
    }
}

pub fn install_service(bin: &str) -> Result<String> {
    require_root()?;

    let mut msgs = Vec::new();

    fs_err::create_dir_all(SYSTEM_BIN_DIR)?;
    let src = Path::new(bin);
    let dst = Path::new(SYSTEM_BIN);
    if src.canonicalize().ok().as_deref() != Some(dst) {
        // Stop the running daemon *before* touching its executable, and
        // unlink the old copy instead of writing over it: macOS refuses to
        // open a running executable for writing (ETXTBSY), which used to
        // make every reinstall-over-a-live-daemon fail here. Unlinking is
        // safe — a running process keeps its inode alive.
        bootout_existing();
        if dst.exists() {
            fs_err::remove_file(dst)?;
        }
        fs_err::copy(src, dst)?;
        msgs.push(format!("copied binary → {}", dst.display()));
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(dst, std::fs::Permissions::from_mode(0o755));
    }

    let data_dir = Path::new(SYSTEM_DATA_DIR);
    fs_err::create_dir_all(data_dir)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(data_dir, std::fs::Permissions::from_mode(0o755));
    }
    let log = data_dir.join("monk.log");

    let tpl = include_str!("../../assets/launchd/dev.monk.monkd.daemon.plist");
    let rendered =
        tpl.replace("__BIN__", &dst.to_string_lossy()).replace("__LOG__", &log.to_string_lossy());
    fs_err::write(SYSTEM_LAUNCHD_PLIST, rendered)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            fs_err::set_permissions(SYSTEM_LAUNCHD_PLIST, std::fs::Permissions::from_mode(0o644));
    }
    msgs.push(format!("wrote {SYSTEM_LAUNCHD_PLIST}"));

    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let legacy =
                PathBuf::from(format!("/Users/{user}/Library/LaunchAgents/dev.monk.monkd.plist"));
            if legacy.exists() {
                if let Some((uid, _)) = sudo_user_ids() {
                    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}")], Some(&legacy));
                }
                let _ = fs_err::remove_file(&legacy);
                msgs.push(format!("removed legacy LaunchAgent {}", legacy.display()));
            }
        }
    }

    super::cleanup_hosts();
    msgs.push("reverted /etc/hosts (clears stale monk markers)".into());

    // bootout system on a not-yet-bootstrapped plist errors with code 5; we
    // expect that on first install — suppress the noise.
    bootout_existing();
    let status =
        Command::new("launchctl").args(["bootstrap", "system"]).arg(SYSTEM_LAUNCHD_PLIST).status();
    match status {
        Ok(s) if s.success() => msgs.push("loaded via launchctl bootstrap system".into()),
        _ => msgs.push(format!(
            "manual start: `sudo launchctl bootstrap system {SYSTEM_LAUNCHD_PLIST}`"
        )),
    }

    // Stable success marker — parsed by elevate_install_service to confirm
    // the elevated child actually ran end-to-end (no-op reinstalls otherwise
    // produce no plist-write line and look like silent failures).
    msgs.push(INSTALL_SUCCESS_MARKER.into());

    Ok(msgs.join("\n"))
}

pub fn uninstall_service(purge: bool) -> Result<String> {
    require_root()?;

    let mut msgs = Vec::new();

    if super::try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    if Path::new(SYSTEM_LAUNCHD_PLIST).exists() {
        bootout_existing();
        fs_err::remove_file(SYSTEM_LAUNCHD_PLIST)?;
        msgs.push(format!("removed {SYSTEM_LAUNCHD_PLIST}"));
    }

    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let legacy =
                PathBuf::from(format!("/Users/{user}/Library/LaunchAgents/dev.monk.monkd.plist"));
            if legacy.exists() {
                if let Some((uid, _)) = sudo_user_ids() {
                    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}")], Some(&legacy));
                }
                let _ = fs_err::remove_file(&legacy);
                msgs.push(format!("removed legacy LaunchAgent {}", legacy.display()));
            }
        }
    }

    super::cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    if Path::new(SYSTEM_BIN).exists() {
        let _ = fs_err::remove_file(SYSTEM_BIN);
    }
    let _ = fs_err::remove_dir(SYSTEM_BIN_DIR);

    if purge {
        let sys_data = Path::new(SYSTEM_DATA_DIR);
        if sys_data.exists() {
            let _ = fs_err::remove_dir_all(sys_data);
            msgs.push("purged system data".into());
        }
    }

    if msgs.is_empty() {
        msgs.push("uninstalled".into());
    }
    msgs.push(UNINSTALL_SUCCESS_MARKER.into());
    Ok(msgs.join(", "))
}

/// Nothing to restart in isolation on macOS: the service runs its own copy
/// of the binary under `/usr/local/libexec`, so picking up a new version
/// means a full (privileged) reinstall, not a bounce.
pub fn restart_service() -> Option<Result<String>> {
    None
}

/// Unload a previously bootstrapped monkd, by label and by plist path.
///
/// The label form (`system/dev.monk.monkd`) also works when the plist file is
/// missing or has already been replaced; the path form covers older launchctl
/// behaviour. Both are noisy no-ops on a first install, hence `quiet_*`.
fn bootout_existing() {
    let _ = quiet_launchctl(&["bootout", &format!("system/{LAUNCHD_LABEL}")], None);
    if Path::new(SYSTEM_LAUNCHD_PLIST).exists() {
        let _ = quiet_launchctl(&["bootout", "system"], Some(Path::new(SYSTEM_LAUNCHD_PLIST)));
    }
}

fn quiet_launchctl(
    args: &[&str],
    extra_path: Option<&Path>,
) -> std::io::Result<std::process::ExitStatus> {
    use std::process::Stdio;
    let mut cmd = Command::new("launchctl");
    cmd.args(args);
    if let Some(p) = extra_path {
        cmd.arg(p);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).status()
}

/// Run a privileged `monk service …` subcommand through the native macOS
/// authorization prompt (osascript `with administrator privileges`).
///
/// osascript exiting 0 only proves the AppleScript ran — the child may have
/// failed silently — so the child's stable success marker is required before
/// we report success.
fn elevate_service_command(args: &str, prompt: &str, marker: &str, retry: &str) -> Result<String> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.to_string_lossy().to_string();
    let sh_quoted = shell_single_quote(&exe_str);
    let inner = format!("{sh_quoted} {args}");
    let as_inner = escape_applescript(&inner);
    let as_prompt = escape_applescript(prompt);
    let script = format!(
        "do shell script \"{as_inner}\" with prompt \"{as_prompt}\" with administrator privileges"
    );
    let out = Command::new("osascript").args(["-e", &script]).output().map_err(|e| {
        Error::Elevation(format!("osascript failed to launch: {e} — run manually: {retry}"))
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(if is_user_cancelled(out.status.code(), &stderr) {
            Error::Elevation(format!("admin prompt cancelled — run `{retry}` when ready"))
        } else {
            Error::Elevation(format!("{stderr} — try `{retry}`"))
        });
    }
    if stdout.contains(marker) {
        Ok(stdout)
    } else {
        Err(Error::Elevation(format!(
            "child command did not confirm — stdout: {stdout:?} stderr: {stderr:?}"
        )))
    }
}

/// Install the system service, asking for admin rights when we lack them.
pub fn elevate_install_service() -> Result<String> {
    if nix::unistd::geteuid().is_root() {
        let bin = std::env::current_exe()?.to_string_lossy().into_owned();
        return install_service(&bin);
    }
    elevate_service_command(
        "service install",
        "monk wants to install the system service",
        INSTALL_SUCCESS_MARKER,
        "sudo monk service install",
    )
}

/// Remove the system service, asking for admin rights when we lack them.
///
/// Without this the user-facing `monk service uninstall` / `monk setup
/// --reset` simply failed on `require_root` and left the LaunchDaemon behind.
pub fn elevate_uninstall_service(purge: bool) -> Result<String> {
    if nix::unistd::geteuid().is_root() {
        return uninstall_service(purge);
    }
    let args = if purge { "service uninstall --purge" } else { "service uninstall" };
    elevate_service_command(
        args,
        "monk wants to remove the system service",
        UNINSTALL_SUCCESS_MARKER,
        "sudo monk service uninstall",
    )
}

fn is_user_cancelled(code: Option<i32>, stderr: &str) -> bool {
    // AppleScript user-cancelled error: code -128, surfaces in stderr as a
    // bracketed token. osascript's process exit is typically 1 in this case;
    // the discriminator is the -128 string in stderr.
    let cancelled_token = stderr.contains("(-128)") || stderr.contains("error -128");
    let cancelled_text = stderr.contains("User canceled") || stderr.contains("User cancelled");
    cancelled_token || cancelled_text || matches!(code, Some(-128))
}

fn shell_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Best-effort native macOS notification via osascript. Fire-and-forget: a
/// wedged osascript (Notification Center under permission prompts, slow
/// startup after wake-from-sleep) must not block CLI commands. The kernel
/// reaps the detached child.
///
/// Title/body are clamped to a conservative length — AppleScript's
/// `display notification` truncates silently around 256 chars on some macOS
/// versions, and very long strings can fail the parse outright.
pub fn notify(title: &str, body: &str) {
    let t = escape_applescript(&clamp(title, 96));
    let b = escape_applescript(&clamp(body, 160));
    let script = format!("display notification \"{b}\" with title \"{t}\"");
    let _ = Command::new("osascript")
        .args(["-e", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn clamp(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Escape for an AppleScript double-quoted string literal. AppleScript
/// understands `\\`, `\"`, `\n`, `\t`, `\r`. Anything else is passed through.
fn escape_applescript(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_single_quote_handles_apostrophe() {
        assert_eq!(shell_single_quote("/tmp/a"), "'/tmp/a'");
        assert_eq!(shell_single_quote("/U/o'reilly/m"), "'/U/o'\\''reilly/m'");
        assert_eq!(shell_single_quote("''"), "''\\'''\\'''");
    }

    #[test]
    fn escape_applescript_handles_control_chars() {
        assert_eq!(escape_applescript("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(escape_applescript("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_applescript("tab\there"), "tab\\there");
    }

    #[test]
    fn user_cancelled_detection() {
        assert!(is_user_cancelled(Some(1), "execution error: User canceled. (-128)"));
        assert!(is_user_cancelled(Some(1), "error -128: …"));
        assert!(is_user_cancelled(Some(-128), ""));
        assert!(!is_user_cancelled(Some(1), "execution error: something else (-1712)"));
    }
}
