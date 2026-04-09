use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{audit::stats::ModeStats, config::Limits, session::Session};

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
    SaveMode { name: String, profile: crate::config::Profile },
    DeleteMode { name: String },
    GetGeneral,
    UpdateGeneral { general: crate::config::General },
    ResetAll,
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
    General(crate::config::General),
    Error { message: String },
}
