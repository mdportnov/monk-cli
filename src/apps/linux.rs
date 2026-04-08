use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use crate::Result;

use super::{AppKind, InstalledApp};

pub fn scan() -> Result<Vec<InstalledApp>> {
    let mut out = Vec::new();
    for root in desktop_roots() {
        walk_desktop(&root, &mut out);
    }
    Ok(out)
}

fn desktop_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
        PathBuf::from("/var/lib/snapd/desktop/applications"),
    ];
    if let Some(dirs) = directories::BaseDirs::new() {
        roots.push(dirs.home_dir().join(".local/share/applications"));
        roots.push(dirs.home_dir().join(".local/share/flatpak/exports/share/applications"));
    }
    roots
}

fn walk_desktop(dir: &Path, out: &mut Vec<InstalledApp>) {
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
            walk_desktop(&path, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        if let Some(app) = parse_desktop(&path) {
            out.push(app);
        }
    }
}

fn parse_desktop(path: &Path) -> Option<InstalledApp> {
    let raw = fs_err::read_to_string(path).ok()?;
    let entries = parse_ini_section(&raw, "Desktop Entry");
    if entries.get("NoDisplay").map(|v| v == "true").unwrap_or(false) {
        return None;
    }
    if entries.get("Type").map(|v| v.as_str()) != Some("Application") {
        return None;
    }
    let name = entries.get("Name")?.clone();
    let exec_raw = entries.get("TryExec").or_else(|| entries.get("Exec"))?;
    let exec_path = resolve_exec(exec_raw)?;
    let id = path.file_stem()?.to_string_lossy().to_string();
    Some(InstalledApp { id, label: name, exec_path, kind: AppKind::DesktopEntry })
}

fn parse_ini_section(raw: &str, section: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut in_section = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_section = name == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

fn resolve_exec(raw: &str) -> Option<PathBuf> {
    let first = raw.split_whitespace().next()?;
    let cleaned: String = first.chars().filter(|c| *c != '"').collect();
    if cleaned.starts_with('/') {
        return Some(PathBuf::from(cleaned));
    }
    for dir in std::env::var("PATH").unwrap_or_default().split(':') {
        let candidate = Path::new(dir).join(&cleaned);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    Some(PathBuf::from(cleaned))
}
