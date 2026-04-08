use std::path::PathBuf;

use directories::ProjectDirs;
use once_cell::sync::Lazy;

use crate::{Error, Result};

static DIRS: Lazy<Option<ProjectDirs>> = Lazy::new(|| ProjectDirs::from("dev", "monk", "monk"));

fn dirs() -> Result<&'static ProjectDirs> {
    DIRS.as_ref().ok_or_else(|| Error::Other("could not resolve user directories".into()))
}

pub fn config_dir() -> Result<PathBuf> {
    let p = dirs()?.config_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    Ok(p)
}

pub fn data_dir() -> Result<PathBuf> {
    let p = dirs()?.data_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    Ok(p)
}

pub fn runtime_dir() -> Result<PathBuf> {
    let p = dirs()?
        .runtime_dir()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("monk"));
    fs_err::create_dir_all(&p)?;
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
