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
    // Prefer the full `Exec` line for sandbox detection — `TryExec` is only a
    // single binary path and never carries the flatpak/snap app id.
    let exec_full = entries.get("Exec");
    let exec_raw = entries.get("TryExec").or(exec_full)?;
    let exec_path = resolve_exec(exec_raw)?;
    let id = path.file_stem()?.to_string_lossy().to_string();
    let sandbox_id = exec_full.and_then(|e| parse_sandbox_id(e));
    Some(InstalledApp { id, label: name, exec_path, kind: AppKind::DesktopEntry, sandbox_id })
}

/// Extract the Flatpak/Snap application id from a `.desktop` `Exec=` line.
///
/// `resolve_exec` keeps only the launcher binary (`flatpak` / `snap`), so the
/// real id — needed to match the sandboxed process via its cgroup — must be
/// recovered from the argument list here.
///
/// - Flatpak: `flatpak run [OPTIONS] APP_ID [ARGS]` — `APP_ID` is the first
///   argument after `run` that is not an option (`-x` / `--opt` / `--opt=val`).
/// - Snap:    `snap run NAME` or a `/snap/bin/NAME` launcher.
fn parse_sandbox_id(exec_raw: &str) -> Option<String> {
    let tokens: Vec<&str> = exec_raw.split_whitespace().filter(|t| !t.is_empty()).collect();
    let launcher = tokens.first()?;
    let launcher_name =
        Path::new(launcher).file_name().and_then(|s| s.to_str()).unwrap_or(launcher);

    if launcher_name == "flatpak" {
        let run_idx = tokens.iter().position(|t| *t == "run")?;
        return tokens[run_idx + 1..]
            .iter()
            .find(|t| !t.starts_with('-'))
            .map(|t| t.trim_matches('"').to_string());
    }

    if launcher_name == "snap" {
        // `snap run NAME ...`
        let run_idx = tokens.iter().position(|t| *t == "run")?;
        return tokens.get(run_idx + 1).map(|t| t.trim_matches('"').to_string());
    }

    // `/snap/bin/NAME` direct launcher.
    if launcher.starts_with("/snap/bin/") || launcher.starts_with("/var/lib/snapd/snap/bin/") {
        return Path::new(launcher).file_name().and_then(|s| s.to_str()).map(|s| s.to_string());
    }

    None
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

#[cfg(test)]
mod tests {
    use super::parse_sandbox_id;

    #[test]
    fn parses_flatpak_id_past_options() {
        assert_eq!(
            parse_sandbox_id("flatpak run --branch=stable --arch=x86_64 org.mozilla.firefox %U")
                .as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            parse_sandbox_id("/usr/bin/flatpak run --filesystem=home org.telegram.desktop")
                .as_deref(),
            Some("org.telegram.desktop")
        );
        assert_eq!(
            parse_sandbox_id("flatpak run com.spotify.Client").as_deref(),
            Some("com.spotify.Client")
        );
    }

    #[test]
    fn parses_snap_id() {
        assert_eq!(parse_sandbox_id("snap run chromium").as_deref(), Some("chromium"));
        assert_eq!(parse_sandbox_id("/snap/bin/slack").as_deref(), Some("slack"));
    }

    #[test]
    fn native_and_incomplete_lines_have_no_sandbox_id() {
        assert_eq!(parse_sandbox_id("/usr/bin/firefox %u"), None);
        assert_eq!(parse_sandbox_id("flatpak"), None);
        assert_eq!(parse_sandbox_id("flatpak run"), None);
        assert_eq!(parse_sandbox_id("flatpak run --gpu"), None);
    }
}
