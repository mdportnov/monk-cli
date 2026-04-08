use std::process::Command;

use crate::Result;

pub fn flush_dns() -> Result<()> {
    let _ = Command::new("dscacheutil").arg("-flushcache").status();
    let _ = Command::new("killall").arg("-HUP").arg("mDNSResponder").status();
    Ok(())
}
