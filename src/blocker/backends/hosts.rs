use std::path::PathBuf;

use crate::{
    blocker::{
        backends::{atomic_write, BlockerBackend, ProbeResult},
        hosts_path, BlockSet, Blocker,
    },
    Error, Result,
};

#[cfg(target_os = "macos")]
use tracing::{debug, warn};

const BEGIN: &str = "# >>> monk begin >>>";
const END: &str = "# <<< monk end <<<";

const DOH_BOOTSTRAP_HOSTS: &[&str] = &[
    "cloudflare-dns.com",
    "one.one.one.one",
    "mozilla.cloudflare-dns.com",
    "chrome.cloudflare-dns.com",
    "security.cloudflare-dns.com",
    "family.cloudflare-dns.com",
    "dns.google",
    "dns.google.com",
    "dns.quad9.net",
    "dns10.quad9.net",
    "dns11.quad9.net",
    "doh.opendns.com",
    "doh.familyshield.opendns.com",
    "doh.umbrella.com",
    "dns.nextdns.io",
    "dns.adguard.com",
    "dns-family.adguard.com",
    "dns-unfiltered.adguard.com",
    "doh.cleanbrowsing.org",
    "mask.icloud.com",
    "mask-h2.icloud.com",
    "mask-api.icloud.com",
];

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

    #[cfg(unix)]
    fn ensure_world_readable_mode(&self) -> Result<bool> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = fs_err::metadata(&self.path)?;
        let mode = meta.mode() & 0o7777;
        if mode & 0o044 == 0o044 {
            return Ok(true);
        }
        let target = (mode & 0o7700) | 0o644;
        match fs_err::set_permissions(&self.path, std::fs::Permissions::from_mode(target)) {
            Ok(()) => {
                tracing::warn!(
                    from = format!("{:o}", mode),
                    to = format!("{:o}", target),
                    "hosts file lacked world-read; chmod'd so system resolver can read it"
                );
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(?e, "failed to chmod hosts; resolver may not see blocks");
                Ok(false)
            }
        }
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
        s.push_str("# doh/dot bootstrap — forces browsers back to system dns\n");
        for host in DOH_BOOTSTRAP_HOSTS {
            s.push_str(&format!("127.0.0.1 {host}\n"));
            s.push_str(&format!("::1       {host}\n"));
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
        let content_unchanged = next == current;
        #[cfg(unix)]
        let mode_ok = self.ensure_world_readable_mode().unwrap_or(false);
        #[cfg(not(unix))]
        let mode_ok = true;
        if content_unchanged && mode_ok {
            return Ok(());
        }
        let result = self.write(&next);
        if result.is_ok() {
            #[cfg(target_os = "macos")]
            return flush_dns_cache();
            #[cfg(not(target_os = "macos"))]
            flush_system_dns();
        }
        result
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
        let result = self.write(cleaned.trim_end());
        result?;
        #[cfg(target_os = "macos")]
        flush_dns_cache()?;
        #[cfg(not(target_os = "macos"))]
        flush_system_dns();
        self.backup = None;
        Ok(())
    }
}

impl BlockerBackend for HostsBlocker {
    fn probe() -> ProbeResult {
        let path = hosts_path();
        match fs_err::OpenOptions::new().write(true).open(&path) {
            Ok(_) => ProbeResult::Available { priority: 10, detail: path.display().to_string() },
            Err(e) => {
                ProbeResult::Unavailable { reason: format!("{} not writable: {e}", path.display()) }
            }
        }
    }

    fn build() -> Result<Self> {
        Ok(Self::default())
    }
}

/// Best-effort resolver-cache flush after a hosts mutation on non-macOS
/// platforms. Unlike macOS (where a flush failure propagates), here the block
/// is already applied, so flush failures are logged and swallowed — fail-closed
/// means the block stays in place regardless.
#[cfg(target_os = "linux")]
fn flush_system_dns() {
    if let Err(e) = crate::blocker::linux::flush_dns() {
        debug!(?e, "linux DNS flush failed (ignored)");
    }
}

#[cfg(target_os = "windows")]
fn flush_system_dns() {
    if let Err(e) = crate::blocker::windows::flush_dns() {
        debug!(?e, "windows DNS flush failed (ignored)");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn flush_system_dns() {}

#[cfg(target_os = "macos")]
fn flush_dns_cache() -> Result<()> {
    debug!("Flushing DNS cache");
    let mut dsc_ok = false;
    let mut mdns_ok = false;

    match std::process::Command::new("dscacheutil").arg("-flushcache").status() {
        Ok(status) if status.success() => {
            debug!("dscacheutil -flushcache: ok");
            dsc_ok = true;
        }
        Ok(status) => warn!("dscacheutil -flushcache exited with: {}", status),
        Err(e) => warn!("Failed to run dscacheutil -flushcache: {}", e),
    }

    match std::process::Command::new("killall").arg("-HUP").arg("mDNSResponder").status() {
        Ok(status) if status.success() => {
            debug!("killall -HUP mDNSResponder: ok");
            mdns_ok = true;
        }
        Ok(status) => warn!("killall -HUP mDNSResponder exited with: {}", status),
        Err(e) => warn!("Failed to run killall -HUP mDNSResponder: {}", e),
    }

    if dsc_ok || mdns_ok {
        Ok(())
    } else {
        Err(Error::Other("Failed to flush DNS cache with both dscacheutil and killall".into()))
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
    fn doh_bootstrap_hosts_injected() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("hosts");
        fs_err::write(&p, "127.0.0.1 localhost\n").unwrap();
        let mut b = HostsBlocker::with_path(p.clone());
        b.apply(&BlockSet { sites: vec!["x.com".into()], apps: vec![] }).unwrap();
        let after = fs_err::read_to_string(&p).unwrap();
        assert!(after.contains("127.0.0.1 dns.google"));
        assert!(after.contains("127.0.0.1 cloudflare-dns.com"));
        assert!(after.contains("127.0.0.1 mask.icloud.com"));
        b.revert().unwrap();
        let reverted = fs_err::read_to_string(&p).unwrap();
        assert!(!reverted.contains("dns.google"));
        assert!(!reverted.contains("cloudflare-dns.com"));
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
