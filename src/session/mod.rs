pub mod lock;

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use lock::{LoadKind, LockStore, NewLock, SessionLock, LOCK_SCHEMA};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub profile: String,
    pub started_at: DateTime<Utc>,
    pub duration: Duration,
    pub hard_mode: bool,
    pub state: SessionState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Running,
    Paused,
    Completed,
    Aborted,
}

impl Session {
    pub fn new(profile: String, duration: Duration, hard_mode: bool) -> Self {
        Self {
            id: Uuid::new_v4(),
            profile,
            started_at: Utc::now(),
            duration,
            hard_mode,
            state: SessionState::Running,
        }
    }

    pub fn ends_at(&self) -> DateTime<Utc> {
        self.started_at
            + chrono::Duration::from_std(self.duration).unwrap_or_else(|_| chrono::Duration::zero())
    }

    pub fn remaining(&self) -> Duration {
        let now = Utc::now();
        let end = self.ends_at();
        if end <= now {
            Duration::ZERO
        } else {
            (end - now).to_std().unwrap_or(Duration::ZERO)
        }
    }
}
