use std::path::{Path, PathBuf};

use crate::Result;

use super::{AppKind, InstalledApp};

pub fn scan() -> Result<Vec<InstalledApp>> {
    let mut out = Vec::new();
    for root in start_menu_roots() {
        walk_lnks(&root, &mut out);
    }
    Ok(out)
}

fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("AppData") {
        roots.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    roots
}

fn walk_lnks(dir: &Path, out: &mut Vec<InstalledApp>) {
    if !dir.exists() {
        return;
    }
    let entries = match fs_err::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_lnks(&path, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()).map(|e| e.eq_ignore_ascii_case("lnk"))
            != Some(true)
        {
            continue;
        }
        if let Some(app) = parse_lnk(&path) {
            out.push(app);
        }
    }
}

fn parse_lnk(path: &Path) -> Option<InstalledApp> {
    let shortcut = lnk::ShellLink::open(path).ok()?;
    let target = shortcut
        .link_info()
        .as_ref()
        .and_then(|info| info.local_base_path().clone())
        .or_else(|| shortcut.string_data().relative_path().clone())?;
    let exec_path = PathBuf::from(&target);
    if exec_path.extension().and_then(|s| s.to_str()).map(|e| e.eq_ignore_ascii_case("exe"))
        != Some(true)
    {
        return None;
    }
    let label = path.file_stem()?.to_string_lossy().to_string();
    let id = exec_path.file_name()?.to_string_lossy().to_lowercase();
    Some(InstalledApp { id, label, exec_path, kind: AppKind::WindowsExe })
}
