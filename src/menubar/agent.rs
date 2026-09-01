//! Login-item management for the menu bar app: a per-user LaunchAgent that
//! starts `monk menubar` at login (Aqua session only). The job points at the
//! executable inside `monk.app` rather than at the bare binary — see
//! [`super::bundle`] for why the identity matters.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{Error, Result};

pub const AGENT_LABEL: &str = super::bundle::BUNDLE_ID;
const AGENT_TEMPLATE: &str = include_str!("../../assets/launchd/dev.monk.menubar.plist");

pub fn agent_plist_path() -> Result<PathBuf> {
    let home = crate::paths::user_home()?;
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
    super::require_user_session()?;
    // Whatever is running now runs the *old* binary; leave it up and the new
    // bundle would lose the single-instance race and exit unnoticed.
    super::stop_running();
    let bin = super::bundle::install()?;
    register(&bin)
}

/// Writes the login item for an already-built bundle and asks launchd to
/// start it now. Split out so a refresh can rebuild the bundle without
/// re-deciding whether the user wants a login item at all.
pub fn register(bin: &std::path::Path) -> Result<bool> {
    let bin = bin.to_str().ok_or_else(|| Error::Other("non-utf8 binary path".into()))?;
    let plist = agent_plist_path()?;
    if let Some(dir) = plist.parent() {
        fs_err::create_dir_all(dir)?;
    }
    fs_err::write(&plist, AGENT_TEMPLATE.replace("__BIN__", bin))?;
    let uid = nix::unistd::getuid();
    // Boot out any stale registration first; ignore failures (not loaded).
    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}/{AGENT_LABEL}")]);
    // launchd unloads asynchronously, and bootstrapping a label it is still
    // tearing down fails with "service already loaded" — which would send us
    // down the direct-launch fallback for no reason.
    wait_until_unloaded(uid.as_raw());
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
    // Prefer the bundled copy: started from the bare binary, the app has no
    // identity to hang notifications off.
    let exe = match super::bundle::executable_path() {
        Ok(path) if path.exists() => path,
        _ => std::env::current_exe()?,
    };
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
    // launchctl only retires the job it started; an app launched by hand
    // would otherwise outlive the bundle it runs from.
    super::stop_running();
    uninstall()?;
    super::bundle::uninstall()
}

/// Polls until launchd forgets the label, or ~2s pass.
fn wait_until_unloaded(uid: u32) {
    let target = format!("gui/{uid}/{AGENT_LABEL}");
    for _ in 0..20 {
        match quiet_launchctl(&["print", &target]) {
            Ok(status) if status.success() => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            _ => return,
        }
    }
    tracing::debug!("launchd still reports the menu bar job after bootout");
}

fn quiet_launchctl(args: &[&str]) -> std::io::Result<std::process::ExitStatus> {
    Command::new("launchctl").args(args).stdout(Stdio::null()).stderr(Stdio::null()).status()
}
