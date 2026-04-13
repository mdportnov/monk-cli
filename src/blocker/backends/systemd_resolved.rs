use std::path::{Path, PathBuf};

use crate::{
    blocker::{
        backends::{atomic_write, BlockerBackend, ProbeResult},
        BlockSet, Blocker,
    },
    Error, Result,
};

const HEADER: &str = "# monk-managed — do not edit\n";
const DEFAULT_ROOT: &str = "/etc/systemd/resolved.conf.d";
const FILENAME: &str = "monk.conf";
const DEFAULT_PORT: u16 = 53535;

#[derive(Debug)]
pub struct SystemdResolvedBlocker {
    root: PathBuf,
    port: u16,
    reload: bool,
}

impl Default for SystemdResolvedBlocker {
    fn default() -> Self {
        Self { root: PathBuf::from(DEFAULT_ROOT), port: DEFAULT_PORT, reload: true }
    }
}

impl SystemdResolvedBlocker {
    #[cfg(test)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root, port: DEFAULT_PORT, reload: false }
    }

    fn file_path(&self) -> PathBuf {
        self.root.join(FILENAME)
    }

    fn render(&self, domains: &[String]) -> String {
        let mut s = String::new();
        s.push_str(HEADER);
        s.push_str("[Resolve]\n");
        s.push_str(&format!("DNS=127.0.0.1:{}\n", self.port));
        s.push_str("Domains=");
        let parts: Vec<String> = domains.iter().map(|d| format!("~{d}")).collect();
        s.push_str(&parts.join(" "));
        s.push('\n');
        s
    }

    fn reload_resolved(&self) {
        if !self.reload {
            return;
        }
        let status = std::process::Command::new("systemctl")
            .args(["reload-or-restart", "systemd-resolved"])
            .status();
        if let Err(e) = status {
            tracing::warn!(?e, "failed to reload systemd-resolved");
        }
    }
}

impl Blocker for SystemdResolvedBlocker {
    fn name(&self) -> &'static str {
        "systemd_resolved"
    }

    fn apply(&mut self, set: &BlockSet) -> Result<()> {
        if set.sites.is_empty() {
            return Ok(());
        }
        if !self.root.exists() {
            fs_err::create_dir_all(&self.root).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    Error::Permission(format!("cannot create {}", self.root.display()))
                } else {
                    Error::Io(e)
                }
            })?;
        }
        let domains: Vec<String> = set
            .sites
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .collect();
        let content = self.render(&domains);
        let path = self.file_path();
        atomic_write(&path, content.as_bytes())?;
        self.reload_resolved();
        Ok(())
    }

    fn revert(&mut self) -> Result<()> {
        let path = self.file_path();
        if !path.exists() {
            return Ok(());
        }
        let is_ours =
            fs_err::read_to_string(&path).map(|s| s.contains("monk-managed")).unwrap_or(false);
        if is_ours {
            let _ = fs_err::remove_file(&path);
            self.reload_resolved();
        }
        Ok(())
    }
}

impl BlockerBackend for SystemdResolvedBlocker {
    fn probe() -> ProbeResult {
        if !Path::new("/run/systemd/resolve").exists() {
            return ProbeResult::Unavailable { reason: "systemd-resolved not active".into() };
        }
        if !nix::unistd::geteuid().is_root() {
            return ProbeResult::Unavailable { reason: "requires root".into() };
        }
        ProbeResult::Available { priority: 80, detail: DEFAULT_ROOT.into() }
    }

    fn build() -> Result<Self> {
        Ok(Self::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make() -> (tempfile::TempDir, SystemdResolvedBlocker) {
        let dir = tempfile::tempdir().unwrap();
        let b = SystemdResolvedBlocker::with_root(dir.path().to_path_buf());
        (dir, b)
    }

    #[test]
    fn writes_dropin_with_routing_domains() {
        let (_dir, mut b) = make();
        b.apply(&BlockSet { sites: vec!["example.com".into(), "foo.test".into()], apps: vec![] })
            .unwrap();
        let content = fs_err::read_to_string(b.file_path()).unwrap();
        assert!(content.contains("monk-managed"));
        assert!(content.contains("DNS=127.0.0.1:53535"));
        assert!(content.contains("~example.com"));
        assert!(content.contains("~foo.test"));
    }

    #[test]
    fn revert_removes_our_file_only() {
        let (_dir, mut b) = make();
        let foreign = b.root.join("foreign.conf");
        fs_err::write(&foreign, "[Resolve]\nDNS=1.1.1.1\n").unwrap();
        b.apply(&BlockSet { sites: vec!["example.com".into()], apps: vec![] }).unwrap();
        b.revert().unwrap();
        assert!(!b.file_path().exists());
        assert!(foreign.exists());
    }

    #[test]
    fn conformance() {
        let (_dir, mut b) = make();
        crate::blocker::backends::assert_conformance(&mut b);
    }
}
