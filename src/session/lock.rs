use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{paths, Error, Result};

pub const LOCK_SCHEMA: u32 = 1;
pub const LOCK_FILE: &str = "session.lock";

const PHRASE_WORDS: &[&str] = &[
    "focus", "quiet", "anchor", "steady", "summit", "ember", "forge", "harbor", "lantern",
    "meadow", "marble", "orbit", "pillar", "river", "stone", "timber", "valley", "willow",
    "yonder", "zephyr", "beacon", "cedar", "glade", "haven",
];

pub fn generate_phrase() -> String {
    use rand::seq::IndexedRandom;
    let mut rng = rand::rng();
    let mut out = Vec::with_capacity(4);
    for _ in 0..4 {
        if let Some(w) = PHRASE_WORDS.choose(&mut rng) {
            out.push(*w);
        }
    }
    out.join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionLock {
    pub schema_version: u32,
    pub id: Uuid,
    pub profile: String,
    pub started_at: DateTime<Utc>,
    pub started_at_boot_ms: u128,
    pub boot_id: String,
    pub duration_ms: u128,
    pub progressed_ms: u128,
    pub hard_mode: bool,
    pub panic_requested_at: Option<DateTime<Utc>>,
    pub panic_delay_ms: u128,
    pub panic_phrase: String,
    pub reason: Option<String>,
    pub penalty_applied_ms: u128,
    pub mac: String,
}

#[derive(Debug, Clone)]
pub struct NewLock {
    pub profile: String,
    pub duration: Duration,
    pub hard_mode: bool,
    pub panic_delay: Duration,
    pub panic_phrase: String,
    pub reason: Option<String>,
    pub boot_id: String,
    pub boot_ms: u128,
}

impl SessionLock {
    pub fn new(params: NewLock) -> Self {
        let mut lock = Self {
            schema_version: LOCK_SCHEMA,
            id: Uuid::new_v4(),
            profile: params.profile,
            started_at: Utc::now(),
            started_at_boot_ms: params.boot_ms,
            boot_id: params.boot_id,
            duration_ms: params.duration.as_millis(),
            progressed_ms: 0,
            hard_mode: params.hard_mode,
            panic_requested_at: None,
            panic_delay_ms: params.panic_delay.as_millis(),
            panic_phrase: params.panic_phrase,
            reason: params.reason,
            penalty_applied_ms: 0,
            mac: String::new(),
        };
        lock.reseal();
        lock
    }

    pub fn remaining(&self) -> Duration {
        let now = Utc::now();
        if now < self.started_at {
            let total_ms = self.duration_ms.saturating_add(self.penalty_applied_ms);
            Duration::from_millis(u64::try_from(total_ms).unwrap_or(u64::MAX))
        } else {
            let end = self.ends_at();
            if now >= end {
                Duration::ZERO
            } else {
                (end - now).to_std().unwrap_or(Duration::ZERO)
            }
        }
    }

    pub fn ends_at(&self) -> DateTime<Utc> {
        let total_ms = self.duration_ms.saturating_add(self.penalty_applied_ms);
        let total_duration = chrono::Duration::milliseconds(i64::try_from(total_ms).unwrap_or(i64::MAX));
        self.started_at + total_duration
    }

    pub fn panic_releases_at(&self) -> Option<DateTime<Utc>> {
        let requested = self.panic_requested_at?;
        let delay =
            chrono::Duration::milliseconds(i64::try_from(self.panic_delay_ms).unwrap_or(i64::MAX));
        Some(requested + delay)
    }

    /// Checks if session has expired based on wall-clock time
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        if now < self.started_at {
            tracing::warn!(
                session_id = %self.id,
                started_at = %self.started_at,
                now = %now,
                "session start time is in the future; treating as not expired"
            );
            return false;
        }
        now >= self.ends_at()
    }

    pub fn should_release_via_panic(&self) -> bool {
        self.panic_releases_at().is_some_and(|t| Utc::now() >= t)
    }

    pub fn apply_penalty(&mut self, extra: Duration) {
        const MAX_PENALTY_MS: u128 = 24 * 60 * 60 * 1000;
        self.penalty_applied_ms = self.penalty_applied_ms.saturating_add(extra.as_millis());
        if self.penalty_applied_ms > MAX_PENALTY_MS {
            self.penalty_applied_ms = MAX_PENALTY_MS;
        }
        self.reseal();
    }

    pub fn advance(&mut self, delta: Duration) {
        self.progressed_ms = self.progressed_ms.saturating_add(delta.as_millis());
        self.reseal();
    }

    pub fn request_panic(&mut self) {
        self.panic_requested_at = Some(Utc::now());
        self.reseal();
    }

    pub fn cancel_panic(&mut self) {
        self.panic_requested_at = None;
        self.reseal();
    }

    pub fn reseal(&mut self) {
        self.mac = String::new();
        self.mac = hex::encode(mac(self));
    }

    pub fn verify(&self) -> bool {
        let mut clone = self.clone();
        let expected = self.mac.clone();
        clone.mac = String::new();
        hex::encode(mac(&clone)) == expected
    }
}

