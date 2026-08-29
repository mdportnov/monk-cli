//! Platform abstraction for OS-specific code.
//!
//! Each backend (`macos`, `linux`, `windows`) provides the same public API:
//!
//! - `apply_sudo_user_home()` — adjust env when running under sudo (Unix only)
//! - `sudo_user_ids() -> Option<(u32, u32)>` — uid/gid of sudo invoker
//! - `chown_to_sudo_user(path)` — lchown a freshly-created file back to the user
//! - `set_acl_current_user(path) -> Result<()>` — Windows-only restrictive ACL
//! - `system_mode() -> bool` — true when a system-wide service is installed
//! - `system_data_dir() / system_runtime_dir()` — daemon paths in system mode
//! - `migrate_legacy_if_needed()` — one-shot per-user → system migration
//! - `install_service(bin) / uninstall_service(purge)` — service lifecycle
//! - `elevate_install_service() / elevate_uninstall_service(purge)` — same,
//!   asking the OS for admin rights first where the service is root-owned
//! - `restart_service()` — bounce the service in place; `None` where that
//!   makes no sense (macOS runs its own copy of the binary)

use std::path::PathBuf;

use crate::{ipc, ipc::Request, Error, Result};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

/// Unix-shared helpers used by both macOS and Linux backends.
#[cfg(unix)]
pub(crate) mod unix_shared {
    use std::path::Path;

    pub fn apply_sudo_user_home() {
        if std::env::var_os("SUDO_USER").is_none() {
            return;
        }
        let Ok(user) = std::env::var("SUDO_USER") else { return };
        if user == "root" {
            return;
        }
        let home = nix::unistd::User::from_name(&user)
            .ok()
            .flatten()
            .map(|u| u.dir.to_string_lossy().to_string())
            .filter(|s| !s.is_empty());
        if let Some(home) = home {
            std::env::set_var("HOME", &home);
            std::env::set_var("USER", &user);
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("XDG_DATA_HOME");
            std::env::remove_var("XDG_RUNTIME_DIR");
        }
    }

    pub fn sudo_user_ids() -> Option<(u32, u32)> {
        let uid = std::env::var("SUDO_UID").ok()?.parse().ok()?;
        let gid = std::env::var("SUDO_GID").ok()?.parse().ok()?;
        Some((uid, gid))
    }

    #[allow(unsafe_code)]
    pub fn chown_to_sudo_user(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        let Some((uid, gid)) = sudo_user_ids() else { return };
        if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
            unsafe {
                let result = libc::lchown(c.as_ptr(), uid, gid);
                if result != 0 {
                    tracing::warn!(
                        "chown failed for {}: errno {}",
                        path.display(),
                        std::io::Error::last_os_error()
                    );
                }
            }
        }
    }
}

/// Path to the legacy per-user config file for `user`. Returns `None` on
/// platforms where no legacy path exists.
#[allow(dead_code)]
pub fn legacy_user_config_file_for(user: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(macos::legacy_user_config_file(user))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = user;
        None
    }
}

/// Drop the machine-readable success markers from a service-install /
/// -uninstall message.
///
/// They are a handshake between the elevated child and its parent (an
/// osascript that exits 0 proves nothing about the child), never something a
/// user should read.
pub fn strip_service_markers(msg: &str) -> String {
    msg.lines()
        .map(|line| {
            line.split(", ")
                .filter(|part| !part.trim().starts_with("monk:service:"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Shared by every platform's service-install path: best-effort hosts revert
/// so a stale block doesn't outlive a reinstall.
pub(crate) fn cleanup_hosts() {
    use crate::blocker::{backends::BlockerBackend, Blocker};
    if let Ok(mut blocker) = crate::blocker::HostsBlocker::build() {
        if let Err(e) = blocker.revert() {
            tracing::warn!(?e, "hosts revert failed");
        } else {
            tracing::info!("hosts reverted (cleared monk markers)");
        }
    }
}

/// Spawn a private current-thread runtime to send a Shutdown request. Used
/// during `service uninstall`; safe to call from both sync and async contexts
/// because it dispatches to a fresh OS thread to avoid runtime nesting.
pub(crate) fn try_shutdown_daemon() -> Result<()> {
    std::thread::spawn(|| -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Other(e.to_string()))?;
        rt.block_on(async {
            match ipc::send(&Request::Shutdown).await {
                Ok(_) => Ok(()),
                Err(crate::Error::DaemonNotRunning) => Ok(()),
                Err(e) => Err(e),
            }
        })
    })
    .join()
    .map_err(|_| Error::Other("shutdown thread panicked".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_are_stripped_from_both_message_shapes() {
        // install joins with newlines, uninstall with ", "
        let install = "wrote /Library/LaunchDaemons/x.plist\nloaded\nmonk:service:install:ok";
        assert_eq!(strip_service_markers(install), "wrote /Library/LaunchDaemons/x.plist\nloaded");

        let uninstall = "removed /Library/LaunchDaemons/x.plist, monk:service:uninstall:ok";
        assert_eq!(strip_service_markers(uninstall), "removed /Library/LaunchDaemons/x.plist");
    }

    #[test]
    fn a_marker_only_message_collapses_to_empty() {
        assert_eq!(strip_service_markers("monk:service:install:ok"), "");
    }

    #[test]
    fn ordinary_text_survives_untouched() {
        let msg = "copied binary → /usr/local/libexec/monk/monkd";
        assert_eq!(strip_service_markers(msg), msg);
    }
}
