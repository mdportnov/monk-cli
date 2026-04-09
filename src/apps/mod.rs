mod cache;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod resolver;
#[cfg(target_os = "windows")]
mod windows;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::Result;

pub use cache::{load_or_scan, AppCache, CACHE_TTL};
pub use resolver::{resolve, Resolution};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    MacBundle,
    DesktopEntry,
    WindowsExe,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledApp {
    pub id: String,
    pub label: String,
    pub exec_path: PathBuf,
    pub kind: AppKind,
}

impl InstalledApp {
    pub fn exec_basename(&self) -> String {
        self.exec_path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default()
    }
}

pub fn scan() -> Result<Vec<InstalledApp>> {
    #[cfg(target_os = "macos")]
    {
        macos::scan()
    }
    #[cfg(target_os = "linux")]
    {
        linux::scan()
    }
    #[cfg(target_os = "windows")]
    {
        windows::scan()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Ok(Vec::new())
    }
}

pub fn dedup_sorted(mut apps: Vec<InstalledApp>) -> Vec<InstalledApp> {
    apps.sort_by(|a, b| a.id.cmp(&b.id));
    apps.dedup_by(|a, b| a.id == b.id);
    apps.sort_by(|a, b| a.label.to_lowercase().cmp(&b.label.to_lowercase()));
    apps
}