fn mac(lock: &SessionLock) -> [u8; 32] {
    let key = derive_key(&lock.id);
    let mut hasher = blake3::Hasher::new_keyed(&key);
    hasher.update(b"monk-lock-canonical-v1\0");
    hasher.update(&lock.schema_version.to_le_bytes());
    hasher.update(lock.id.as_bytes());
    write_len_prefixed(&mut hasher, lock.profile.as_bytes());
    hasher.update(&lock.started_at.timestamp_millis().to_le_bytes());
    hasher.update(&lock.started_at_boot_ms.to_le_bytes());
    write_len_prefixed(&mut hasher, lock.boot_id.as_bytes());
    hasher.update(&lock.duration_ms.to_le_bytes());
    hasher.update(&lock.progressed_ms.to_le_bytes());
    hasher.update(&[u8::from(lock.hard_mode)]);
    match lock.panic_requested_at {
        Some(ts) => {
            hasher.update(&[1]);
            hasher.update(&ts.timestamp_millis().to_le_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
    hasher.update(&lock.panic_delay_ms.to_le_bytes());
    write_len_prefixed(&mut hasher, lock.panic_phrase.as_bytes());
    match &lock.reason {
        Some(s) => {
            hasher.update(&[1]);
            write_len_prefixed(&mut hasher, s.as_bytes());
        }
        None => {
            hasher.update(&[0]);
        }
    }
    hasher.update(&lock.penalty_applied_ms.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn write_len_prefixed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn derive_key(session_id: &Uuid) -> [u8; 32] {
    let machine = machine_identity();
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"monk-session-lock-v1");
    hasher.update(machine.as_bytes());
    hasher.update(session_id.as_bytes());
    *hasher.finalize().as_bytes()
}

fn machine_identity() -> String {
    if let Ok(id) = machine_uid::get() {
        return id;
    }
    persistent_fallback_identity().unwrap_or_else(|_| "monk-fallback-unresolved".to_string())
}

fn persistent_fallback_identity() -> Result<String> {
    let path = paths::data_dir()?.join("machine-key");
    if let Ok(existing) = fs_err::read_to_string(&path) {
        let trimmed = existing.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let mut buf = [0u8; 32];
    use rand::RngCore;
    rand::rng().fill_bytes(&mut buf);
    let encoded = hex::encode(buf);
    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    fs_err::write(&path, &encoded)?;
    Ok(encoded)
}

#[derive(Debug, Clone)]
pub struct LockStore {
    primary: PathBuf,
    backups: Vec<PathBuf>,
}

impl LockStore {
    pub fn new() -> Result<Self> {
        Ok(Self {
            primary: paths::data_dir()?.join(LOCK_FILE),
            backups: vec![
                paths::config_dir()?.join(LOCK_FILE),
                paths::runtime_dir()?.join(LOCK_FILE),
            ],
        })
    }

    pub fn with_paths(primary: PathBuf, backups: Vec<PathBuf>) -> Self {
        Self { primary, backups }
    }

    pub fn primary(&self) -> &Path {
        &self.primary
    }

    pub fn load(&self) -> Result<Option<(SessionLock, LoadKind)>> {
        let mut best: Option<(SessionLock, LoadKind)> = None;
        let mut any_present = false;

        for (idx, path) in std::iter::once(&self.primary).chain(self.backups.iter()).enumerate() {
            if !path.exists() {
                continue;
            }
            any_present = true;
            let raw = match fs_err::read_to_string(path) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let lock: SessionLock = match serde_json::from_str(&raw) {
                Ok(l) => l,
                Err(_) => {
                    let kind =
                        if idx == 0 { LoadKind::TamperedPrimary } else { LoadKind::TamperedBackup };
                    best = Some((placeholder_after_tamper(&raw), kind));
                    continue;
                }
            };
            let kind = if lock.verify() {
                LoadKind::Valid
            } else if idx == 0 {
                LoadKind::TamperedPrimary
            } else {
                LoadKind::TamperedBackup
            };
            match &best {
                None => best = Some((lock, kind)),
                Some((cur, _)) if lock.ends_at() > cur.ends_at() => best = Some((lock, kind)),
                _ => {}
            }
        }

        if !any_present {
            return Ok(None);
        }
        Ok(best)
    }

    pub fn save(&self, lock: &SessionLock) -> Result<()> {
        let raw = serde_json::to_vec_pretty(lock)?;
        write_atomic(&self.primary, &raw)?;
        for p in &self.backups {
            if let Err(e) = write_atomic(p, &raw) {
                tracing::warn!(path = %p.display(), ?e, "backup lock write failed");
            }
        }
        Ok(())
    }

    pub fn delete(&self) -> Result<()> {
        for p in std::iter::once(&self.primary).chain(self.backups.iter()) {
            if p.exists() {
                let _ = fs_err::remove_file(p);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadKind {
    Valid,
    TamperedPrimary,
    TamperedBackup,
}

fn placeholder_after_tamper(_raw: &str) -> SessionLock {
    SessionLock {
        schema_version: LOCK_SCHEMA,
        id: Uuid::nil(),
        profile: String::new(),
        started_at: Utc::now(),
        started_at_boot_ms: 0,
        boot_id: String::new(),
        duration_ms: 0,
        progressed_ms: 0,
        hard_mode: true,
        panic_requested_at: None,
        panic_delay_ms: 0,
        panic_phrase: String::new(),
        reason: None,
        penalty_applied_ms: 0,
        mac: String::new(),
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        fs_err::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("lock.tmp");
    {
        let mut f =
            fs_err::OpenOptions::new().create(true).write(true).truncate(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        f.file().sync_all()?;
    }
    fs_err::rename(&tmp, path).map_err(Error::Io)?;
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs_err::File::open(parent) {
            let _ = dir.file().sync_all();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SessionLock {
        SessionLock::new(NewLock {
            profile: "deepwork".into(),
            duration: Duration::from_secs(25 * 60),
            hard_mode: true,
            panic_delay: Duration::from_secs(15 * 60),
            panic_phrase: "focus on the work".into(),
            reason: Some("ship the PR".into()),
            boot_id: "boot-123".into(),
            boot_ms: 0,
        })
    }

    #[test]
    fn roundtrip_verifies() {
        let lock = sample();
        assert!(lock.verify());
        let raw = serde_json::to_string(&lock).unwrap();
        let back: SessionLock = serde_json::from_str(&raw).unwrap();
        assert!(back.verify());
        assert_eq!(lock, back);
    }

    #[test]
    fn tamper_breaks_mac() {
        let mut lock = sample();
        lock.duration_ms = 10;
        assert!(!lock.verify());
    }

    #[test]
    fn penalty_advances_end() {
        let mut lock = sample();
        lock.started_at = Utc::now() - chrono::Duration::minutes(20);
        let before = lock.remaining();
        lock.apply_penalty(Duration::from_secs(60));
        let after = lock.remaining();
        assert!(after > before);
        assert!(lock.verify());
    }

    #[test]
    fn store_save_load_quorum() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("a").join("session.lock");
        let backups = vec![
            dir.path().join("b").join("session.lock"),
            dir.path().join("c").join("session.lock"),
        ];
        let store = LockStore::with_paths(primary.clone(), backups.clone());

        let lock = sample();
        store.save(&lock).unwrap();

        let (loaded, kind) = store.load().unwrap().unwrap();
        assert_eq!(kind, LoadKind::Valid);
        assert_eq!(loaded, lock);

        fs_err::remove_file(&primary).unwrap();
        let (loaded, kind) = store.load().unwrap().unwrap();
        assert_eq!(kind, LoadKind::Valid);
        assert_eq!(loaded, lock);

        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn hard_mode_e2e_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("a/session.lock");
        let backups = vec![dir.path().join("b/session.lock"), dir.path().join("c/session.lock")];
        let store = LockStore::with_paths(primary.clone(), backups.clone());

        let mut lock = SessionLock::new(NewLock {
            profile: "deepwork".into(),
            duration: Duration::from_secs(60 * 60),
            hard_mode: true,
            panic_delay: Duration::from_millis(50),
            panic_phrase: "focus on the work".into(),
            reason: Some("ship it".into()),
            boot_id: "boot-xyz".into(),
            boot_ms: 0,
        });
        store.save(&lock).unwrap();

        let (restored, kind) = store.load().unwrap().unwrap();
        assert_eq!(kind, LoadKind::Valid);
        assert!(restored.hard_mode && !restored.is_expired());

        assert!(!lock.should_release_via_panic());
        lock.request_panic();
        store.save(&lock).unwrap();
        std::thread::sleep(Duration::from_millis(80));
        let (after, _) = store.load().unwrap().unwrap();
        assert!(after.should_release_via_panic());

        let mut tampered = after.clone();
        tampered.duration_ms = 0;
        let raw = serde_json::to_vec_pretty(&tampered).unwrap();
        fs_err::write(&primary, raw).unwrap();
        for b in &backups {
            let _ = fs_err::remove_file(b);
        }
        let (_, kind) = store.load().unwrap().unwrap();
        assert_eq!(kind, LoadKind::TamperedPrimary);

        let mut penalized = after;
        penalized.started_at = Utc::now() - chrono::Duration::minutes(30);
        penalized.apply_penalty(Duration::from_secs(15 * 60));
        assert!(penalized.verify());
        assert!(penalized.remaining() > Duration::from_secs(15 * 60));

        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn tampered_primary_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("session.lock");
        let store = LockStore::with_paths(primary.clone(), vec![]);
        let lock = sample();
        store.save(&lock).unwrap();

        let mut raw: serde_json::Value =
            serde_json::from_str(&fs_err::read_to_string(&primary).unwrap()).unwrap();
        raw["duration_ms"] = serde_json::json!(1);
        fs_err::write(&primary, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();

        let (_, kind) = store.load().unwrap().unwrap();
        assert_eq!(kind, LoadKind::TamperedPrimary);
    }

    #[test]
    fn multiple_tamper_events_unique_detection() {
        let lock1 = sample();
        let mut lock2 = sample();
        let mut lock3 = sample();

        lock2.duration_ms = 100;
        lock2.reseal();
        lock3.duration_ms = 200;
        lock3.reseal();

        assert_ne!(lock1.mac, lock2.mac);
        assert_ne!(lock2.mac, lock3.mac);
        assert_ne!(lock1.mac, lock3.mac);

        let mut penalty_count = 0;
        let mut last_mac: Option<String> = None;

        for lock in [&lock1, &lock2, &lock3] {
            if let Some(ref prev_mac) = last_mac {
                if *prev_mac != lock.mac {
                    penalty_count += 1;
                }
            } else {
                penalty_count += 1;
            }
            last_mac = Some(lock.mac.clone());
        }

        assert_eq!(penalty_count, 3);
    }

    #[test]
    fn future_start_time_handling() {
        let future = Utc::now() + chrono::Duration::minutes(10);
        let mut lock = SessionLock::new(NewLock {
            profile: "test".into(),
            duration: Duration::from_secs(60),
            hard_mode: false,
            panic_delay: Duration::from_secs(300),
            panic_phrase: "test phrase".into(),
            reason: None,
            boot_id: "boot-123".into(),
            boot_ms: 0,
        });
        lock.started_at = future;
        lock.reseal();

        assert!(!lock.is_expired());
        assert_eq!(lock.remaining(), Duration::from_secs(60));
    }
}
