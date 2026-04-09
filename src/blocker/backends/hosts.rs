use std::path::PathBuf;

use crate::{
    blocker::{
        backends::{atomic_write, BlockerBackend, ProbeResult},
        hosts_path, BlockSet, Blocker,
    },
    Error, Result,
};

const BEGIN: &str = "# >>> monk begin >>>";
const END: &str = "# <<< monk end <<<";

#[derive(Debug)]
pub struct HostsBlocker {
    path: PathBuf,
    backup: Option<String>,
}

impl Default for HostsBlocker {
    fn default() -> Self {
        Self { path: hosts_path(), backup: None }
    }
}

impl HostsBlocker {
    pub fn with_path(path: PathBuf) -> Self {
        Self { path, backup: None }
    }

    fn read(&self) -> Result<String> {
        fs_err::read_to_string(&self.path).map_err(Error::from)
    }

    fn write(&self, contents: &str) -> Result<()> {
        atomic_write(&self.path, contents.as_bytes())
    }

    fn strip_block(raw: &str) -> String {
        let mut out = String::with_capacity(raw.len());
        let mut skipping = false;
        for line in raw.lines() {
            if line.trim() == BEGIN {
                skipping = true;
                continue;
            }
            if line.trim() == END {
                skipping = false;
                continue;
            }
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    fn render_block(set: &BlockSet) -> String {
        let mut s = String::new();
        s.push_str(BEGIN);
        s.push('\n');
        for host in &set.sites {
            let host = host.trim();
            if host.is_empty() || host.starts_with('#') {
                continue;
            }
            s.push_str(&format!("127.0.0.1 {host}\n"));
            s.push_str(&format!("::1       {host}\n"));
            if !host.starts_with("www.") {
                s.push_str(&format!("127.0.0.1 www.{host}\n"));
                s.push_str(&format!("::1       www.{host}\n"));
            }
        }
        s.push_str(END);
        s.push('\n');
        s
    }
}

impl Blocker for HostsBlocker {
    fn name(&self) -> &'static str {
        "hosts"
    }

    fn apply(&mut self, set: &BlockSet) -> Result<()> {
        if set.sites.is_empty() {
            return Ok(());
        }
        let current = self.read()?;
        if self.backup.is_none() {
            self.backup = Some(current.clone());
        }
        let cleaned = Self::strip_block(&current);
        let mut next = cleaned.trim_end().to_string();
        next.push_str("\n\n");
        next.push_str(&Self::render_block(set));
        self.write(&next)
    }

    fn revert(&mut self) -> Result<()> {
        let current = match self.read() {
            Ok(c) => c,
            Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                self.backup = None;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let cleaned = Self::strip_block(&current);
        self.write(cleaned.trim_end())?;
        self.backup = None;
        Ok(())
    }
}

impl BlockerBackend for HostsBlocker {
    fn probe() -> ProbeResult {
        let path = hosts_path();
        match fs_err::OpenOptions::new().write(true).open(&path) {
            Ok(_) => ProbeResult::Available {
                priority: 10,
                detail: path.display().to_string(),
            },
            Err(e) => ProbeResult::Unavailable {
                reason: format!("{} not writable: {e}", path.display()),
            },
        }
    }

    fn build() -> Result<Self> {
        Ok(Self::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_strip() {
        let raw = "127.0.0.1 localhost\n# >>> monk begin >>>\n127.0.0.1 x.com\n# <<< monk end <<<\nother\n";
        assert_eq!(HostsBlocker::strip_block(raw), "127.0.0.1 localhost\nother\n");
    }

    #[test]
    fn apply_and_revert_in_tempfile() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosts");
        fs_err::write(&p, "127.0.0.1 localhost\n").unwrap();
        let mut b = HostsBlocker::with_path(p.clone());
        b.apply(&BlockSet { sites: vec!["x.com".into()], apps: vec![] }).unwrap();
        let after = fs_err::read_to_string(&p).unwrap();
        assert!(after.contains("127.0.0.1 x.com"));
        b.revert().unwrap();
        let reverted = fs_err::read_to_string(&p).unwrap();
        assert!(!reverted.contains("x.com"));
        assert!(reverted.contains("localhost"));
    }

    #[test]
    fn conformance() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosts");
        fs_err::write(&p, "127.0.0.1 localhost\n").unwrap();
        let mut b = HostsBlocker::with_path(p);
        crate::blocker::backends::assert_conformance(&mut b);
    }
}
