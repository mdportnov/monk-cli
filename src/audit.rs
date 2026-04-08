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
        let event = AuditEvent { at: Utc::now(), kind, session_id, message: message.to_string() };
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
