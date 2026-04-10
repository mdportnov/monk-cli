use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::{
    blocker::{
        backends::{atomic_write, BlockerBackend, ProbeResult},
        BlockSet, Blocker,
    },
    Error, Result,
};

const MARKER: &str = "# monk-managed";
const DEFAULT_ROOT: &str = "/etc/resolver";
const DEFAULT_PORT: u16 = 53535;

#[derive(Debug)]
pub struct ResolverDirBlocker {
    root: PathBuf,
    port: u16,
}

impl Default for ResolverDirBlocker {
    fn default() -> Self {
        Self { root: PathBuf::from(DEFAULT_ROOT), port: DEFAULT_PORT }
    }
}

impl ResolverDirBlocker {
    #[allow(dead_code)]
    pub fn with_root(root: PathBuf) -> Self {
        Self { root, port: DEFAULT_PORT }
    }

    fn file_for(&self, domain: &str) -> PathBuf {
        let safe_domain: String = domain
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        if safe_domain.is_empty() || safe_domain != domain {
            return self.root.join("invalid");
        }
        self.root.join(safe_domain)
    }

    fn render(&self) -> String {
        format!("{}\nnameserver 127.0.0.1\nport {}\n", MARKER, self.port)
    }

    fn is_ours(path: &Path) -> bool {
        match fs_err::read_to_string(path) {
            Ok(s) => s.lines().next().map(|l| l.trim() == MARKER).unwrap_or(false),
            Err(_) => false,
        }
    }

    fn sweep(&self) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(rd) = fs_err::read_dir(&self.root) else { return out };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_file() && Self::is_ours(&p) {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    out.push(name.to_string());
                }
            }
        }
        out
    }

}

impl Blocker for ResolverDirBlocker {
    fn name(&self) -> &'static str {
        "resolver_dir"
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

        let desired: BTreeSet<String> = set
            .sites
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .collect();

        let existing: BTreeSet<String> = self.sweep().into_iter().collect();

        for stale in existing.difference(&desired) {
            let p = self.file_for(stale);
            if Self::is_ours(&p) {
                let _ = fs_err::remove_file(&p);
            }
        }

        let content = self.render();
        for domain in &desired {
            let p = self.file_for(domain);
            atomic_write(&p, content.as_bytes())?;
        }
        Ok(())
    }

    fn revert(&mut self) -> Result<()> {
        for name in self.sweep() {
            let p = self.file_for(&name);
            if Self::is_ours(&p) {
                let _ = fs_err::remove_file(&p);
            }
        }
        Ok(())
    }
}

impl BlockerBackend for ResolverDirBlocker {
    fn probe() -> ProbeResult {
        if !nix::unistd::geteuid().is_root() {
            return ProbeResult::Unavailable { reason: "requires root".into() };
        }
        let root = Path::new(DEFAULT_ROOT);
        if !root.exists() && fs_err::create_dir_all(root).is_err() {
            return ProbeResult::Unavailable {
                reason: format!("cannot create {}", root.display()),
            };
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

    fn make() -> (tempfile::TempDir, ResolverDirBlocker) {
        let dir = tempfile::tempdir().unwrap();
        let b = ResolverDirBlocker::with_root(dir.path().to_path_buf());
        (dir, b)
    }

    #[test]
    fn writes_and_reverts_marker_files() {
        let (_dir, mut b) = make();
        b.apply(&BlockSet {
            sites: vec!["example.com".into(), "foo.test".into()],
            apps: vec![],
        })
        .unwrap();
        assert!(b.root.join("example.com").exists());
        assert!(b.root.join("foo.test").exists());
        let body = fs_err::read_to_string(b.root.join("example.com")).unwrap();
        assert!(body.starts_with(MARKER));
        assert!(body.contains("nameserver 127.0.0.1"));
        b.revert().unwrap();
        assert!(!b.root.join("example.com").exists());
        assert!(!b.root.join("foo.test").exists());
    }

    #[test]
    fn preserves_foreign_files() {
        let (_dir, mut b) = make();
        let foreign = b.root.join("bbc.com");
        fs_err::write(&foreign, "nameserver 8.8.8.8\n").unwrap();
        b.apply(&BlockSet { sites: vec!["example.com".into()], apps: vec![] }).unwrap();
        b.revert().unwrap();
        assert!(foreign.exists());
        assert_eq!(fs_err::read_to_string(&foreign).unwrap(), "nameserver 8.8.8.8\n");
    }

    #[test]
    fn shrinks_on_reapply() {
        let (_dir, mut b) = make();
        b.apply(&BlockSet {
            sites: vec!["a.com".into(), "b.com".into()],
            apps: vec![],
        })
        .unwrap();
        b.apply(&BlockSet { sites: vec!["a.com".into()], apps: vec![] }).unwrap();
        assert!(b.root.join("a.com").exists());
        assert!(!b.root.join("b.com").exists());
    }

    #[test]
    fn conformance() {
        let (_dir, mut b) = make();
        crate::blocker::backends::assert_conformance(&mut b);
    }

    #[test]
    fn rejects_path_traversal() {
        let (_dir, b) = make();
        let bad_paths = ["../evil", "sub/../../../etc/passwd", "foo/bar", "test\\windows"];
        for bad in bad_paths {
            let result = b.file_for(bad);
            assert_eq!(result.file_name().unwrap(), "invalid");
        }
        let good = b.file_for("example.com");
        assert_eq!(good.file_name().unwrap(), "example.com");
    }
}
