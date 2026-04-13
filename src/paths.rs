use std::path::PathBuf;

use directories::ProjectDirs;
use once_cell::sync::Lazy;

use crate::{Error, Result};

static DIRS: Lazy<Option<ProjectDirs>> = Lazy::new(|| {
    #[cfg(unix)]
    apply_sudo_user_home();
    ProjectDirs::from("dev", "monk", "monk")
});

#[cfg(unix)]
fn apply_sudo_user_home() {
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

#[cfg(unix)]
pub fn sudo_user_ids() -> Option<(u32, u32)> {
    let uid = std::env::var("SUDO_UID").ok()?.parse().ok()?;
    let gid = std::env::var("SUDO_GID").ok()?.parse().ok()?;
    Some((uid, gid))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn chown_to_sudo_user(path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let Some((uid, gid)) = sudo_user_ids() else { return };
    if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
        unsafe {
            let result = libc::chown(c.as_ptr(), uid, gid);
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

#[cfg(not(unix))]
fn chown_to_sudo_user(_: &std::path::Path) {}

fn dirs() -> Result<&'static ProjectDirs> {
    DIRS.as_ref().ok_or_else(|| Error::Other("could not resolve user directories".into()))
}

pub fn config_dir() -> Result<PathBuf> {
    let p = dirs()?.config_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    chown_to_sudo_user(&p);
    Ok(p)
}

pub fn data_dir() -> Result<PathBuf> {
    let p = dirs()?.data_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    chown_to_sudo_user(&p);
    Ok(p)
}

pub fn runtime_dir() -> Result<PathBuf> {
    let p = dirs()?
        .runtime_dir()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("monk"));
    fs_err::create_dir_all(&p)?;
    chown_to_sudo_user(&p);
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
