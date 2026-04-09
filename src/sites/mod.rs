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

pub fn sanitize_host(raw: &str) -> Option<String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let s = s.split('/').next()?.trim_end_matches('.');
    if s.is_empty() || s.contains(char::is_whitespace) || !s.contains('.') {
        return None;
    }
    Some(s.to_string())
}

fn load_namespace(namespace: &str, raw: &str) -> Result<Vec<SiteGroup>> {
    let parsed: RawRegistry = toml::from_str(raw)
        .map_err(|e| Error::Config(format!("site registry `{namespace}`: {e}")))?;
    let mut groups = Vec::new();
    for (id, cat) in parsed.categories {
        let mut hosts: BTreeSet<String> = BTreeSet::new();
        for domain in &cat.domains {
            let Some(domain) = sanitize_host(domain) else { continue };
            hosts.insert(domain.clone());
            let mut subs = parsed.default_subdomains.clone();
            if let Some(extra) = cat.extra_subdomains.get(&domain) {
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
    fn registry_has_all_expected_categories() {
        let groups = all_groups().unwrap();
        let names: BTreeSet<String> = groups.iter().map(|g| g.qualified()).collect();
        for q in [
            "global.social",
            "global.video",
            "global.news",
            "global.chat",
            "global.shopping",
            "global.games",
            "global.adult",
            "global.ai",
            "global.productivity_trap",
            "global.dating",
            "global.crypto",
            "global.betting",
            "global.finance_doom",
            "ru.social",
            "ru.video",
            "ru.news",
            "ru.chat",
            "ru.shopping",
            "ru.games",
            "ru.productivity_trap",
            "ru.ai",
            "ru.adult",
            "ru.dating",
            "ru.crypto",
            "ru.betting",
        ] {
            assert!(names.contains(q), "missing category: {q}");
        }
    }

    #[test]
    fn no_duplicate_hosts_within_group() {
        for g in all_groups().unwrap() {
            let uniq: BTreeSet<_> = g.hosts.iter().collect();
            assert_eq!(g.hosts.len(), uniq.len(), "duplicate hosts in {}", g.qualified());
        }
    }

    #[test]
    fn all_loaded_hosts_are_clean() {
        for g in all_groups().unwrap() {
            for h in &g.hosts {
                assert!(!h.contains('/'), "host has slash: {h} in {}", g.qualified());
                assert!(!h.contains(char::is_whitespace), "host has whitespace: {h}");
                assert_eq!(h, &h.to_ascii_lowercase(), "host not lowercase: {h}");
            }
        }
    }

    #[test]
    fn sanitize_strips_paths_and_lowercases() {
        assert_eq!(sanitize_host("BING.com/chat"), Some("bing.com".into()));
        assert_eq!(sanitize_host("vk.com/im "), Some("vk.com".into()));
        assert_eq!(sanitize_host("nodot"), None);
        assert_eq!(sanitize_host(""), None);
    }

    #[test]
    fn expands_groups_flattens_and_dedups() {
        let expanded =
            expand_groups(&["global.social".to_string(), "global.social".to_string()]).unwrap();
        let uniq: BTreeSet<_> = expanded.iter().collect();
        assert_eq!(expanded.len(), uniq.len());
    }
}
