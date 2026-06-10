use std::process::Command;

use tracing::{debug, warn};

use crate::Result;

/// Flush the system DNS resolver cache so a freshly written hosts block takes
/// effect immediately instead of after the current cache TTL expires.
///
/// Linux has no single resolver: `systemd-resolved`, `nscd` and others are all
/// common and mutually exclusive. We try each in turn and treat "tool not
/// installed" as a no-op. Best-effort by contract — the hosts block is already
/// in place when this runs, so a failed flush only delays propagation; it must
/// never fail the block.
pub fn flush_dns() -> Result<()> {
    let mut flushed = false;

    // systemd-resolved (modern) — `resolvectl`, falling back to the older
    // `systemd-resolve` binary name on distros that still ship it.
    if run_flush("resolvectl", &["flush-caches"]) {
        flushed = true;
    } else if run_flush("systemd-resolve", &["--flush-caches"]) {
        flushed = true;
    }

    // nscd, if running — independent of systemd-resolved.
    if run_flush("nscd", &["--invalidate=hosts"]) {
        flushed = true;
    }

    if !flushed {
        debug!("no DNS cache flush tool succeeded; relying on resolver TTL expiry");
    }
    Ok(())
}

fn run_flush(bin: &str, args: &[&str]) -> bool {
    match Command::new(bin).args(args).status() {
        Ok(status) if status.success() => {
            debug!(tool = bin, "flushed DNS cache");
            true
        }
        Ok(status) => {
            debug!(tool = bin, ?status, "DNS flush tool exited non-zero");
            false
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => {
            warn!(tool = bin, ?e, "failed to run DNS flush tool");
            false
        }
    }
}
