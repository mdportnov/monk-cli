use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{paths, Error, Result};

pub const AUDIT_FILE: &str = "audit.sqlite3";
pub const LEGACY_AUDIT_FILE: &str = "audit.log";

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

impl AuditKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SessionStarted => "session_started",
            Self::SessionCompleted => "session_completed",
            Self::SessionPanicked => "session_panicked",
            Self::PanicRequested => "panic_requested",
            Self::PanicCancelled => "panic_cancelled",
            Self::StopDenied => "stop_denied",
            Self::UninstallDenied => "uninstall_denied",
            Self::ResetDenied => "reset_denied",
            Self::TamperDetected => "tamper_detected",
            Self::TamperPenalty => "tamper_penalty",
            Self::HostsRepaired => "hosts_repaired",
            Self::DaemonRestarted => "daemon_restarted",
            Self::ClockAnomaly => "clock_anomaly",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "session_started" => Self::SessionStarted,
            "session_completed" => Self::SessionCompleted,
            "session_panicked" => Self::SessionPanicked,
            "panic_requested" => Self::PanicRequested,
            "panic_cancelled" => Self::PanicCancelled,
            "stop_denied" => Self::StopDenied,
            "uninstall_denied" => Self::UninstallDenied,
            "reset_denied" => Self::ResetDenied,
            "tamper_detected" => Self::TamperDetected,
            "tamper_penalty" => Self::TamperPenalty,
            "hosts_repaired" => Self::HostsRepaired,
            "daemon_restarted" => Self::DaemonRestarted,
            "clock_anomaly" => Self::ClockAnomaly,
            _ => return None,
        })
    }
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
    conn: Arc<Mutex<Connection>>,
}

impl AuditLog {
    pub fn new() -> Result<Self> {
        let path = paths::data_dir()?.join(AUDIT_FILE);
        Self::open(path)
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self::open(path).expect("open audit sqlite in tests")
    }

    fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS audit_events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                at          TEXT    NOT NULL,
                kind        TEXT    NOT NULL,
                session_id  TEXT,
                message     TEXT    NOT NULL,
                extra       TEXT    NOT NULL DEFAULT 'null'
            );
            CREATE INDEX IF NOT EXISTS idx_audit_at         ON audit_events(at);
            CREATE INDEX IF NOT EXISTS idx_audit_kind_at    ON audit_events(kind, at);
            CREATE INDEX IF NOT EXISTS idx_audit_session_id ON audit_events(session_id);
            "#,
        )?;
        let log = Self { path, conn: Arc::new(Mutex::new(conn)) };
        log.migrate_legacy_jsonl();
        Ok(log)
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
        if let Err(e) = self.insert(kind, session_id, message, &extra) {
            tracing::warn!(?e, "audit write failed");
        }
    }

    fn insert(
        &self,
        kind: AuditKind,
        session_id: Option<Uuid>,
        message: &str,
        extra: &serde_json::Value,
    ) -> Result<()> {
        let at = Utc::now().to_rfc3339();
        let sid = session_id.map(|id| id.to_string());
        let extra_s = serde_json::to_string(extra).map_err(Error::from)?;
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO audit_events(at, kind, session_id, message, extra) VALUES (?, ?, ?, ?, ?)",
            params![at, kind.as_str(), sid, message, extra_s],
        )?;
        Ok(())
    }

    pub fn read_all(&self) -> Result<Vec<AuditEvent>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT at, kind, session_id, message, extra FROM audit_events ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], row_to_event)?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(Some(e)) = r {
                out.push(e);
            }
        }
        Ok(out)
    }

    fn migrate_legacy_jsonl(&self) {
        let Some(dir) = self.path.parent() else { return };
        let legacy = dir.join(LEGACY_AUDIT_FILE);
        if !legacy.exists() {
            return;
        }
        let raw = match fs_err::read_to_string(&legacy) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "legacy audit read failed");
                return;
            }
        };
        let conn = self.conn.lock();
        let tx = match conn.unchecked_transaction() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(?e, "legacy audit tx failed");
                return;
            }
        };
        let mut imported = 0usize;
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<AuditEvent>(line) else { continue };
            let sid = event.session_id.map(|id| id.to_string());
            let extra = serde_json::to_string(&event.extra).unwrap_or_else(|_| "null".into());
            if tx
                .execute(
                    "INSERT INTO audit_events(at, kind, session_id, message, extra) VALUES (?, ?, ?, ?, ?)",
                    params![event.at.to_rfc3339(), event.kind.as_str(), sid, event.message, extra],
                )
                .is_ok()
            {
                imported += 1;
            }
        }
        if let Err(e) = tx.commit() {
            tracing::warn!(?e, "legacy audit commit failed");
            return;
        }
        let backup = dir.join(format!("{LEGACY_AUDIT_FILE}.bak"));
        let _ = fs_err::rename(&legacy, &backup);
        tracing::info!(imported, "migrated legacy audit log");
    }
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<AuditEvent>> {
    let at: String = row.get(0)?;
    let kind_s: String = row.get(1)?;
    let sid: Option<String> = row.get(2)?;
    let message: String = row.get(3)?;
    let extra_s: String = row.get(4)?;
    let at = DateTime::parse_from_rfc3339(&at).map(|d| d.with_timezone(&Utc));
    let (Ok(at), Some(kind)) = (at, AuditKind::from_str(&kind_s)) else {
        return Ok(None);
    };
    let session_id = sid.and_then(|s| Uuid::parse_str(&s).ok());
    let extra = serde_json::from_str(&extra_s).unwrap_or(serde_json::Value::Null);
    Ok(Some(AuditEvent { at, kind, session_id, message, extra }))
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
        let daily_cap_remaining = limits.daily_cap.map(|cap| cap.saturating_sub(used_24h));
        ModeStats { used_24h, last_completed_at, cooldown_remaining, daily_cap_remaining }
    }

    pub(crate) mod dur_ms {
        use serde::{Deserialize, Deserializer, Serializer};
        use std::time::Duration;
        pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_u64(d.as_millis() as u64)
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
            Ok(Duration::from_millis(u64::deserialize(d)?))
        }
    }

    pub(crate) mod dur_ms_opt {
        use serde::{Deserialize, Deserializer, Serializer};
        use std::time::Duration;
        pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
            match d {
                Some(v) => s.serialize_u64(v.as_millis() as u64),
                None => s.serialize_none(),
            }
        }
        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
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

    #[test]
    fn migrates_legacy_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_AUDIT_FILE);
        let event = AuditEvent {
            at: Utc::now(),
            kind: AuditKind::SessionCompleted,
            session_id: Some(Uuid::nil()),
            message: "deepwork".into(),
            extra: serde_json::json!({"duration_ms": 1500_u64}),
        };
        fs_err::write(&legacy, format!("{}\n", serde_json::to_string(&event).unwrap())).unwrap();
        let log = AuditLog::with_path(dir.path().join(AUDIT_FILE));
        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].message, "deepwork");
        assert!(!legacy.exists());
    }
}
