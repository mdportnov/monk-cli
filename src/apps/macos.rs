use std::path::{Path, PathBuf};

use crate::Result;

use super::{AppKind, InstalledApp};

const SEARCH_ROOTS: &[&str] = &["/Applications", "/System/Applications"];

pub fn scan() -> Result<Vec<InstalledApp>> {
    let mut out = Vec::new();
    let mut roots: Vec<PathBuf> = SEARCH_ROOTS.iter().map(PathBuf::from).collect();
    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
        roots.push(home.join("Applications"));
    }
    for root in roots {
        walk_bundles(&root, 0, &mut out);
    }
    Ok(out)
}

fn walk_bundles(dir: &Path, depth: usize, out: &mut Vec<InstalledApp>) {
    if depth > 3 || !dir.exists() {
        return;
    }
    let entries = match fs_err::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("app") {
            if let Some(app) = parse_bundle(&path) {
                out.push(app);
            }
        } else if path.is_dir() && !path.file_name().and_then(|n| n.to_str()).unwrap_or("").starts_with('.') {
            walk_bundles(&path, depth + 1, out);
        }
    }
}

fn parse_bundle(bundle: &Path) -> Option<InstalledApp> {
    let plist_path = bundle.join("Contents").join("Info.plist");
    if !plist_path.exists() {
        return None;
    }
    let value = plist::Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;
    let bundle_id = dict.get("CFBundleIdentifier").and_then(|v| v.as_string())?.to_string();
    let exec_name = dict
        .get("CFBundleExecutable")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .unwrap_or_else(|| bundle.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string());
    let label = dict
        .get("CFBundleDisplayName")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .or_else(|| dict.get("CFBundleName").and_then(|v| v.as_string()).map(|s| s.to_string()))
        .unwrap_or_else(|| bundle.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string());
    let exec_path = bundle.join("Contents").join("MacOS").join(&exec_name);
    Some(InstalledApp { id: bundle_id, label, exec_path, kind: AppKind::MacBundle })
}
