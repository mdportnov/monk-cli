use std::path::PathBuf;

use directories::ProjectDirs;
use once_cell::sync::Lazy;

use crate::{platform, Error, Result};

static DIRS: Lazy<Option<ProjectDirs>> = Lazy::new(|| {
    platform::apply_sudo_user_home();
    ProjectDirs::from("dev", "monk", "monk")
});

pub use platform::{migrate_legacy_if_needed, sudo_user_ids, system_mode};

fn dirs() -> Result<&'static ProjectDirs> {
    DIRS.as_ref().ok_or_else(|| Error::Other("could not resolve user directories".into()))
}

/// Try to create dir; swallow permission errors so CLI as user doesn't fail
/// when the daemon owns the path. Caller still gets the path back, and
/// downstream open/read calls produce a clear error.
fn ensure_dir_best_effort(p: &std::path::Path) {
    if let Err(e) = fs_err::create_dir_all(p) {
        if e.kind() != std::io::ErrorKind::AlreadyExists && !p.exists() {
            tracing::debug!(?e, path = %p.display(), "create_dir_all failed (continuing)");
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    if system_mode() {
        if let Some(sys) = platform::system_data_dir() {
            let p = PathBuf::from(sys);
            ensure_dir_best_effort(&p);
            return Ok(p);
        }
    }
    let p = dirs()?.config_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    platform::chown_to_sudo_user(&p);
    Ok(p)
}

pub fn data_dir() -> Result<PathBuf> {
    if system_mode() {
        if let Some(sys) = platform::system_data_dir() {
            let p = PathBuf::from(sys);
            ensure_dir_best_effort(&p);
            return Ok(p);
        }
    }
    let p = dirs()?.data_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    platform::chown_to_sudo_user(&p);
    Ok(p)
}

pub fn runtime_dir() -> Result<PathBuf> {
    if system_mode() {
        if let Some(sys) = platform::system_runtime_dir() {
            let p = PathBuf::from(sys);
            ensure_dir_best_effort(&p);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs_err::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
            }
            return Ok(p);
        }
    }
    let p = dirs()?
        .runtime_dir()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("monk"));
    fs_err::create_dir_all(&p)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
    }

    #[cfg(windows)]
    {
        if let Err(e) = platform::set_acl_current_user(&p) {
            tracing::warn!("Failed to set Windows ACL on runtime dir: {}", e);
        }
    }

    platform::chown_to_sudo_user(&p);
    Ok(p)
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn db_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("monk.sqlite3"))
}

pub fn log_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("monk.log"))
}

pub fn pid_file() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("monkd.pid"))
}

pub fn ipc_socket() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(PathBuf::from(r"\\.\pipe\monkd"))
    }
    #[cfg(not(windows))]
    {
        Ok(runtime_dir()?.join("monkd.sock"))
    }
}
