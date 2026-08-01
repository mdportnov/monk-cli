use std::borrow::Cow;

use crate::{config::Profile, i18n, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetTier {
    Scenario,
    Surgical,
    Maximum,
}

impl PresetTier {
    pub fn label_key(self) -> &'static str {
        match self {
            PresetTier::Scenario => "onboarding.preset_tier_scenario",
            PresetTier::Surgical => "onboarding.preset_tier_surgical",
            PresetTier::Maximum => "onboarding.preset_tier_maximum",
        }
    }
    pub fn glyph(self) -> &'static str {
        match self {
            PresetTier::Scenario => "◆",
            PresetTier::Surgical => "•",
            PresetTier::Maximum => "⬣",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PresetMeta {
    pub id: &'static str,
    pub tier: PresetTier,
}

pub const PRESETS: &[PresetMeta] = &[
    PresetMeta { id: "deepwork", tier: PresetTier::Scenario },
    PresetMeta { id: "morning", tier: PresetTier::Scenario },
    PresetMeta { id: "study", tier: PresetTier::Scenario },
    PresetMeta { id: "detox", tier: PresetTier::Scenario },
    PresetMeta { id: "sleep", tier: PresetTier::Scenario },
    PresetMeta { id: "sober", tier: PresetTier::Scenario },
    PresetMeta { id: "no-social", tier: PresetTier::Surgical },
    PresetMeta { id: "no-video", tier: PresetTier::Surgical },
    PresetMeta { id: "no-news", tier: PresetTier::Surgical },
    PresetMeta { id: "no-games", tier: PresetTier::Surgical },
    PresetMeta { id: "no-chat", tier: PresetTier::Surgical },
    PresetMeta { id: "no-shopping", tier: PresetTier::Surgical },
    PresetMeta { id: "no-adult", tier: PresetTier::Surgical },
    PresetMeta { id: "no-gambling", tier: PresetTier::Surgical },
    PresetMeta { id: "no-dating", tier: PresetTier::Surgical },
    PresetMeta { id: "no-ai", tier: PresetTier::Surgical },
    PresetMeta { id: "lockdown", tier: PresetTier::Maximum },
];

pub const PRESET_NAMES: &[&str] = &[
    "deepwork",
    "morning",
    "study",
    "detox",
    "sleep",
    "sober",
    "no-social",
    "no-video",
    "no-news",
    "no-games",
    "no-chat",
    "no-shopping",
    "no-adult",
    "no-gambling",
    "no-dating",
    "no-ai",
    "lockdown",
];

const DEEPWORK: &str = include_str!("../../assets/presets/deepwork.toml");
const MORNING: &str = include_str!("../../assets/presets/morning.toml");
const STUDY: &str = include_str!("../../assets/presets/study.toml");
const DETOX: &str = include_str!("../../assets/presets/detox.toml");
const SLEEP: &str = include_str!("../../assets/presets/sleep.toml");
const SOBER: &str = include_str!("../../assets/presets/sober.toml");
const LOCKDOWN: &str = include_str!("../../assets/presets/lockdown.toml");
const NO_SOCIAL: &str = include_str!("../../assets/presets/no-social.toml");
const NO_VIDEO: &str = include_str!("../../assets/presets/no-video.toml");
const NO_NEWS: &str = include_str!("../../assets/presets/no-news.toml");
const NO_GAMES: &str = include_str!("../../assets/presets/no-games.toml");
const NO_CHAT: &str = include_str!("../../assets/presets/no-chat.toml");
const NO_SHOPPING: &str = include_str!("../../assets/presets/no-shopping.toml");
const NO_ADULT: &str = include_str!("../../assets/presets/no-adult.toml");
const NO_GAMBLING: &str = include_str!("../../assets/presets/no-gambling.toml");
const NO_DATING: &str = include_str!("../../assets/presets/no-dating.toml");
const NO_AI: &str = include_str!("../../assets/presets/no-ai.toml");

pub fn load_preset(name: &str) -> Result<Profile> {
    let raw = match name {
        "deepwork" => DEEPWORK,
        "morning" => MORNING,
        "study" => STUDY,
        "detox" => DETOX,
        "sleep" => SLEEP,
        "sober" => SOBER,
        "lockdown" => LOCKDOWN,
        "no-social" => NO_SOCIAL,
        "no-video" => NO_VIDEO,
        "no-news" => NO_NEWS,
        "no-games" => NO_GAMES,
        "no-chat" => NO_CHAT,
        "no-shopping" => NO_SHOPPING,
        "no-adult" => NO_ADULT,
        "no-gambling" => NO_GAMBLING,
        "no-dating" => NO_DATING,
        "no-ai" => NO_AI,
        other => return Err(Error::Config(format!("unknown preset `{other}`"))),
    };
    toml::from_str(raw).map_err(Error::from)
}

pub fn lookup_preset(name: &str) -> Option<Profile> {
    load_preset(name).ok()
}

pub fn preset_meta(id: &str) -> Option<&'static PresetMeta> {
    PRESETS.iter().find(|p| p.id == id)
}

pub fn preset_label(id: &str) -> Cow<'static, str> {
    let key = format!("onboarding.preset_{}_label", id.replace('-', "_"));
    let v = i18n::lookup(&key);
    if v.as_ref() == key {
        Cow::Owned(id.to_string())
    } else {
        v
    }
}

pub fn preset_blurb(id: &str) -> Cow<'static, str> {
    let key = format!("onboarding.preset_{}_blurb", id.replace('-', "_"));
    let v = i18n::lookup(&key);
    if v.as_ref() == key {
        Cow::Owned(String::new())
    } else {
        v
    }
}

#[cfg(test)]
mod preset_smoke_tests {
    use super::*;
    use crate::sites::expand_groups;

    #[test]
    fn all_registered_presets_load() {
        for name in PRESET_NAMES {
            let p = load_preset(name).unwrap_or_else(|e| panic!("preset {name}: {e}"));
            for g in &p.site_groups {
                assert!(g.contains('.'), "preset {name} has malformed group {g}");
            }
            let hosts = expand_groups(&p.site_groups).unwrap();
            assert!(
                !hosts.is_empty() || p.site_groups.is_empty(),
                "preset {name} expands to zero hosts (group ids invalid?)"
            );
        }
    }

    #[test]
    fn metadata_covers_all_presets() {
        for name in PRESET_NAMES {
            assert!(preset_meta(name).is_some(), "missing meta for {name}");
        }
        for meta in PRESETS {
            assert!(PRESET_NAMES.contains(&meta.id), "meta refers to unregistered {}", meta.id);
        }
    }
}
