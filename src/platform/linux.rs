use std::path::{Path, PathBuf};
use std::process::Command;

use super::unix_shared;
use crate::{Error, Result};

pub use unix_shared::{apply_sudo_user_home, chown_to_sudo_user, sudo_user_ids};

pub fn system_mode() -> bool {
    false
}

pub fn system_data_dir() -> Option<&'static str> {
    None
}

pub fn system_runtime_dir() -> Option<&'static str> {
    None
}

#[allow(dead_code)]
pub fn set_acl_current_user(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn migrate_legacy_if_needed() {}

pub fn elevate_install_service() -> Result<String> {
    let bin = std::env::current_exe()?.to_string_lossy().into_owned();
    install_service(&bin)
}

/// Best-effort native notification. Fire-and-forget: `notify-send` can block
/// indefinitely on a hung D-Bus, and a missed notification is far less bad
/// than freezing the CLI. We detach stdin/stdout/stderr and drop the child
/// handle without waiting; the kernel reaps it.
pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args([title, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn dirs_config() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().to_path_buf())
        .ok_or_else(|| Error::Other("cannot resolve config dir".into()))
}

pub fn install_service(bin: &str) -> Result<String> {
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

pub fn uninstall_service(purge: bool) -> Result<String> {
    let mut msgs = Vec::new();

    if super::try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    let _ = Command::new("systemctl").args(["--user", "disable", "--now", "monk"]).output();
    super::cleanup_hosts();

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
