use super::{AppCache, InstalledApp};

#[derive(Debug, Clone)]
pub struct Resolution {
    pub resolved: Vec<InstalledApp>,
    pub stale: Vec<String>,
}

pub fn resolve(ids: &[String], cache: &AppCache) -> Resolution {
    let mut resolved = Vec::new();
    let mut stale = Vec::new();
    for id in ids {
        match cache.apps.iter().find(|a| &a.id == id) {
            Some(app) => resolved.push(app.clone()),
            None => stale.push(id.clone()),
        }
    }
    Resolution { resolved, stale }
}
