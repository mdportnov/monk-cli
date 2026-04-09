pub(crate) mod backends;
pub mod dns_server;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod linux;
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod macos;
mod process;
#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod windows;

use std::path::PathBuf;

use crate::{apps::InstalledApp, Result};

pub use backends::hosts::HostsBlocker;
pub use backends::{cleanup_all as cleanup_all_backends, BlockerBackend, ProbeResult};
pub use process::ProcessGuard;

#[derive(Debug, Clone, Default)]
pub struct BlockSet {
    pub sites: Vec<String>,
    pub apps: Vec<InstalledApp>,
}

/// Stable contract for a site-blocker backend.
///
/// ## Semantics
///
/// - `apply(set)` has **set-to** (declarative) semantics. After a successful
///   call, the OS state reflects exactly `set`: any entries written by a
///   previous `apply` on the same instance that are not in the new `set` MUST
///   be removed. Implementations MAY no-op when `set.sites` is empty
///   (interpret empty as "ignore", not "clear") — use `revert` to clear.
///
/// - `apply` MUST be **idempotent**: calling it twice with the same `set`
///   leaves the same observable OS state as a single call.
///
/// - `revert` MUST restore the pre-`apply` state and MUST be idempotent.
///   Calling `revert` on a freshly-constructed instance (that never called
///   `apply`) is a no-op. `revert` MUST tolerate:
///     - missing system resources (e.g. `/etc/hosts` not found)
///     - foreign files the backend does not own (leave them alone)
///     - a previous crash mid-apply (partial state)
///
/// - Implementations MAY only mutate files or resources they own, identified
///   by a marker token written at apply-time.
///
/// ## Lifecycle
///
/// The daemon calls `cleanup_all_backends()` at startup, which invokes
/// `revert` on every known backend via `Default`. Any backend added to the
/// registry MUST therefore implement `Default` and have a `revert` that is
/// safe to call from a never-applied state.
pub trait Blocker: Send + Sync + std::fmt::Debug {
    fn name(&self) -> &'static str;
    fn apply(&mut self, set: &BlockSet) -> Result<()>;
    fn revert(&mut self) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoopBlocker;

impl Blocker for NoopBlocker {
    fn name(&self) -> &'static str {
        "noop"
    }
    fn apply(&mut self, _set: &BlockSet) -> Result<()> {
        Ok(())
    }
    fn revert(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn select_site_blocker() -> Box<dyn Blocker> {
    let experimental = std::env::var("MONK_DNS_BACKEND").is_ok();

    if experimental {
        #[cfg(target_os = "macos")]
        {
            use backends::resolver_dir::ResolverDirBlocker;
            match ResolverDirBlocker::probe() {
                ProbeResult::Available { detail, .. } => match ResolverDirBlocker::build() {
                    Ok(b) => {
                        tracing::info!(backend = "resolver_dir", %detail, "selected site blocker backend");
                        return Box::new(b);
                    }
                    Err(e) => tracing::warn!(?e, "resolver_dir build failed"),
                },
                ProbeResult::Unavailable { reason } => {
                    tracing::debug!(%reason, "resolver_dir unavailable");
                }
            }
        }
        #[cfg(target_os = "linux")]
        {
            use backends::systemd_resolved::SystemdResolvedBlocker;
            match SystemdResolvedBlocker::probe() {
                ProbeResult::Available { detail, .. } => match SystemdResolvedBlocker::build() {
                    Ok(b) => {
                        tracing::info!(backend = "systemd_resolved", %detail, "selected site blocker backend");
                        return Box::new(b);
                    }
                    Err(e) => tracing::warn!(?e, "systemd_resolved build failed"),
                },
                ProbeResult::Unavailable { reason } => {
                    tracing::debug!(%reason, "systemd_resolved unavailable");
                }
            }
        }
    }

    match HostsBlocker::probe() {
        ProbeResult::Available { detail, .. } => match HostsBlocker::build() {
            Ok(b) => {
                tracing::info!(backend = "hosts", %detail, "selected site blocker backend");
                Box::new(b)
            }
            Err(e) => {
                tracing::warn!(?e, "hosts build failed; running noop");
                Box::new(NoopBlocker)
            }
        },
        ProbeResult::Unavailable { reason } => {
            tracing::warn!(%reason, "hosts unavailable; running noop");
            Box::new(NoopBlocker)
        }
    }
}

pub fn hosts_path() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\Windows\System32\drivers\etc\hosts")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/etc/hosts")
    }
}
