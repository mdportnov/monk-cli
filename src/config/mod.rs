use std::{collections::BTreeMap, path::Path, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{paths, Error, Result};

pub const CURRENT_SCHEMA: u32 = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HardModeLevel {
    #[default]
    Off,
    Hard,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct General {
    #[serde(default)]
    pub initialized: bool,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default = "default_hard_mode")]
    pub hard_mode: bool,
    #[serde(default = "default_default_profile")]
    pub default_profile: String,
    #[serde(default = "default_duration", with = "humantime_serde")]
    pub default_duration: Duration,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub hard_mode_level: HardModeLevel,
    #[serde(default = "default_panic_delay", with = "humantime_serde")]
    pub panic_delay: Duration,
    #[serde(default = "default_tamper_penalty", with = "humantime_serde")]
    pub tamper_penalty: Duration,
}

impl Default for General {
    fn default() -> Self {
        Self {
            initialized: false,
            locale: None,
            hard_mode: default_hard_mode(),
            default_profile: default_default_profile(),
            default_duration: default_duration(),
            autostart: false,
            hard_mode_level: HardModeLevel::Off,
            panic_delay: default_panic_delay(),
            tamper_penalty: default_tamper_penalty(),
        }
    }
}

fn default_panic_delay() -> Duration {
    Duration::from_secs(15 * 60)
}
fn default_tamper_penalty() -> Duration {
    Duration::from_secs(15 * 60)
}

fn default_hard_mode() -> bool {
    false
}
fn default_default_profile() -> String {
    "deepwork".into()
}
fn default_duration() -> Duration {
    Duration::from_secs(25 * 60)
}

mod humantime_serde {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&humantime::format_duration(*d).to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        let raw = String::deserialize(d)?;
        humantime::parse_duration(&raw).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    #[serde(default)]
    pub sites: Vec<String>,
    #[serde(default)]
    pub site_groups: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub hooks: Hooks,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default, with = "humantime_serde_opt")]
    pub max_duration: Option<Duration>,
    #[serde(default, with = "humantime_serde_opt")]
    pub min_duration: Option<Duration>,
    #[serde(default, with = "humantime_serde_opt")]
    pub cooldown: Option<Duration>,
    #[serde(default, with = "humantime_serde_opt")]
    pub daily_cap: Option<Duration>,
}

mod humantime_serde_opt {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(v) => s.serialize_str(&humantime::format_duration(*v).to_string()),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        let raw = Option::<String>::deserialize(d)?;
        match raw {
            Some(s) => humantime::parse_duration(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    #[serde(default)]
    pub before: Vec<String>,
    #[serde(default)]
    pub after: Vec<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = paths::config_file()?;
        if !path.exists() {
            let cfg = Self::default();
            cfg.save_to(&path)?;
            return Ok(cfg);
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let raw = fs_err::read_to_string(path)?;
        let mut cfg: Self = toml::from_str(&raw)?;
        cfg.migrate()?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::config_file()?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        fs_err::write(path, raw)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if !self.profiles.is_empty() && !self.profiles.contains_key(&self.general.default_profile) {
            return Err(Error::Config(format!(
                "default_profile `{}` is not present in profiles",
                self.general.default_profile
            )));
        }
        for (name, p) in &self.profiles {
            if name.is_empty() {
                return Err(Error::Config("profile name cannot be empty".into()));
            }
            for site in &p.sites {
                if site.contains(char::is_whitespace) {
                    return Err(Error::Config(format!("invalid host in profile `{name}`: {site}")));
                }
            }
        }
        Ok(())
    }

    fn migrate(&mut self) -> Result<()> {
        if self.schema_version > CURRENT_SCHEMA {
            return Err(Error::Config(format!(
                "config schema {} is newer than supported {CURRENT_SCHEMA}",
                self.schema_version
            )));
        }
        if self.schema_version == 0 {
            self.schema_version = 1;
        }
        if self.schema_version < 2 {
            self.schema_version = 2;
        }
        if self.schema_version < 3 {
            let mut cleared = 0usize;
            for p in self.profiles.values_mut() {
                if !p.apps.is_empty() {
                    cleared += p.apps.len();
                    p.apps.clear();
                }
            }
            if cleared > 0 {
                tracing::warn!(
                    cleared,
                    "config v3 migration: dropped legacy `apps` entries — re-select via `monk profile edit`"
                );
            }
            self.schema_version = 3;
        }
        if self.schema_version < 4 {
            self.schema_version = 4;
        }
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }
}
