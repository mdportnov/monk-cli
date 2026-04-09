use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use parking_lot::{Mutex, RwLock};
use uuid::Uuid;

use crate::{
    apps::{self, AppCache},
    audit::{AuditKind, AuditLog},
    blocker::{self, BlockSet, Blocker, ProcessGuard},
    clock,
    config::{Config, Limits, Profile},
    ipc::HardModeInfo,
    session::{LoadKind, LockStore, NewLock, Session, SessionLock, SessionState},
    sites, Error, Result,
};

#[derive(Debug)]
pub struct Supervisor {
    config: Arc<RwLock<Config>>,
    hosts: Arc<Mutex<Box<dyn Blocker>>>,
    procs: Arc<Mutex<ProcessGuard>>,
    store: Arc<LockStore>,
    audit: Arc<AuditLog>,
    last_tick_ms: Arc<AtomicU64>,
    active_profile: Arc<RwLock<Option<String>>>,
}

impl Supervisor {
    pub fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            hosts: Arc::new(Mutex::new(blocker::select_site_blocker())),
            procs: Arc::new(Mutex::new(ProcessGuard::new())),
            store: Arc::new(LockStore::new()?),
            audit: Arc::new(AuditLog::new()?),
            last_tick_ms: Arc::new(AtomicU64::new(clock::monotonic_ms() as u64)),
            active_profile: Arc::new(RwLock::new(None)),
        })
    }

    pub fn config(&self) -> Config {
        self.config.read().clone()
    }

    pub fn get_config(&self) -> Config {
        if let Ok(fresh) = Config::load() {
            *self.config.write() = fresh;
        }
        self.config.read().clone()
    }

    pub fn save_config(&self, cfg: Config) -> Result<()> {
        cfg.validate()?;
        cfg.save()?;
        *self.config.write() = cfg;
        Ok(())
    }

    pub fn audit(&self) -> Arc<AuditLog> {
        self.audit.clone()
    }

    pub fn active(&self) -> Option<Session> {
        let (lock, _) = self.store.load().ok().flatten()?;
        Some(lock_to_session(&lock))
    }

    pub fn list_modes(&self) -> Vec<crate::ipc::ModeSummary> {
        if let Ok(fresh) = Config::load() {
            *self.config.write() = fresh;
        }
        let cfg = self.config.read().clone();
        let events = self.audit.read_all().unwrap_or_default();
        let now = chrono::Utc::now();
        let default = cfg.general.default_profile.clone();
        cfg.profiles
            .iter()
            .map(|(name, p)| crate::ipc::ModeSummary {
                name: name.clone(),
                color: p.color.clone(),
                blocked_apps: p.apps.len(),
                blocked_sites: p.sites.len(),
                blocked_groups: p.site_groups.len(),
                limits: p.limits.clone(),
                stats: crate::audit::stats::mode_stats(&events, name, &p.limits, now),
                is_default: name == &default,
            })
            .collect()
    }

    pub fn mode_stats(&self, name: &str) -> Result<crate::audit::stats::ModeStats> {
        if let Ok(fresh) = Config::load() {
            *self.config.write() = fresh;
        }
        let cfg = self.config.read().clone();
        let profile =
            cfg.profile(name).ok_or_else(|| Error::Config(format!("unknown mode `{name}`")))?;
        let events = self.audit.read_all().unwrap_or_default();
        Ok(crate::audit::stats::mode_stats(&events, name, &profile.limits, chrono::Utc::now()))
    }

    pub fn save_mode(&self, name: String, profile: Profile) -> Result<()> {
        if name.is_empty() {
            return Err(Error::Config("mode name cannot be empty".into()));
        }
        let mut cfg = self.config.read().clone();
        cfg.profiles.insert(name, profile);
        cfg.validate()?;
        cfg.save()?;
        *self.config.write() = cfg;
        Ok(())
    }

    pub fn get_general(&self) -> crate::config::General {
        if let Ok(fresh) = Config::load() {
            *self.config.write() = fresh;
        }
        self.config.read().general.clone()
    }

    pub fn update_general(&self, general: crate::config::General) -> Result<()> {
        let mut cfg = self.config.read().clone();
        cfg.general = general;
        cfg.validate()?;
        cfg.save()?;
        *self.config.write() = cfg;
        Ok(())
    }

    pub fn reset_all(&self) -> Result<()> {
        if self.store.load()?.is_some() {
            return Err(Error::Config("cannot reset while a session is active".into()));
        }
        let cfg_path = crate::paths::config_file()?;
        if cfg_path.exists() {
            std::fs::remove_file(&cfg_path).ok();
        }
        let data_dir = crate::paths::data_dir()?;
        for name in [
            crate::audit::AUDIT_FILE,
            "audit.sqlite3-wal",
            "audit.sqlite3-shm",
            crate::audit::LEGACY_AUDIT_FILE,
            "audit.log.bak",
        ] {
            let p = data_dir.join(name);
            if p.exists() {
                std::fs::remove_file(&p).ok();
            }
        }
        let fresh = crate::config::Config::default();
        fresh.save()?;
        *self.config.write() = fresh;
        Ok(())
    }

    pub fn delete_mode(&self, name: &str) -> Result<()> {
        let mut cfg = self.config.read().clone();
        if cfg.profiles.remove(name).is_none() {
            return Err(Error::Config(format!("mode `{name}` not found")));
        }
        if cfg.general.default_profile == name {
            cfg.general.default_profile = cfg.profiles.keys().next().cloned().unwrap_or_default();
        }
        cfg.save()?;
        *self.config.write() = cfg;
        Ok(())
    }

    pub fn hard_info(&self) -> Option<HardModeInfo> {
        let (lock, _) = self.store.load().ok().flatten()?;
        if !lock.hard_mode {
            return None;
        }
        Some(HardModeInfo {
            ends_at: lock.ends_at(),
            remaining: lock.remaining(),
            reason: lock.reason.clone(),
            panic_phrase: lock.panic_phrase.clone(),
            panic_requested_at: lock.panic_requested_at,
            panic_releases_at: lock.panic_releases_at(),
        })
    }

    pub fn start(
        &self,
        profile: String,
        duration: Duration,
        hard_mode: bool,
        reason: Option<String>,
        panic_phrase: String,
    ) -> Result<Session> {
        if let Some((existing, _)) = self.store.load()? {
            if !existing.is_expired() {
                return Err(Error::Other("a session is already running".into()));
            }
        }

        if let Ok(fresh) = Config::load() {
            *self.config.write() = fresh;
        }
        let cfg = self.config.read().clone();
        let profile_def = cfg
            .profile(&profile)
            .ok_or_else(|| Error::Config(format!("unknown profile `{profile}`")))?
            .clone();
        let duration = enforce_limits(&profile, &profile_def.limits, duration, &self.audit)?;
        let set = build_block_set(&profile_def)?;

        let lock = SessionLock::new(NewLock {
            profile: profile.clone(),
            duration,
            hard_mode,
            panic_delay: cfg.general.panic_delay,
            panic_phrase,
            reason,
            boot_id: clock::boot_id(),
            boot_ms: clock::monotonic_ms(),
        });

        if let Err(e) = self.hosts.lock().apply(&set) {
            if matches!(e, Error::Permission(_)) {
                tracing::warn!(?e, "hosts apply failed; continuing without site blocking");
            } else {
                return Err(e);
            }
        }
        let _ = self.procs.lock().kill_matching(&set.apps);
        self.store.save(&lock)?;
        *self.active_profile.write() = Some(profile.clone());
        self.last_tick_ms.store(clock::monotonic_ms() as u64, Ordering::SeqCst);
        self.audit.append_with(
            AuditKind::SessionStarted,
            Some(lock.id),
            &profile,
            session_claim(&lock),
        );

        Ok(lock_to_session(&lock))
    }

    pub fn stop(&self) -> Result<Option<Session>> {
        let Some((lock, _)) = self.store.load()? else {
            return Ok(None);
        };
        if lock.hard_mode && !lock.is_expired() && !lock.should_release_via_panic() {
            self.audit.append(AuditKind::StopDenied, Some(lock.id), "stop denied in hard mode");
            return Err(Error::HardModeActive);
        }
        self.finalize(&lock, SessionState::Aborted)?;
        Ok(Some(lock_to_session(&lock)))
    }

    pub fn panic(&self, phrase: &str, cancel: bool) -> Result<SessionLock> {
        let Some((mut lock, kind)) = self.store.load()? else {
            return Err(Error::Other("no active session".into()));
        };
        if matches!(kind, LoadKind::TamperedPrimary | LoadKind::TamperedBackup) {
            self.handle_tamper(&mut lock);
        }
        if cancel {
            lock.cancel_panic();
            self.store.save(&lock)?;
            self.audit.append(AuditKind::PanicCancelled, Some(lock.id), "panic cancelled");
            return Ok(lock);
        }
        if phrase != lock.panic_phrase {
            self.audit.append(AuditKind::StopDenied, Some(lock.id), "bad panic phrase");
            return Err(Error::Other("phrase does not match".into()));
        }
        lock.request_panic();
        self.store.save(&lock)?;
        self.audit.append(AuditKind::PanicRequested, Some(lock.id), "panic scheduled");
        Ok(lock)
    }

    pub fn tick(&self) -> Result<()> {
        let Some((mut lock, kind)) = self.store.load()? else {
            *self.active_profile.write() = None;
            return Ok(());
        };

        let now_ms = clock::monotonic_ms() as u64;
        let prev = self.last_tick_ms.swap(now_ms, Ordering::SeqCst);
        let delta = clock::bounded_delta(u128::from(prev), u128::from(now_ms));

        if matches!(kind, LoadKind::TamperedPrimary | LoadKind::TamperedBackup) {
            self.handle_tamper(&mut lock);
            self.store.save(&lock)?;
            return Ok(());
        }

        lock.advance(delta);

        if lock.should_release_via_panic() {
            self.finalize(&lock, SessionState::Aborted)?;
            self.audit.append(AuditKind::SessionPanicked, Some(lock.id), "panic released");
            return Ok(());
        }

        if lock.is_expired() {
            self.finalize(&lock, SessionState::Completed)?;
            self.audit.append_with(
                AuditKind::SessionCompleted,
                Some(lock.id),
                &lock.profile,
                serde_json::json!({ "duration_ms": lock.duration_ms }),
            );
            return Ok(());
        }

        let cfg = self.config.read().clone();
        if let Some(profile) = cfg.profile(&lock.profile).cloned() {
            match build_block_set(&profile) {
                Ok(set) => {
                    let mut hosts = self.hosts.lock();
                    if let Err(e) = hosts.apply(&set) {
                        tracing::warn!(?e, "hosts reapply failed");
                    } else {
                        self.audit.append(AuditKind::HostsRepaired, Some(lock.id), "hosts ensured");
                    }
                    drop(hosts);
                    let _ = self.procs.lock().kill_matching(&set.apps);
                }
                Err(e) => tracing::warn!(?e, "block set build failed"),
            }
        }

        self.store.save(&lock)?;
        *self.active_profile.write() = Some(lock.profile.clone());
        Ok(())
    }

    pub fn restore(&self) -> Result<()> {
        let loaded = self.store.load()?;
        let loaded = match loaded {
            Some(l) => Some(l),
            None => match self.reconstruct_from_audit()? {
                Some(lock) => {
                    self.store.save(&lock)?;
                    self.audit.append(
                        AuditKind::SessionReconstructed,
                        Some(lock.id),
                        &lock.profile,
                    );
                    Some((lock, crate::session::LoadKind::Valid))
                }
                None => None,
            },
        };
        let Some((mut lock, kind)) = loaded else { return Ok(()) };
        if matches!(kind, LoadKind::TamperedPrimary | LoadKind::TamperedBackup) {
            self.handle_tamper(&mut lock);
            self.store.save(&lock)?;
        }
        if lock.is_expired() {
            self.finalize(&lock, SessionState::Completed)?;
            return Ok(());
        }
        let cfg = self.config.read().clone();
        if let Some(profile) = cfg.profile(&lock.profile).cloned() {
            if let Ok(set) = build_block_set(&profile) {
                let _ = self.hosts.lock().apply(&set);
            }
        }
        *self.active_profile.write() = Some(lock.profile.clone());
        self.audit.append(AuditKind::DaemonRestarted, Some(lock.id), "lock restored");
        Ok(())
    }

    fn finalize(&self, lock: &SessionLock, _state: SessionState) -> Result<()> {
        let _ = self.hosts.lock().revert();
        self.store.delete()?;
        *self.active_profile.write() = None;
        tracing::info!(id = %lock.id, "session finalized");
        Ok(())
    }

    fn reconstruct_from_audit(&self) -> Result<Option<SessionLock>> {
        let events = self.audit.read_all().unwrap_or_default();
        let mut last_start: Option<&crate::audit::AuditEvent> = None;
        for e in &events {
            match e.kind {
                AuditKind::SessionStarted => last_start = Some(e),
                AuditKind::SessionCompleted
                | AuditKind::SessionPanicked
                | AuditKind::SessionReconstructed => {
                    if let Some(start) = last_start {
                        if e.session_id == start.session_id {
                            last_start = None;
                        }
                    }
                }
                _ => {}
            }
        }
        let Some(start) = last_start else { return Ok(None) };
        let extra = &start.extra;
        let id = start.session_id.unwrap_or_else(uuid::Uuid::new_v4);
        let profile = start.message.clone();
        let duration_ms = extra.get("duration_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        if duration_ms == 0 {
            return Ok(None);
        }
        let hard_mode = extra.get("hard_mode").and_then(|v| v.as_bool()).unwrap_or(false);
        let panic_delay_ms = extra.get("panic_delay_ms").and_then(|v| v.as_u64()).unwrap_or(0);
        let panic_phrase =
            extra.get("panic_phrase").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let boot_id = extra.get("boot_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let boot_ms = extra.get("boot_ms").and_then(|v| v.as_u64()).unwrap_or(0) as u128;
        let reason =
            extra.get("reason").and_then(|v| v.as_str()).map(std::string::ToString::to_string);

        let now = chrono::Utc::now();
        let elapsed = now.signed_duration_since(start.at).num_milliseconds().max(0) as u128;
        if elapsed >= u128::from(duration_ms) {
            return Ok(None);
        }

        let mut lock = SessionLock {
            schema_version: crate::session::LOCK_SCHEMA,
            id,
            profile,
            started_at: start.at,
            started_at_boot_ms: boot_ms,
            boot_id,
            duration_ms: u128::from(duration_ms),
            progressed_ms: elapsed,
            hard_mode,
            panic_requested_at: None,
            panic_delay_ms: u128::from(panic_delay_ms),
            panic_phrase,
            reason,
            penalty_applied_ms: 0,
            mac: String::new(),
        };
        lock.reseal();
        tracing::warn!(id = %lock.id, "session lock reconstructed from audit trail");
        Ok(Some(lock))
    }

    fn handle_tamper(&self, lock: &mut SessionLock) {
        let penalty = self.config.read().general.tamper_penalty;
        lock.apply_penalty(penalty);
        self.audit.append(
            AuditKind::TamperPenalty,
            Some(lock.id),
            &format!("+{}s", penalty.as_secs()),
        );
    }
}

