use std::{
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{paths, Result};

pub const AUDIT_FILE: &str = "audit.log";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    SessionStarted,
    SessionCompleted,
    SessionPanicked,
    PanicRequested,
    PanicCancelled,
    StopDenied,
    UninstallDenied,
    ResetDenied,
    TamperDetected,
    TamperPenalty,
    HostsRepaired,
    DaemonRestarted,
    ClockAnomaly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: DateTime<Utc>,
    pub kind: AuditKind,
    pub session_id: Option<Uuid>,
    pub message: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
}

impl AuditLog {
    pub fn new() -> Result<Self> {
        Ok(Self { path: paths::data_dir()?.join(AUDIT_FILE) })
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, kind: AuditKind, session_id: Option<Uuid>, message: &str) {
        self.append_with(kind, session_id, message, serde_json::Value::Null);
    }

    pub fn append_with(
        &self,
        kind: AuditKind,
        session_id: Option<Uuid>,
        message: &str,
        extra: serde_json::Value,
    ) {
        let event = AuditEvent {
            at: Utc::now(),
            kind,
            session_id,
            message: message.to_string(),
            extra,
        };
        if let Err(e) = self.write(&event) {
            tracing::warn!(?e, "audit write failed");
        }
    }

    fn write(&self, event: &AuditEvent) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let mut file = fs_err::OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        file.write_all(&line)?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEvent>> {
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let raw = fs_err::read_to_string(&self.path)?;
        let mut out = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
                out.push(event);
            }
        }
        Ok(out)
    }
}

pub mod stats {
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    use super::{AuditEvent, AuditKind};
    use crate::config::Limits;

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct ModeStats {
        #[serde(with = "crate::audit::stats::dur_ms")]
        pub used_24h: Duration,
        pub last_completed_at: Option<DateTime<Utc>>,
        #[serde(default, with = "crate::audit::stats::dur_ms_opt")]
        pub cooldown_remaining: Option<Duration>,
        #[serde(default, with = "crate::audit::stats::dur_ms_opt")]
        pub daily_cap_remaining: Option<Duration>,
    }

    pub fn mode_stats(
        events: &[AuditEvent],
        mode: &str,
        limits: &Limits,
        now: DateTime<Utc>,
    ) -> ModeStats {
        let since = now - chrono::Duration::hours(24);
        let mut used_24h = Duration::ZERO;
        let mut last_completed_at: Option<DateTime<Utc>> = None;
        for e in events {
            if e.kind != AuditKind::SessionCompleted || e.message != mode || e.at < since {
                continue;
            }
            let ms = e.extra.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            used_24h = used_24h.saturating_add(Duration::from_millis(ms));
            last_completed_at = Some(match last_completed_at {
                Some(prev) if prev > e.at => prev,
                _ => e.at,
            });
        }
        let cooldown_remaining = match (limits.cooldown, last_completed_at) {
            (Some(cd), Some(last)) => {
                let elapsed = now.signed_duration_since(last);
                let cd_chrono =
                    chrono::Duration::from_std(cd).unwrap_or_else(|_| chrono::Duration::zero());
                if elapsed < cd_chrono {
                    (cd_chrono - elapsed).to_std().ok()
                } else {
                    None
                }
            }
            _ => None,
        };
        let daily_cap_remaining = limits
            .daily_cap
            .map(|cap| cap.saturating_sub(used_24h));
        ModeStats { used_24h, last_completed_at, cooldown_remaining, daily_cap_remaining }
    }

    pub(crate) mod dur_ms {
        use std::time::Duration;
        use serde::{Deserialize, Deserializer, Serializer};
        pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_u64(d.as_millis() as u64)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
            Ok(Duration::from_millis(u64::deserialize(d)?))
        }
    }

    pub(crate) mod dur_ms_opt {
        use std::time::Duration;
        use serde::{Deserialize, Deserializer, Serializer};
        pub fn serialize<S: Serializer>(
            d: &Option<Duration>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            match d {
                Some(v) => s.serialize_u64(v.as_millis() as u64),
                None => s.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Duration>, D::Error> {
            Ok(Option::<u64>::deserialize(d)?.map(Duration::from_millis))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::with_path(dir.path().join(AUDIT_FILE));
        log.append(AuditKind::SessionStarted, Some(Uuid::nil()), "start");
        log.append(AuditKind::StopDenied, Some(Uuid::nil()), "denied");
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, AuditKind::SessionStarted);
        assert_eq!(events[1].kind, AuditKind::StopDenied);
    }
}
