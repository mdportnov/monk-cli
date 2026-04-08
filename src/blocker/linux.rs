use std::process::Command;

use crate::Result;

pub fn flush_dns() -> Result<()> {
    let _ = Command::new("resolvectl").arg("flush-caches").status();
    Ok(())
}
