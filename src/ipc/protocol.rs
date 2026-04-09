use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    audit::stats::{DayUsage, ModeStats},
    config::{Config, Limits, Profile},
    session::Session,
};

pub const PROTOCOL_VERSION: u16 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub v: u16,
    pub body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
    Start { profile: String, duration: Duration, hard_mode: bool, reason: Option<String> },
    Stop { id: Option<Uuid> },
    Pause { id: Uuid },
    Resume { id: Uuid },
    List,
    Shutdown,
    Panic { phrase: String, cancel: bool },
    ListModes,
    ModeStats { name: String },
    ModeDetail { name: String, days: u32 },
    SaveMode { name: String, profile: Box<crate::config::Profile> },
    DeleteMode { name: String },
    GetGeneral,
    UpdateGeneral { general: crate::config::General },
    ResetAll,
    GetConfig,
    SaveConfig { config: Box<Config> },
    NextScheduled,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeSummary {
    pub name: String,
    pub color: Option<String>,
    pub blocked_apps: usize,
    pub blocked_sites: usize,
    pub blocked_groups: usize,
    pub limits: Limits,
    pub stats: ModeStats,
    pub is_default: bool,
    #[serde(default)]
    pub has_schedule: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeDetailPayload {
    pub profile: Profile,
    pub expanded_sites: Vec<String>,
    pub usage: Vec<DayUsage>,
    pub total_sessions_7d: u32,
    pub total_duration_7d: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardModeInfo {
    pub ends_at: DateTime<Utc>,
    pub remaining: Duration,
    pub reason: Option<String>,
    pub panic_phrase: String,
    pub panic_requested_at: Option<DateTime<Utc>>,
    pub panic_releases_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Pong { version: String },
    Session(Box<Session>),
    Sessions { sessions: Vec<Session> },
    Status { active: Option<Box<Session>>, hard_mode: Option<Box<HardModeInfo>>, pid: u32 },
    PanicScheduled(Box<HardModeInfo>),
    HardModeActive(Box<HardModeInfo>),
    Modes { modes: Vec<ModeSummary> },
    ModeStatsData(ModeStats),
    ModeDetailData(Box<ModeDetailPayload>),
    General(crate::config::General),
    Config(Box<Config>),
    NextScheduled { profile: Option<String>, at: Option<DateTime<Utc>> },
    Error { message: String },
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_request_roundtrip() {
        let env = Envelope { v: PROTOCOL_VERSION, body: Request::Ping };
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope<Request> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.v, PROTOCOL_VERSION);
        assert!(matches!(back.body, Request::Ping));
    }

    #[test]
    fn unknown_request_kind_decodes_to_unknown() {
        let json = r#"{"v":2,"body":{"kind":"future_op","payload":123}}"#;
        let env: Envelope<Request> = serde_json::from_str(json).unwrap();
        assert!(matches!(env.body, Request::Unknown));
    }

    #[test]
    fn unknown_response_kind_decodes_to_unknown() {
        let json = r#"{"v":2,"body":{"kind":"future_status","extra":"hi"}}"#;
        let env: Envelope<Response> = serde_json::from_str(json).unwrap();
        assert!(matches!(env.body, Response::Unknown));
    }
}
