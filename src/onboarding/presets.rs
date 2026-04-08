use crate::{config::Profile, Error, Result};

pub const PRESET_NAMES: &[&str] = &["deepwork", "no-chat", "no-news", "no-games"];

const DEEPWORK: &str = include_str!("../../assets/presets/deepwork.toml");
const NO_CHAT: &str = include_str!("../../assets/presets/no-chat.toml");
const NO_NEWS: &str = include_str!("../../assets/presets/no-news.toml");
const NO_GAMES: &str = include_str!("../../assets/presets/no-games.toml");

pub fn load_preset(name: &str) -> Result<Profile> {
    let raw = match name {
        "deepwork" => DEEPWORK,
        "no-chat" => NO_CHAT,
        "no-news" => NO_NEWS,
        "no-games" => NO_GAMES,
        other => return Err(Error::Config(format!("unknown preset `{other}`"))),
    };
    toml::from_str(raw).map_err(Error::from)
}
