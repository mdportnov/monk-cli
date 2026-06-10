use crate::apps::InstalledApp;

#[derive(Debug, Clone, Copy)]
pub struct CuratedEntry {
    pub category: &'static str,
    /// Exact bundle id (macOS), .desktop id (Linux) or shortcut target stem
    /// (Windows), lower-cased. Matched case-insensitively against `app.id`.
    pub id: &'static str,
}

/// Known common distractors. Curated, deliberately conservative — false
/// positives erode user trust during onboarding more than false negatives.
/// Add entries with full canonical ids, not substrings.
pub const CURATED: &[CuratedEntry] = &[
    // chat
    CuratedEntry { category: "chat", id: "com.tinyspeck.slackmacgap" },
    CuratedEntry { category: "chat", id: "com.hnc.discord" },
    CuratedEntry { category: "chat", id: "ru.keepcoder.telegram" },
    CuratedEntry { category: "chat", id: "org.telegram.desktop" },
    CuratedEntry { category: "chat", id: "desktop.telegram" },
    CuratedEntry { category: "chat", id: "net.whatsapp.whatsapp" },
    CuratedEntry { category: "chat", id: "com.skype.skype" },
    CuratedEntry { category: "chat", id: "com.microsoft.teams" },
    CuratedEntry { category: "chat", id: "com.microsoft.teams2" },
    CuratedEntry { category: "chat", id: "com.viber" },
    CuratedEntry { category: "chat", id: "org.whispersystems.signal-desktop" },
    CuratedEntry { category: "chat", id: "com.facebook.messenger" },
    // social
    CuratedEntry { category: "social", id: "com.twitter.twitter-mac" },
    CuratedEntry { category: "social", id: "maccatalyst.com.atebits.tweetie2" },
    CuratedEntry { category: "social", id: "com.tapbots.tweetbot3mac" },
    CuratedEntry { category: "social", id: "com.reddit.reddit" },
    // media
    CuratedEntry { category: "media", id: "com.spotify.client" },
    CuratedEntry { category: "media", id: "com.apple.music" },
    // games
    CuratedEntry { category: "games", id: "com.valvesoftware.steam" },
    CuratedEntry { category: "games", id: "com.epicgames.launcher" },
    CuratedEntry { category: "games", id: "com.blizzard.worldofwarcraft" },
    CuratedEntry { category: "games", id: "net.battle.net" },
    CuratedEntry { category: "games", id: "com.riotgames.leagueoflegends.gameclient" },
    // browsers — intentionally NOT pre-selected by default; opt-in via
    // explicit profile editing. Listed here so the picker can surface them.
];

pub fn match_curated(app: &InstalledApp) -> Option<&'static str> {
    let id_lower = app.id.to_lowercase();
    CURATED.iter().find(|e| e.id.eq_ignore_ascii_case(&id_lower)).map(|e| e.category)
}

pub fn partition(apps: &[InstalledApp]) -> (Vec<&InstalledApp>, Vec<&InstalledApp>) {
    let mut curated = Vec::new();
    let mut rest = Vec::new();
    for a in apps {
        if match_curated(a).is_some() {
            curated.push(a);
        } else {
            rest.push(a);
        }
    }
    (curated, rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::{AppKind, InstalledApp};
    use std::path::PathBuf;

    fn app(id: &str) -> InstalledApp {
        InstalledApp {
            id: id.into(),
            label: id.into(),
            exec_path: PathBuf::from("/Applications/X.app/Contents/MacOS/X"),
            kind: AppKind::MacBundle,
            sandbox_id: None,
        }
    }

    #[test]
    fn exact_id_matches_case_insensitively() {
        assert_eq!(match_curated(&app("com.tinyspeck.slackmacgap")), Some("chat"));
        assert_eq!(match_curated(&app("COM.TINYSPECK.SLACKMACGAP")), Some("chat"));
        assert_eq!(match_curated(&app("ru.keepcoder.Telegram")), Some("chat"));
        assert_eq!(match_curated(&app("com.spotify.client")), Some("media"));
    }

    #[test]
    fn substring_no_longer_matches() {
        // Used to match before tightening: any bundle id *containing*
        // "telegram" or "reddit" was claimed. These should now miss.
        assert_eq!(match_curated(&app("com.unrelated.telegram-tools")), None);
        assert_eq!(match_curated(&app("net.reddit.viewer")), None);
        assert_eq!(match_curated(&app("com.google.chrome")), None);
    }

    #[test]
    fn unknown_apps_miss() {
        assert_eq!(match_curated(&app("com.apple.Calculator")), None);
        assert_eq!(match_curated(&app("com.example.notesapp")), None);
    }

    #[test]
    fn partition_splits_correctly() {
        let apps = vec![
            app("com.tinyspeck.slackmacgap"),
            app("com.apple.Calculator"),
            app("com.spotify.client"),
        ];
        let (curated, rest) = partition(&apps);
        assert_eq!(curated.len(), 2);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].id, "com.apple.Calculator");
    }
}
