use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::session::Session;

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
    Pong,
    Session(Box<Session>),
    Sessions(Vec<Session>),
    Status { active: Option<Box<Session>>, hard_mode: Option<Box<HardModeInfo>>, pid: u32 },
    PanicScheduled(Box<HardModeInfo>),
    HardModeActive(Box<HardModeInfo>),
    Error { message: String },
}
