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
    fn apply(&mut self, set: &BlockSet) -> Result<()>;
    fn revert(&mut self) -> Result<()>;
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
