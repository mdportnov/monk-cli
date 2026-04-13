use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Error, Result};

const GLOBAL_TOML: &str = include_str!("../../assets/brands/global.toml");
const RU_TOML: &str = include_str!("../../assets/brands/ru.toml");

#[derive(Debug, Deserialize)]
struct RawRegistry {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    brands: Vec<RawBrand>,
}

#[derive(Debug, Deserialize)]
struct RawBrand {
    id: String,
    name: String,
    category: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    apps: AppIds,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct AppIds {
    #[serde(default)]
    pub macos: Vec<String>,
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
    #[serde(default)]
    pub ios: Vec<String>,
    #[serde(default)]
    pub android: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Brand {
    pub id: String,
    pub namespace: String,
    pub name: String,
    pub category: String,
    pub icon: Option<String>,
    pub aliases: Vec<String>,
    pub domains: Vec<String>,
    pub apps: AppIds,
}

impl Brand {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.namespace, self.id)
    }
    pub fn current_platform_apps(&self) -> &[String] {
        #[cfg(target_os = "macos")]
        {
            &self.apps.macos
        }
        #[cfg(target_os = "windows")]
        {
            &self.apps.windows
        }
        #[cfg(target_os = "linux")]
        {
            &self.apps.linux
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            &[]
        }
    }
}

pub fn all_brands() -> Result<Vec<Brand>> {
    let mut out = Vec::new();
    out.extend(load_namespace("global", GLOBAL_TOML)?);
    out.extend(load_namespace("ru", RU_TOML)?);
    Ok(out)
}

#[derive(Debug, Default)]
pub struct Resolved {
    pub domains: BTreeSet<String>,
    pub apps: BTreeSet<String>,
    pub unknown: Vec<String>,
}

pub fn resolve(qualified_ids: &[String]) -> Result<Resolved> {
    let brands = all_brands()?;
    let by_q: BTreeMap<String, &Brand> = brands.iter().map(|b| (b.qualified(), b)).collect();
    let mut out = Resolved::default();
    for q in qualified_ids {
        if let Some(b) = by_q.get(q) {
            for d in &b.domains {
                out.domains.insert(d.clone());
            }
            for a in b.current_platform_apps() {
                out.apps.insert(a.clone());
            }
        } else {
            out.unknown.push(q.clone());
        }
    }
    Ok(out)
}

fn load_namespace(namespace: &str, raw: &str) -> Result<Vec<Brand>> {
    let parsed: RawRegistry = toml::from_str(raw)
        .map_err(|e| Error::Config(format!("brand registry `{namespace}`: {e}")))?;
    if parsed.version == 0 {
        return Err(Error::Config(format!("brand registry `{namespace}` missing version")));
    }
    let mut out = Vec::with_capacity(parsed.brands.len());
    for b in parsed.brands {
        out.push(Brand {
            id: b.id,
            namespace: namespace.to_string(),
            name: b.name,
            category: b.category,
            icon: b.icon,
            aliases: b.aliases,
            domains: b
                .domains
                .into_iter()
                .filter_map(|d| crate::sites::sanitize_host(&d))
                .collect(),
            apps: b.apps,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_both_namespaces() {
        let brands = all_brands().unwrap();
        assert!(!brands.is_empty());
        assert!(brands.iter().any(|b| b.namespace == "global"));
        assert!(brands.iter().any(|b| b.namespace == "ru"));
    }

    #[test]
    fn ids_unique_per_namespace() {
        let brands = all_brands().unwrap();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for b in &brands {
            let q = b.qualified();
            assert!(seen.insert(q.clone()), "duplicate brand id: {q}");
        }
    }

    #[test]
    fn resolve_returns_domains_and_apps() {
        let r = resolve(&["global.instagram".into()]).unwrap();
        assert!(!r.domains.is_empty());
    }

    #[test]
    fn resolve_known_brand_includes_canonical_domain() {
        let r = resolve(&["global.instagram".into()]).unwrap();
        assert!(r.domains.contains("instagram.com"));
        assert!(r.unknown.is_empty());
    }

    #[test]
    fn resolve_records_unknown() {
        let r = resolve(&["global:nonsense_xyz".into()]).unwrap();
        assert_eq!(r.unknown.len(), 1);
    }
}
