use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sysinfo::{ProcessesToUpdate, Signal, System};
use tracing::{info, warn};

use crate::{
    apps::{AppKind, InstalledApp},
    Result,
};

/// Grace period between asking an app to quit and force-killing it.
const QUIT_GRACE: Duration = Duration::from_secs(3);

/// Our own pid, computed once. We never want to match ourselves —
/// `is_system_path` doesn't cover `/usr/local/libexec` where the daemon
/// lives, so guard explicitly.
fn self_pid() -> sysinfo::Pid {
    sysinfo::Pid::from_u32(std::process::id())
}

/// A graceful-quit request we're waiting on. `start_time` pins the request to
/// the exact process we asked — if the PID is recycled by a new process before
/// the grace window elapses, the start time differs and we discard the stale
/// entry instead of force-killing whatever now holds that PID.
#[derive(Debug, Clone, Copy)]
struct PendingQuit {
    asked_at: Instant,
    start_time: u64,
}

#[derive(Debug)]
pub struct ProcessGuard {
    sys: System,
    pending_quit: HashMap<sysinfo::Pid, PendingQuit>,
}

impl Default for ProcessGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessGuard {
    pub fn new() -> Self {
        Self { sys: System::new(), pending_quit: HashMap::new() }
    }

    /// Ask blocked apps to terminate. Mac bundles get an AppleScript Quit
    /// Apple Event so they exit normally without tripping crash-recovery
    /// dialogs (e.g. Telegram's "keeps crashing → log out" prompt). Other
    /// kinds get SIGTERM. If the process is still alive `QUIT_GRACE` later
    /// we escalate to SIGKILL.
    ///
    /// Returns the number of processes actually killed (force-terminated)
    /// on this call. Graceful-quit requests don't count toward the total
    /// since the process is still alive when we return.
    pub fn kill_matching(&mut self, apps: &[InstalledApp]) -> Result<usize> {
        if apps.is_empty() {
            return Ok(0);
        }

        // Refresh on every call — caller invokes us roughly once per second.
        // The 5-second cache the previous implementation used caused 3-5s
        // detection lag for newly-spawned apps.
        self.sys.refresh_processes(ProcessesToUpdate::All, false);

        // Drop tracking entries for processes that already exited (either by
        // our graceful quit, by the user, or naturally). Also drop entries
        // whose PID was recycled by a different process — the start time no
        // longer matches what we recorded when we asked it to quit.
        self.pending_quit.retain(|pid, pending| {
            self.sys.process(*pid).map(|p| p.start_time() == pending.start_time).unwrap_or(false)
        });

        let current_uid = get_current_uid();
        let now = Instant::now();
        let me = self_pid();

        let mut matches: Vec<(sysinfo::Pid, AppKind, String, String, Option<PathBuf>, u64)> =
            Vec::new();
        for proc in self.sys.processes().values() {
            if proc.pid() == me {
                continue;
            }
            if !should_consider_process(proc, current_uid) {
                continue;
            }
            let exe = proc.exe().map(Path::to_path_buf);
            let name_lower = proc.name().to_string_lossy().to_lowercase();
            let Some(app) =
                apps.iter().find(|a| matches_process(a, exe.as_deref(), &name_lower, proc.pid()))
            else {
                continue;
            };
            if let Some(p) = &exe {
                if is_system_path(p) {
                    warn!(pid = ?proc.pid(), name = %name_lower, "skip system process");
                    continue;
                }
            }
            matches.push((
                proc.pid(),
                app.kind,
                app.id.clone(),
                proc.name().to_string_lossy().to_string(),
                exe,
                proc.start_time(),
            ));
        }

        // Bundle apps often spawn multiple helper processes (Spotify Helper,
        // Telegram Helper, etc.). One AppleScript Quit to the parent bundle
        // exits the helpers too. Dedupe so we don't spawn N osascript
        // subprocesses per call.
        let mut quit_sent_for: HashSet<String> = HashSet::new();
        let mut killed = 0;
        for (pid, kind, app_id, name, exe, start_time) in matches {
            match self.pending_quit.get(&pid).copied() {
                // Past the grace window — force-kill.
                Some(pending) if now.duration_since(pending.asked_at) >= QUIT_GRACE => {
                    if let Some(proc) = self.sys.process(pid) {
                        if proc.kill() {
                            info!(
                                ?pid, %name, %app_id, ?exe,
                                "force-killed after graceful quit timed out"
                            );
                            killed += 1;
                            self.pending_quit.remove(&pid);
                        }
                    }
                }
                // Already asked, still within grace window — leave it alone.
                Some(_) => {}
                // First contact — ask politely.
                None => {
                    let asked =
                        if matches!(kind, AppKind::MacBundle) && quit_sent_for.contains(&app_id) {
                            // Already sent AppleScript quit for this bundle in
                            // this call. The event reaches every helper via the
                            // parent. Just track this pid so escalation works.
                            true
                        } else {
                            let ok = request_graceful_quit(&self.sys, pid, kind, &app_id);
                            if ok && matches!(kind, AppKind::MacBundle) {
                                quit_sent_for.insert(app_id.clone());
                            }
                            ok
                        };
                    if asked {
                        info!(?pid, %name, %app_id, kind = ?kind, "requested graceful quit");
                        self.pending_quit.insert(pid, PendingQuit { asked_at: now, start_time });
                    } else {
                        // Graceful path unavailable — fall back to SIGKILL.
                        if let Some(proc) = self.sys.process(pid) {
                            if proc.kill() {
                                info!(
                                    ?pid, %name, %app_id, ?exe,
                                    "killed (graceful unavailable)"
                                );
                                killed += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok(killed)
    }
}

fn request_graceful_quit(sys: &System, pid: sysinfo::Pid, kind: AppKind, app_id: &str) -> bool {
    #[cfg(target_os = "macos")]
    if matches!(kind, AppKind::MacBundle) {
        // AppleScript "quit" delivers a real Apple Event so the app saves
        // state and exits normally. `ignoring application responses` makes
        // it fire-and-forget — without it osascript blocks until the target
        // acks the event, which can hang for an unresponsive app and stall
        // the supervisor mutex.
        let script = format!(
            r#"ignoring application responses
   tell application id "{}" to quit
end ignoring"#,
            app_id
        );
        let result = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(result, Ok(s) if s.success()) {
            return true;
        }
        // osascript failed (no app registered with that id, scripting
        // disabled, etc.) — fall through to SIGTERM.
    }
    let _ = (kind, app_id);
    if let Some(proc) = sys.process(pid) {
        // kill_with returns None if the signal isn't supported on this
        // platform; treat that as "graceful unavailable" and let the caller
        // fall back to SIGKILL.
        return proc.kill_with(Signal::Term).unwrap_or(false);
    }
    false
}

fn matches_process(
    app: &InstalledApp,
    exe: Option<&Path>,
    name_lower: &str,
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))] pid: sysinfo::Pid,
) -> bool {
    match app.kind {
        AppKind::MacBundle => {
            if let Some(exe_path) = exe {
                if exe_path == app.exec_path {
                    return true;
                }
                let bundle_root = bundle_root_of(&app.exec_path);
                if let Some(root) = bundle_root {
                    if exe_path.starts_with(&root) {
                        return true;
                    }
                }
            }
            if let Some(expected) = app.exec_path.file_name().and_then(|s| s.to_str()) {
                return name_lower == expected.to_lowercase();
            }
            false
        }
        AppKind::DesktopEntry => {
            if let Some(exe_path) = exe {
                if exe_path == app.exec_path {
                    return true;
                }
                #[cfg(target_os = "linux")]
                {
                    if is_flatpak_or_snap_wrapper(&app.exec_path) {
                        return matches_sandboxed_app(app, pid);
                    }
                }
            }
            if let Some(expected) = app.exec_path.file_name().and_then(|s| s.to_str()) {
                return comm_matches(name_lower, &expected.to_lowercase());
            }
            false
        }
        AppKind::WindowsExe => {
            if let Some(exe_path) = exe {
                if exe_path == app.exec_path {
                    return true;
                }
            }
            let basename = app
                .exec_path
                .file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if basename.is_empty() {
                return false;
            }
            name_lower == basename
                || exe
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .map(|n| n == basename)
                    .unwrap_or(false)
        }
    }
}

/// Compare a process's `name()` against an expected binary basename.
///
/// On Linux `sysinfo` reads the name from `/proc/<pid>/comm`, which the kernel
/// truncates to 15 bytes (`TASK_COMM_LEN - 1`). A binary called `signal-desktop`
/// fits, but anything longer — e.g. `element-desktop` is fine, yet a 20-char
/// launcher name gets cut. When the expected name exceeds the limit and the
/// observed name is exactly 15 bytes, fall back to a prefix match so the block
/// still fires instead of silently leaking the app. This only relaxes matching
/// for the truncated case; exact comparison is required otherwise.
fn comm_matches(name_lower: &str, expected_lower: &str) -> bool {
    if name_lower == expected_lower {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        const COMM_MAX: usize = 15;
        if expected_lower.len() > COMM_MAX && name_lower.len() == COMM_MAX {
            return expected_lower.as_bytes().starts_with(name_lower.as_bytes());
        }
    }
    false
}

fn bundle_root_of(exec_path: &Path) -> Option<PathBuf> {
    let mut cur = exec_path.parent()?;
    while let Some(parent) = cur.parent() {
        if cur.extension().and_then(|e| e.to_str()) == Some("app") {
            return Some(cur.to_path_buf());
        }
        cur = parent;
    }
    None
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn get_current_uid() -> Option<u32> {
    use nix::unistd::Uid;
    Some(Uid::current().as_raw())
}

#[cfg(target_os = "windows")]
fn get_current_uid() -> Option<u32> {
    None
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn should_consider_process(proc: &sysinfo::Process, current_uid: Option<u32>) -> bool {
    // System-mode daemon runs as root and must reach every user's blocked
    // apps. is_system_path() filters out OS binaries separately, so allowing
    // root to see all PIDs here is safe.
    if current_uid == Some(0) {
        return true;
    }
    if let (Some(proc_uid), Some(current)) = (proc.user_id(), current_uid) {
        if **proc_uid != current {
            return false;
        }
    }
    true
}

#[cfg(target_os = "windows")]
fn should_consider_process(_proc: &sysinfo::Process, _current_uid: Option<u32>) -> bool {
    true
}

fn is_system_path(path: &Path) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        path.starts_with("/usr/bin")
            || path.starts_with("/bin")
            || path.starts_with("/usr/sbin")
            || path.starts_with("/sbin")
            || path.starts_with("/System")
            || path.starts_with("/usr/libexec")
            || path.starts_with("/usr/lib")
            || path.starts_with("/Library/PrivilegedHelperTools")
            || path.starts_with("/Library/LaunchDaemons")
            || path.starts_with("/Library/LaunchAgents")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(path_str) = path.to_str() {
            let path_lower = path_str.to_lowercase();
            path_lower.starts_with("c:\\windows\\")
                || path_lower.starts_with("c:\\program files\\windows")
        } else {
            false
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn is_flatpak_or_snap_wrapper(exec_path: &Path) -> bool {
    exec_path.to_string_lossy().contains("flatpak")
        || exec_path.file_name().and_then(|s| s.to_str()) == Some("snap")
}

#[cfg(target_os = "linux")]
fn matches_sandboxed_app(app: &InstalledApp, pid: sysinfo::Pid) -> bool {
    let cgroup_path = format!("/proc/{}/cgroup", pid.as_u32());
    if let Ok(cgroup) = std::fs::read_to_string(&cgroup_path) {
        let app_id = extract_app_id_from_exec(&app.exec_path);
        if let Some(id) = app_id {
            return cgroup.contains(&format!("app-flatpak-{}", id))
                || cgroup.contains(&format!("snap.{}", id));
        }
    }
    warn!("Failed to match Flatpak/Snap process for app_id: {}", app.id);
    false
}

#[cfg(target_os = "linux")]
fn extract_app_id_from_exec(exec_path: &Path) -> Option<String> {
    let exec_str = exec_path.to_string_lossy();
    if exec_str.contains("flatpak") {
        if let Some(run_pos) = exec_str.find("run") {
            let after_run = &exec_str[run_pos + 3..].trim_start();
            if let Some(first_arg) = after_run.split_whitespace().next() {
                return Some(first_arg.to_string());
            }
        }
    }
    if exec_str.contains("snap") {
        if let Some(first_arg) = exec_str.split_whitespace().nth(1) {
            return Some(first_arg.to_string());
        }
    }
    None
}
