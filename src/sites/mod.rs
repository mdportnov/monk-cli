use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::{Error, Result};

const GLOBAL_TOML: &str = include_str!("../../assets/sites/global.toml");
const RU_TOML: &str = include_str!("../../assets/sites/ru.toml");

#[derive(Debug, Deserialize)]
struct RawRegistry {
    #[serde(default)]
    default_subdomains: Vec<String>,
    #[serde(flatten)]
    categories: BTreeMap<String, RawCategory>,
}

#[derive(Debug, Deserialize)]
struct RawCategory {
    #[serde(default)]
    label: String,
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    extra_subdomains: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct SiteGroup {
    pub id: String,
    pub label: String,
    pub namespace: String,
    pub hosts: Vec<String>,
}

impl SiteGroup {
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.namespace, self.id)
    }
}

pub fn all_groups() -> Result<Vec<SiteGroup>> {
    let mut out = Vec::new();
    out.extend(load_namespace("global", GLOBAL_TOML)?);
    out.extend(load_namespace("ru", RU_TOML)?);
    Ok(out)
}

pub fn expand_groups(qualified_ids: &[String]) -> Result<Vec<String>> {
    let groups = all_groups()?;
    let mut out: BTreeSet<String> = BTreeSet::new();
    for q in qualified_ids {
        if let Some(group) = groups.iter().find(|g| &g.qualified() == q) {
            for host in &group.hosts {
                out.insert(host.clone());
            }
        }
    }
    Ok(out.into_iter().collect())
}

fn load_namespace(namespace: &str, raw: &str) -> Result<Vec<SiteGroup>> {
    let parsed: RawRegistry =
        toml::from_str(raw).map_err(|e| Error::Config(format!("site registry `{namespace}`: {e}")))?;
    let mut groups = Vec::new();
    for (id, cat) in parsed.categories {
        let mut hosts: BTreeSet<String> = BTreeSet::new();
        for domain in &cat.domains {
            let domain = domain.trim();
            if domain.is_empty() {
                continue;
            }
            hosts.insert(domain.to_string());
            let mut subs = parsed.default_subdomains.clone();
            if let Some(extra) = cat.extra_subdomains.get(domain) {
                subs.extend(extra.iter().cloned());
            }
            for sub in subs {
                if sub.is_empty() || domain.starts_with(&format!("{sub}.")) {
                    continue;
                }
                hosts.insert(format!("{sub}.{domain}"));
            }
        }
        groups.push(SiteGroup {
            id,
            label: cat.label,
            namespace: namespace.to_string(),
            hosts: hosts.into_iter().collect(),
        });
    }
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_both_namespaces() {
        let groups = all_groups().unwrap();
        assert!(groups.iter().any(|g| g.qualified() == "global.social"));
        assert!(groups.iter().any(|g| g.qualified() == "ru.news"));
    }

    #[test]
    fn expands_subdomains() {
        let groups = all_groups().unwrap();
        let social = groups.iter().find(|g| g.qualified() == "global.social").unwrap();
        assert!(social.hosts.iter().any(|h| h == "facebook.com"));
        assert!(social.hosts.iter().any(|h| h == "www.facebook.com"));
        assert!(social.hosts.iter().any(|h| h == "mbasic.facebook.com"));
    }

    #[test]
    fn expands_groups_flattens_and_dedups() {
        let expanded =
            expand_groups(&["global.social".to_string(), "global.social".to_string()]).unwrap();
        let uniq: BTreeSet<_> = expanded.iter().collect();
        assert_eq!(expanded.len(), uniq.len());
    }
}
