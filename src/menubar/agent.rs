//! Login-item management for the menu bar app: a per-user LaunchAgent that
//! starts `monk menubar` at login (Aqua session only).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{Error, Result};

pub const AGENT_LABEL: &str = "dev.monk.menubar";
const AGENT_TEMPLATE: &str = include_str!("../../assets/launchd/dev.monk.menubar.plist");

pub fn agent_plist_path() -> Result<PathBuf> {
    let home = directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .ok_or_else(|| Error::Other("cannot resolve home directory".into()))?;
    Ok(home.join("Library/LaunchAgents").join(format!("{AGENT_LABEL}.plist")))
}

pub fn installed() -> bool {
    agent_plist_path().map(|p| p.exists()).unwrap_or(false)
}

pub fn install() -> Result<()> {
    let bin = std::env::current_exe()?;
    let bin = bin.to_str().ok_or_else(|| Error::Other("non-utf8 binary path".into()))?;
    let plist = agent_plist_path()?;
    if let Some(dir) = plist.parent() {
        fs_err::create_dir_all(dir)?;
    }
    fs_err::write(&plist, AGENT_TEMPLATE.replace("__BIN__", bin))?;
    let uid = nix::unistd::getuid();
    // Boot out any stale registration first; ignore failures (not loaded).
    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}/{AGENT_LABEL}")]);
    let target = format!("gui/{uid}");
    let status = Command::new("launchctl")
        .args(["bootstrap", &target])
        .arg(&plist)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        tracing::warn!(?plist, "launchctl bootstrap failed; agent loads at next login");
    }
    Ok(())
}

/// Removes the plist only. Deliberately no `bootout`: when the running menu
/// bar app was itself started by this agent, booting the job out would kill
/// the app the user is clicking in. The loaded job simply won't return at
/// next login.
pub fn uninstall() -> Result<()> {
    let plist = agent_plist_path()?;
    if plist.exists() {
        fs_err::remove_file(&plist)?;
    }
    Ok(())
}

fn quiet_launchctl(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    Command::new("launchctl").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()
}
