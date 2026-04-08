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

impl AppCache {
    pub fn load() -> Result<Option<Self>> {
        let path = paths::data_dir()?.join(CACHE_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs_err::read_to_string(&path)?;
        let cache: Self = serde_json::from_str(&raw).map_err(crate::Error::from)?;
        Ok(Some(cache))
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::data_dir()?.join(CACHE_FILE);
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let raw = serde_json::to_vec_pretty(self).map_err(crate::Error::from)?;
        fs_err::write(&path, raw)?;
        Ok(())
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
    let _ = cache.save();
    Ok(cache)
}
