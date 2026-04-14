use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use sysinfo::{ProcessesToUpdate, System};
use tracing::{info, warn};

use crate::{
    apps::{AppKind, InstalledApp},
    Result,
};

#[derive(Debug)]
pub struct ProcessGuard {
    sys: System,
    last_refresh: Instant,
}

impl Default for ProcessGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessGuard {
    pub fn new() -> Self {
        use std::time::Duration;
        Self { sys: System::new(), last_refresh: Instant::now() - Duration::from_secs(10) }
    }

    pub fn kill_matching(&mut self, apps: &[InstalledApp]) -> Result<usize> {
        if apps.is_empty() {
            return Ok(0);
        }

        let current_uid = get_current_uid();
        let mut total_killed = 0;

        for attempt in 0..2 {
            if attempt == 0 {
                let now = Instant::now();
                if now.duration_since(self.last_refresh).as_secs() >= 5 {
                    self.sys.refresh_processes(ProcessesToUpdate::All, true);
                    self.last_refresh = now;
                }
            } else {
                self.sys.refresh_processes(ProcessesToUpdate::All, true);
                self.last_refresh = Instant::now();
            }

            let mut to_kill = Vec::new();

            for proc in self.sys.processes().values() {
                if !should_consider_process(proc, current_uid) {
                    continue;
                }

                let exe = proc.exe().map(Path::to_path_buf);
                let name = proc.name().to_string_lossy().to_lowercase();

                if let Some(app) = apps.iter().find(|app| matches_process(app, exe.as_deref(), &name)) {
                    if let Some(exe_path) = &exe {
                        if is_system_path(exe_path) {
                            warn!(
                                "Skipping system process: pid={}, name={:?}, path={:?}",
                                proc.pid(),
                                proc.name(),
                                exe_path
                            );
                            continue;
                        }
                    }

                    to_kill.push((proc.pid(), proc.name().to_string_lossy().to_string(), exe.clone(), app.id.clone()));
                }
            }

            let killed = tokio::task::block_in_place(|| {
                let mut count = 0;
                for (pid, name, exe, app_id) in to_kill {
                    if let Some(proc) = self.sys.process(pid) {
                        if proc.kill() {
                            info!(
                                "Killed process: pid={}, name={}, app_id={}, path={:?}",
                                pid, name, app_id, exe
                            );
                            count += 1;
                        }
                    }
                }
                count
            });

            total_killed += killed;
            if killed == 0 {
                break;
            }
        }
        Ok(total_killed)
    }
}

fn matches_process(app: &InstalledApp, exe: Option<&Path>, name_lower: &str) -> bool {
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
            }
            if let Some(expected) = app.exec_path.file_name().and_then(|s| s.to_str()) {
                return name_lower == expected.to_lowercase();
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
        path.starts_with("/usr/bin") ||
        path.starts_with("/bin") ||
        path.starts_with("/usr/sbin") ||
        path.starts_with("/sbin") ||
        path.starts_with("/System") ||
        path.starts_with("/usr/libexec")
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(path_str) = path.to_str() {
            let path_lower = path_str.to_lowercase();
            path_lower.starts_with("c:\\windows\\") ||
            path_lower.starts_with("c:\\program files\\windows")
        } else {
            false
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}
