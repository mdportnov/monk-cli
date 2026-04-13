use std::path::Path;

use super::Blocker;
use crate::{Error, Result};

#[derive(Debug)]
pub enum ProbeResult {
    Available { priority: u8, detail: String },
    Unavailable { reason: String },
}

pub trait BlockerBackend: Blocker + Sized {
    fn probe() -> ProbeResult;
    fn build() -> Result<Self>;
}

pub mod hosts;
#[cfg(target_os = "macos")]
pub mod resolver_dir;
#[cfg(target_os = "linux")]
pub mod systemd_resolved;

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("no parent for {}", path.display()),
        ))
    })?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let tmp = parent.join(format!(".{file_name}.monk-tmp.{}", std::process::id()));
    let map_err = |e: std::io::Error, p: &Path| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            Error::Permission(format!("cannot write {}", p.display()))
        } else {
            Error::Io(e)
        }
    };
    fs_err::write(&tmp, contents).map_err(|e| map_err(e, &tmp))?;
    if let Err(e) = fs_err::rename(&tmp, path) {
        let _ = fs_err::remove_file(&tmp);
        return Err(map_err(e, path));
    }
    Ok(())
}

/// Best-effort revert on every known backend, regardless of the currently
/// selected one. Called at daemon startup to clean up residue from:
///   - a previous crashed session (no graceful revert)
///   - a backend switch (e.g., `MONK_DNS_BACKEND` toggled between runs)
///   - an upgrade where the default selector changed
///
/// Each backend's `revert` MUST tolerate "never applied" and missing system
/// resources. Errors are logged and swallowed — a failing cleanup must not
/// block daemon startup.
pub fn cleanup_all() {
    fn try_revert<B: Blocker + Default>(label: &'static str) {
        let mut b = B::default();
        if let Err(e) = b.revert() {
            tracing::debug!(backend = label, ?e, "startup cleanup: revert failed (ignored)");
        } else {
            tracing::debug!(backend = label, "startup cleanup: reverted");
        }
    }
    try_revert::<hosts::HostsBlocker>("hosts");
    #[cfg(target_os = "macos")]
    try_revert::<resolver_dir::ResolverDirBlocker>("resolver_dir");
    #[cfg(target_os = "linux")]
    try_revert::<systemd_resolved::SystemdResolvedBlocker>("systemd_resolved");
}

#[cfg(test)]
pub(crate) fn assert_conformance<B: Blocker>(b: &mut B) {
    use super::BlockSet;
    let set_a = BlockSet { sites: vec!["example.com".into(), "foo.test".into()], apps: vec![] };
    let set_b = BlockSet { sites: vec!["example.com".into()], apps: vec![] };
    let empty = BlockSet::default();

    b.revert().expect("revert on clean instance should be ok");
    b.apply(&empty).expect("apply empty is a no-op");
    b.apply(&set_a).expect("apply A");
    b.apply(&set_a).expect("apply A twice must be idempotent");
    b.apply(&set_b).expect("shrink to B");
    b.revert().expect("revert after apply");
    b.revert().expect("revert twice must be idempotent");
}
