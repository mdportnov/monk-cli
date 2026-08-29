use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{paths, Result};

use super::InstalledApp;

pub const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24);
const CACHE_FILE: &str = "apps.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppCache {
    pub scanned_at: DateTime<Utc>,
    pub apps: Vec<InstalledApp>,
}

/// Where the shared cache lives — the copy the daemon reads.
fn shared_path() -> Result<PathBuf> {
    Ok(paths::data_dir()?.join(CACHE_FILE))
}

/// Per-user copy, used only in system mode where the shared data dir is
/// root-owned and a user's CLI cannot write it. Without this every command
/// rescans every installed application.
fn user_path() -> Option<PathBuf> {
    if !needs_user_cache() {
        return None;
    }
    paths::user_cache_dir().ok().map(|d| d.join(CACHE_FILE))
}

#[cfg(unix)]
fn needs_user_cache() -> bool {
    paths::system_mode() && !nix::unistd::geteuid().is_root()
}

#[cfg(not(unix))]
fn needs_user_cache() -> bool {
    false
}

fn read_at(path: &std::path::Path) -> Option<AppCache> {
    let raw = fs_err::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn write_at(cache: &AppCache, path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    let raw = serde_json::to_vec_pretty(cache).map_err(crate::Error::from)?;
    fs_err::write(path, raw)?;
    Ok(())
}

impl AppCache {
    /// Newest of the copies we can see. A stale shared cache must never win
    /// over a fresh per-user one, hence the comparison instead of a
    /// first-hit-wins lookup.
    pub fn load() -> Result<Option<Self>> {
        let mut best: Option<Self> = None;
        for path in [shared_path().ok(), user_path()].into_iter().flatten() {
            let Some(cache) = read_at(&path) else { continue };
            if best.as_ref().is_none_or(|b| cache.scanned_at > b.scanned_at) {
                best = Some(cache);
            }
        }
        Ok(best)
    }

    /// Write the shared copy, falling back to the per-user one when the
    /// daemon owns the data dir.
    pub fn save(&self) -> Result<()> {
        let shared = shared_path()?;
        match write_at(self, &shared) {
            Ok(()) => Ok(()),
            Err(e) => match user_path() {
                Some(user) => {
                    tracing::debug!(?e, "shared app cache not writable; using the per-user copy");
                    write_at(self, &user)
                }
                None => Err(e),
            },
        }
    }

    pub fn is_stale(&self) -> bool {
        let scanned = SystemTime::from(self.scanned_at);
        SystemTime::now().duration_since(scanned).map(|d| d > CACHE_TTL).unwrap_or(true)
    }

    pub fn refresh_now(apps: Vec<InstalledApp>) -> Self {
        Self { scanned_at: Utc::now(), apps }
    }
}

pub fn load_or_scan(force: bool) -> Result<AppCache> {
    if !force {
        if let Some(cache) = AppCache::load()? {
            if !cache.is_stale() {
                return Ok(cache);
            }
        }
    }
    let apps = super::dedup_sorted(super::scan()?);
    let cache = AppCache::refresh_now(apps);
    if let Err(e) = cache.save() {
        tracing::warn!(?e, "could not persist the app cache — the next command will rescan");
    }
    Ok(cache)
}
