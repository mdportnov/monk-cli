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
    pub brands: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    #[serde(default = "default_sched_enabled")]
    pub enabled: bool,
    pub days: Vec<Weekday>,
    pub start: String,
    pub end: String,
    #[serde(default = "default_tz")]
    pub tz: String,
}

fn default_sched_enabled() -> bool {
    true
}
fn default_tz() -> String {
    "local".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
    pub fn from_chrono(w: chrono::Weekday) -> Self {
        match w {
            chrono::Weekday::Mon => Self::Mon,
            chrono::Weekday::Tue => Self::Tue,
            chrono::Weekday::Wed => Self::Wed,
            chrono::Weekday::Thu => Self::Thu,
            chrono::Weekday::Fri => Self::Fri,
            chrono::Weekday::Sat => Self::Sat,
            chrono::Weekday::Sun => Self::Sun,
        }
    }
}

impl Schedule {
    pub fn mask(&self) -> u8 {
        self.days.iter().fold(0u8, |m, d| m | d.bit())
    }
    pub fn parse_hhmm(s: &str) -> Result<(u32, u32)> {
        let (h, m) = s
            .split_once(':')
            .ok_or_else(|| Error::Config(format!("invalid HH:MM `{s}`")))?;
        let h: u32 = h.parse().map_err(|_| Error::Config(format!("invalid hour `{s}`")))?;
        let m: u32 = m.parse().map_err(|_| Error::Config(format!("invalid minute `{s}`")))?;
        if h > 23 || m > 59 {
            return Err(Error::Config(format!("out of range `{s}`")));
        }
        Ok((h, m))
    }
    pub fn validate(&self) -> Result<()> {
        if self.days.is_empty() {
            return Err(Error::Config("schedule.days must not be empty".into()));
        }
        Self::parse_hhmm(&self.start)?;
        Self::parse_hhmm(&self.end)?;
        if self.tz != "local" {
            self.tz
                .parse::<chrono_tz::Tz>()
                .map_err(|e| Error::Config(format!("bad tz `{}`: {e}", self.tz)))?;
        }
        Ok(())
    }
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
        let tmp = path.with_extension("toml.tmp");
        fs_err::write(&tmp, raw)?;
        fs_err::rename(&tmp, path)?;
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
            if let Some(sch) = &p.schedule {
                sch.validate().map_err(|e| {
                    Error::Config(format!("profile `{name}` schedule: {e}"))
                })?;
            }
            for b in &p.brands {
                if b.split_once('.').filter(|(ns, id)| !ns.is_empty() && !id.is_empty()).is_none() {
                    return Err(Error::Config(format!(
                        "profile `{name}` brand `{b}` must be `<namespace>.<id>`"
                    )));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_roundtrip() {
        let s = Schedule {
            enabled: true,
            days: vec![Weekday::Mon, Weekday::Wed, Weekday::Fri],
            start: "09:00".into(),
            end: "17:30".into(),
            tz: "Europe/Berlin".into(),
        };
        let toml_str = toml::to_string(&s).unwrap();
        let back: Schedule = toml::from_str(&toml_str).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.mask(), 0b0010101);
        back.validate().unwrap();
    }

    #[test]
    fn schedule_rejects_empty_days() {
        let s = Schedule {
            enabled: true,
            days: vec![],
            start: "09:00".into(),
            end: "10:00".into(),
            tz: "local".into(),
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn config_rejects_bad_brand_id() {
        let mut cfg = Config::default();
        let mut p = Profile::default();
        p.brands.push("badformat".into());
        cfg.profiles.insert("focus".into(), p);
        cfg.general.default_profile = "focus".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn schedule_rejects_bad_time() {
        let s = Schedule {
            enabled: true,
            days: vec![Weekday::Mon],
            start: "25:00".into(),
            end: "17:00".into(),
            tz: "local".into(),
        };
        assert!(s.validate().is_err());
    }
}
