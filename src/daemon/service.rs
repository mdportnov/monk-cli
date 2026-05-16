#[cfg(target_os = "linux")]
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

    let daemon_reload = Command::new("systemctl").args(["--user", "daemon-reload"]).output();
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

    let enable_start =
        Command::new("systemctl").args(["--user", "enable", "--now", "monk"]).output();
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
const SYSTEM_PLIST: &str = "/Library/LaunchDaemons/dev.monk.monkd.plist";
#[cfg(target_os = "macos")]
const SYSTEM_BIN_DIR: &str = "/usr/local/libexec/monk";
#[cfg(target_os = "macos")]
const SYSTEM_BIN: &str = "/usr/local/libexec/monk/monkd";

#[cfg(target_os = "macos")]
fn require_root() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        Ok(())
    } else {
        Err(Error::Permission(
            "this command must run as root — try `sudo monk service install`".into(),
        ))
    }
}

#[cfg(target_os = "macos")]
fn install(bin: &str) -> Result<String> {
    require_root()?;

    let mut msgs = Vec::new();

    fs_err::create_dir_all(SYSTEM_BIN_DIR)?;
    let src = std::path::Path::new(bin);
    let dst = std::path::Path::new(SYSTEM_BIN);
    if src.canonicalize().ok().as_deref() != Some(dst) {
        fs_err::copy(src, dst)?;
        msgs.push(format!("copied binary → {}", dst.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(dst, std::fs::Permissions::from_mode(0o755));
    }

    let data_dir = std::path::Path::new("/Library/Application Support/monk");
    fs_err::create_dir_all(data_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(data_dir, std::fs::Permissions::from_mode(0o755));
    }
    let log = data_dir.join("monk.log");

    let tpl = include_str!("../../assets/launchd/dev.monk.monkd.daemon.plist");
    let rendered =
        tpl.replace("__BIN__", &dst.to_string_lossy()).replace("__LOG__", &log.to_string_lossy());
    fs_err::write(SYSTEM_PLIST, rendered)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(SYSTEM_PLIST, std::fs::Permissions::from_mode(0o644));
    }
    msgs.push(format!("wrote {SYSTEM_PLIST}"));

    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let legacy = std::path::PathBuf::from(format!(
                "/Users/{user}/Library/LaunchAgents/dev.monk.monkd.plist"
            ));
            if legacy.exists() {
                if let Some((uid, _)) = paths::sudo_user_ids() {
                    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}")], Some(&legacy));
                }
                let _ = fs_err::remove_file(&legacy);
                msgs.push(format!("removed legacy LaunchAgent {}", legacy.display()));
            }
        }
    }

    cleanup_hosts();
    msgs.push("reverted /etc/hosts (clears stale monk markers)".into());

    // bootout system on a not-yet-bootstrapped plist errors with code 5; we
    // expect that on first install — suppress the noise.
    let _ = quiet_launchctl(&["bootout", "system"], Some(std::path::Path::new(SYSTEM_PLIST)));
    let status = std::process::Command::new("launchctl")
        .args(["bootstrap", "system"])
        .arg(SYSTEM_PLIST)
        .status();
    match status {
        Ok(s) if s.success() => msgs.push("loaded via launchctl bootstrap system".into()),
        _ => msgs.push(format!("manual start: `sudo launchctl bootstrap system {SYSTEM_PLIST}`")),
    }

    Ok(msgs.join("\n"))
}

#[cfg(target_os = "macos")]
fn quiet_launchctl(
    args: &[&str],
    extra_path: Option<&std::path::Path>,
) -> std::io::Result<std::process::ExitStatus> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("launchctl");
    cmd.args(args);
    if let Some(p) = extra_path {
        cmd.arg(p);
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null()).status()
}

#[cfg(target_os = "macos")]
fn uninstall(purge: bool) -> Result<String> {
    require_root()?;

    let mut msgs = Vec::new();

    if try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    if std::path::Path::new(SYSTEM_PLIST).exists() {
        let _ = quiet_launchctl(&["bootout", "system"], Some(std::path::Path::new(SYSTEM_PLIST)));
        fs_err::remove_file(SYSTEM_PLIST)?;
        msgs.push(format!("removed {SYSTEM_PLIST}"));
    }

    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let legacy = std::path::PathBuf::from(format!(
                "/Users/{user}/Library/LaunchAgents/dev.monk.monkd.plist"
            ));
            if legacy.exists() {
                if let Some((uid, _)) = paths::sudo_user_ids() {
                    let _ = quiet_launchctl(&["bootout", &format!("gui/{uid}")], Some(&legacy));
                }
                let _ = fs_err::remove_file(&legacy);
                msgs.push(format!("removed legacy LaunchAgent {}", legacy.display()));
            }
        }
    }

    cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    if std::path::Path::new(SYSTEM_BIN).exists() {
        let _ = fs_err::remove_file(SYSTEM_BIN);
    }
    let _ = fs_err::remove_dir(SYSTEM_BIN_DIR);

    if purge {
        let sys_data = std::path::Path::new("/Library/Application Support/monk");
        if sys_data.exists() {
            let _ = fs_err::remove_dir_all(sys_data);
            msgs.push("purged system data".into());
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
            tracing::warn!(?e, "hosts revert failed");
        } else {
            tracing::info!("hosts reverted (cleared monk markers)");
        }
    }
}

#[cfg(target_os = "linux")]
fn dirs_config() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().to_path_buf())
        .ok_or_else(|| Error::Other("cannot resolve config dir".into()))
}