fn enforce_limits(
    profile: &str,
    limits: &Limits,
    requested: Duration,
    audit: &AuditLog,
) -> Result<Duration> {
    let mut duration = requested;
    if let Some(max) = limits.max_duration {
        if duration > max {
            duration = max;
        }
    }
    if let Some(min) = limits.min_duration {
        if duration < min {
            return Err(Error::Config(format!(
                "profile `{profile}` requires at least {}",
                humantime::format_duration(min)
            )));
        }
    }
    let events = audit.read_all().unwrap_or_default();
    let stats = crate::audit::stats::mode_stats(&events, profile, limits, chrono::Utc::now());
    if let Some(remaining) = stats.cooldown_remaining {
        return Err(Error::Config(format!(
            "profile `{profile}` cooldown active ({}s remaining)",
            remaining.as_secs()
        )));
    }
    if let (Some(cap), Some(remaining)) = (limits.daily_cap, stats.daily_cap_remaining) {
        if remaining.is_zero() || duration > remaining {
            return Err(Error::Config(format!(
                "profile `{profile}` daily cap reached ({})",
                humantime::format_duration(cap)
            )));
        }
    }
    Ok(duration)
}

fn session_claim(lock: &SessionLock) -> serde_json::Value {
    serde_json::json!({
        "duration_ms": u64::try_from(lock.duration_ms).unwrap_or(u64::MAX),
        "hard_mode": lock.hard_mode,
        "panic_delay_ms": u64::try_from(lock.panic_delay_ms).unwrap_or(u64::MAX),
        "panic_phrase": lock.panic_phrase,
        "boot_id": lock.boot_id,
        "boot_ms": u64::try_from(lock.started_at_boot_ms).unwrap_or(u64::MAX),
        "reason": lock.reason,
        "mac": lock.mac,
    })
}

fn build_block_set(profile: &Profile) -> Result<BlockSet> {
    let mut hosts: std::collections::BTreeSet<String> =
        profile.sites.iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect();
    for host in sites::expand_groups(&profile.site_groups)? {
        hosts.insert(host);
    }
    let cache = match AppCache::load()? {
        Some(c) => c,
        None => apps::load_or_scan(false)?,
    };
    let resolution = apps::resolve(&profile.apps, &cache);
    if !resolution.stale.is_empty() {
        tracing::warn!(stale = ?resolution.stale, "profile references uninstalled apps");
    }
    Ok(BlockSet { sites: hosts.into_iter().collect(), apps: resolution.resolved })
}

fn lock_to_session(lock: &SessionLock) -> Session {
    Session {
        id: lock.id,
        profile: lock.profile.clone(),
        started_at: lock.started_at,
        duration: Duration::from_millis(u64::try_from(lock.duration_ms).unwrap_or(u64::MAX)),
        hard_mode: lock.hard_mode,
        state: if lock.is_expired() { SessionState::Completed } else { SessionState::Running },
    }
}

#[allow(dead_code)]
fn _unused(_: Uuid) {}
