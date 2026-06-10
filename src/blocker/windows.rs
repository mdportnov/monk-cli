use std::process::Command;

use tracing::{debug, warn};

use crate::Result;

/// Flush the Windows DNS Client resolver cache so a freshly written hosts block
/// takes effect immediately. Best-effort: a flush failure only delays
/// propagation and must never fail an already-applied block.
pub fn flush_dns() -> Result<()> {
    match Command::new("ipconfig").arg("/flushdns").status() {
        Ok(status) if status.success() => {
            debug!("flushed DNS cache via ipconfig /flushdns");
        }
        Ok(status) => warn!(?status, "ipconfig /flushdns exited non-zero"),
        Err(e) => warn!(?e, "failed to run ipconfig /flushdns"),
    }
    Ok(())
}
