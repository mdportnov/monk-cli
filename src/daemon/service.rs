#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use crate::paths;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy)]
pub enum ServiceAction {
    Install,
    Uninstall,
}

pub fn run(action: ServiceAction) -> Result<String> {
    let bin = std::env::current_exe()?.to_string_lossy().into_owned();
    match action {
        ServiceAction::Install => install(&bin),
        ServiceAction::Uninstall => uninstall(),
    }
}

#[cfg(target_os = "linux")]
fn install(bin: &str) -> Result<String> {
    let dir = dirs_config()?.join("systemd/user");
    fs_err::create_dir_all(&dir)?;
    let unit = dir.join("monk.service");
    let tpl = include_str!("../../assets/systemd/monk.service");
    fs_err::write(&unit, tpl.replace("__BIN__", bin))?;
    Ok(format!(
        "wrote {}\nnext: `systemctl --user daemon-reload && systemctl --user enable --now monk`",
        unit.display()
    ))
}

#[cfg(target_os = "linux")]
fn uninstall() -> Result<String> {
    let unit = dirs_config()?.join("systemd/user/monk.service");
    if unit.exists() {
        fs_err::remove_file(&unit)?;
    }
    Ok(format!("removed {}\nnext: `systemctl --user daemon-reload`", unit.display()))
}

#[cfg(target_os = "macos")]
fn install(bin: &str) -> Result<String> {
    let home = dirs_home()?;
    let dir = home.join("Library/LaunchAgents");
    fs_err::create_dir_all(&dir)?;
    let plist = dir.join("dev.monk.monkd.plist");
    let log = paths::log_file()?;
    let tpl = include_str!("../../assets/launchd/dev.monk.monkd.plist");
    let rendered = tpl.replace("__BIN__", bin).replace("__LOG__", &log.to_string_lossy());
    fs_err::write(&plist, rendered)?;
    Ok(format!("wrote {}\nnext: `launchctl load -w {}`", plist.display(), plist.display()))
}

#[cfg(target_os = "macos")]
fn uninstall() -> Result<String> {
    let home = dirs_home()?;
    let plist = home.join("Library/LaunchAgents/dev.monk.monkd.plist");
    if plist.exists() {
        let _ = std::process::Command::new("launchctl").args(["unload", "-w"]).arg(&plist).status();
        fs_err::remove_file(&plist)?;
    }
    Ok(format!("removed {}", plist.display()))
}

#[cfg(windows)]
fn install(bin: &str) -> Result<String> {
    let status = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/SC", "ONLOGON", "/RL", "HIGHEST", "/TN", "monkd", "/TR"])
        .arg(format!("\"{bin}\" daemon run"))
        .status()?;
    if !status.success() {
        return Err(Error::Other("schtasks /Create failed".into()));
    }
    Ok("installed scheduled task `monkd` (runs at logon, admin)".into())
}

#[cfg(windows)]
fn uninstall() -> Result<String> {
    let _ = std::process::Command::new("schtasks").args(["/End", "/TN", "monkd"]).status();
    let status =
        std::process::Command::new("schtasks").args(["/Delete", "/F", "/TN", "monkd"]).status()?;
    if !status.success() {
        return Err(Error::Other("schtasks /Delete failed".into()));
    }
    Ok("removed scheduled task `monkd`".into())
}

#[cfg(target_os = "macos")]
fn dirs_home() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .ok_or_else(|| Error::Other("cannot resolve home dir".into()))
}

#[cfg(target_os = "linux")]
fn dirs_config() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().to_path_buf())
        .ok_or_else(|| Error::Other("cannot resolve config dir".into()))
}
