use std::process::Command;

use crate::Result;

pub fn flush_dns() -> Result<()> {
    let _ = Command::new("ipconfig").arg("/flushdns").status();
    Ok(())
}
