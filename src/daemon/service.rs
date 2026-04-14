#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "macos")]
use crate::paths;
use crate::{ipc, ipc::Request, Error, Result};


#[derive(Debug, Clone)]
pub enum ServiceAction {
    Install,
    Uninstall { purge: bool },
}

pub fn run(action: ServiceAction) -> Result<String> {
    let bin = std::env::current_exe()?.to_string_lossy().into_owned();
    match action {
        ServiceAction::Install => install(&bin),
        ServiceAction::Uninstall { purge } => uninstall(purge),
    }
}

#[cfg(target_os = "linux")]
fn install(bin: &str) -> Result<String> {
    let dir = dirs_config()?.join("systemd/user");
    fs_err::create_dir_all(&dir)?;
    let unit = dir.join("monk.service");
    let tpl = include_str!("../../assets/systemd/monk.service");
    fs_err::write(&unit, tpl.replace("__BIN__", bin))?;

    let mut msgs = vec![format!("wrote {}", unit.display())];

    let daemon_reload = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    match daemon_reload {
        Ok(output) => {
            if output.status.success() {
                tracing::debug!("systemctl --user daemon-reload: ok");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("systemctl --user daemon-reload failed: {}", stderr);
            }
        }
        Err(e) => {
            tracing::debug!(?e, "systemctl --user daemon-reload failed");
        }
    }

    let enable_start = Command::new("systemctl")
        .args(["--user", "enable", "--now", "monk"])
        .output();
    match enable_start {
        Ok(output) => {
            if output.status.success() {
                msgs.push("enabled and started monk.service".into());
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("systemctl --user enable --now monk failed: {}", stderr);
                msgs.push("manual start: `systemctl --user enable --now monk`".into());
            }
        }
        Err(e) => {
            tracing::debug!(?e, "systemctl --user enable --now monk failed");
            msgs.push("manual start: `systemctl --user enable --now monk`".into());
        }
    }

    Ok(msgs.join("\n"))
}

#[cfg(target_os = "linux")]
fn uninstall(purge: bool) -> Result<String> {
    let mut msgs = Vec::new();

    if try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    let _ = Command::new("systemctl").args(["--user", "disable", "--now", "monk"]).output();
    cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    let unit = dirs_config()?.join("systemd/user/monk.service");
    if unit.exists() {
        fs_err::remove_file(&unit)?;
        msgs.push(format!("removed {}", unit.display()));
    }

    if purge {
        if let Ok(data_dir) = crate::paths::data_dir() {
            let _ = fs_err::remove_dir_all(&data_dir);
            msgs.push("purged user data".into());
        }
        if let Ok(config_dir) = crate::paths::config_dir() {
            let _ = fs_err::remove_dir_all(&config_dir);
            msgs.push("purged config".into());
        }
    }

    let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).output();

    tracing::info!("service uninstalled");

    if msgs.is_empty() {
        msgs.push("uninstalled".into());
    }
    Ok(msgs.join(", "))
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
fn uninstall(purge: bool) -> Result<String> {
    let mut msgs = Vec::new();

    if try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    let home = dirs_home()?;
    let plist = home.join("Library/LaunchAgents/dev.monk.monkd.plist");
    if plist.exists() {
        let _ = std::process::Command::new("launchctl").args(["unload", "-w"]).arg(&plist).status();
        fs_err::remove_file(&plist)?;
        msgs.push(format!("removed {}", plist.display()));
    }

    cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    if purge {
        if let Ok(data_dir) = crate::paths::data_dir() {
            let _ = fs_err::remove_dir_all(&data_dir);
            msgs.push("purged user data".into());
        }
        if let Ok(config_dir) = crate::paths::config_dir() {
            let _ = fs_err::remove_dir_all(&config_dir);
            msgs.push("purged config".into());
        }
    }

    if msgs.is_empty() {
        msgs.push("uninstalled".into());
    }
    Ok(msgs.join(", "))
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
fn uninstall(purge: bool) -> Result<String> {
    let mut msgs = Vec::new();

    if try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    let _ = std::process::Command::new("schtasks").args(["/End", "/TN", "monkd"]).status();
    let status =
        std::process::Command::new("schtasks").args(["/Delete", "/F", "/TN", "monkd"]).status()?;
    if !status.success() {
        return Err(Error::Other("schtasks /Delete failed".into()));
    }
    msgs.push("removed scheduled task `monkd`".into());

    cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    if purge {
        if let Ok(data_dir) = crate::paths::data_dir() {
            let _ = fs_err::remove_dir_all(&data_dir);
            msgs.push("purged user data".into());
        }
        if let Ok(config_dir) = crate::paths::config_dir() {
            let _ = fs_err::remove_dir_all(&config_dir);
            msgs.push("purged config".into());
        }
    }

    Ok(msgs.join(", "))
}

fn try_shutdown_daemon() -> Result<()> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| Error::Other(e.to_string()))?;
    rt.block_on(async {
        match ipc::send(&Request::Shutdown).await {
            Ok(_) => Ok(()),
            Err(crate::Error::DaemonNotRunning) => Ok(()),
            Err(e) => Err(e),
        }
    })
}

fn cleanup_hosts() {
    use crate::blocker::{backends::BlockerBackend, Blocker};
    if let Ok(mut blocker) = crate::blocker::HostsBlocker::build() {
        if let Err(e) = blocker.revert() {
            tracing::warn!(?e, "hosts cleanup failed during uninstall");
        } else {
            tracing::info!("hosts cleaned up during uninstall");
        }
    }
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
