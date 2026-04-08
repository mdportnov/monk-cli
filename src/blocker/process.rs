use std::path::{Path, PathBuf};

use sysinfo::{ProcessesToUpdate, System};

use crate::{
    apps::{AppKind, InstalledApp},
    Result,
};

#[derive(Debug, Default)]
pub struct ProcessGuard {
    sys: System,
}

impl ProcessGuard {
    pub fn new() -> Self {
        Self { sys: System::new() }
    }

    pub fn kill_matching(&mut self, apps: &[InstalledApp]) -> Result<usize> {
        if apps.is_empty() {
            return Ok(0);
        }
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let mut killed = 0;
        for proc in self.sys.processes().values() {
            let exe = proc.exe().map(Path::to_path_buf);
            let name = proc.name().to_string_lossy().to_lowercase();
            if apps.iter().any(|app| matches_process(app, exe.as_deref(), &name)) && proc.kill() {
                killed += 1;
            }
        }
        Ok(killed)
    }
}

fn matches_process(app: &InstalledApp, exe: Option<&Path>, name_lower: &str) -> bool {
    match app.kind {
        AppKind::MacBundle => {
            let bundle_root = bundle_root_of(&app.exec_path);
            if let (Some(exe), Some(root)) = (exe, bundle_root) {
                if exe.starts_with(&root) {
                    return true;
                }
            }
            if let Some(expected) = app.exec_path.file_name().and_then(|s| s.to_str()) {
                return name_lower == expected.to_lowercase();
            }
            false
        }
        AppKind::DesktopEntry => {
            if let Some(exe) = exe {
                if exe == app.exec_path {
                    return true;
                }
            }
            if let Some(expected) = app.exec_path.file_name().and_then(|s| s.to_str()) {
                return name_lower == expected.to_lowercase();
            }
            false
        }
        AppKind::WindowsExe => {
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
