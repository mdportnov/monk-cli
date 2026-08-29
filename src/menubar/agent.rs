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

/// Registers the login item and asks launchd to start it now
/// (`RunAtLoad`). Returns whether the bootstrap succeeded — when it did not,
/// the plist is still in place (the agent loads at next login) and the
/// caller may fall back to [`spawn_now`].
pub fn install() -> Result<bool> {
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
    Ok(status.success())
}

/// Starts `monk menubar` directly, detached from the calling terminal.
/// The single-instance lock makes this idempotent: a second copy exits
/// immediately, so callers may fire this without checking first.
pub fn spawn_now() -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    Command::new(exe)
        .arg("menubar")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
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

/// Full removal for CLI / reset flows: stops the launchd-managed instance
/// (if any) and deletes the plist. Not for use from inside the menu bar app
/// itself — the bootout would kill it mid-click.
pub fn uninstall_and_stop() -> Result<()> {
    let uid = nix::unistd::getuid();
    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}/{AGENT_LABEL}")]);
    uninstall()
}

fn quiet_launchctl(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    Command::new("launchctl").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()
}
