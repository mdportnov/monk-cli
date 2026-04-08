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
    blocker::{BlockSet, Blocker, HostsBlocker, ProcessGuard},
    clock,
    config::{Config, Profile},
    ipc::HardModeInfo,
    session::{LoadKind, LockStore, NewLock, Session, SessionLock, SessionState},
    sites, Error, Result,
};

#[derive(Debug)]
pub struct Supervisor {
    config: Arc<RwLock<Config>>,
    hosts: Arc<Mutex<HostsBlocker>>,
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
            hosts: Arc::new(Mutex::new(HostsBlocker::default())),
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

    pub fn audit(&self) -> Arc<AuditLog> {
        self.audit.clone()
    }

    pub fn active(&self) -> Option<Session> {
        let (lock, _) = self.store.load().ok().flatten()?;
        Some(lock_to_session(&lock))
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

        let cfg = self.config.read().clone();
        let profile_def = cfg
            .profile(&profile)
            .ok_or_else(|| Error::Config(format!("unknown profile `{profile}`")))?
            .clone();
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

        self.hosts.lock().apply(&set)?;
        let _ = self.procs.lock().kill_matching(&set.apps);
        self.store.save(&lock)?;
        *self.active_profile.write() = Some(profile.clone());
        self.last_tick_ms.store(clock::monotonic_ms() as u64, Ordering::SeqCst);
        self.audit.append(AuditKind::SessionStarted, Some(lock.id), &profile);

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
            self.audit.append(AuditKind::SessionCompleted, Some(lock.id), &lock.profile);
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
                        self.audit.append(
                            AuditKind::HostsRepaired,
                            Some(lock.id),
                            "hosts ensured",
                        );
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
        let Some((mut lock, kind)) = self.store.load()? else { return Ok(()) };
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

fn build_block_set(profile: &Profile) -> Result<BlockSet> {
    let mut hosts: std::collections::BTreeSet<String> = profile
        .sites
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
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
