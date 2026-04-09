mod hosts;
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

pub use hosts::HostsBlocker;
pub use process::ProcessGuard;

#[derive(Debug, Clone, Default)]
pub struct BlockSet {
    pub sites: Vec<String>,
    pub apps: Vec<InstalledApp>,
}

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
    let hosts = hosts_path();
    let writable =
        fs_err::OpenOptions::new().append(true).open(&hosts).is_ok();
    if writable {
        tracing::info!(backend = "hosts", "selected site blocker backend");
        Box::<HostsBlocker>::default()
    } else {
        tracing::warn!(
            path = %hosts.display(),
            "hosts file not writable; running without site blocking (noop backend)"
        );
        Box::new(NoopBlocker)
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
