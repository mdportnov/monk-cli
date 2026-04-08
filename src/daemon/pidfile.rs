use std::path::{Path, PathBuf};

use crate::{paths, Error, Result};

#[derive(Debug)]
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn new() -> Result<Self> {
        Ok(Self { path: paths::pid_file()? })
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(&self) -> Result<Option<u32>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let raw = fs_err::read_to_string(&self.path)?;
        Ok(raw.trim().parse::<u32>().ok())
    }

    pub fn is_alive(&self) -> Result<Option<u32>> {
        let Some(pid) = self.read()? else { return Ok(None) };
        if pid_alive(pid) {
            Ok(Some(pid))
        } else {
            Ok(None)
        }
    }

    pub fn acquire(&self) -> Result<()> {
        if let Some(pid) = self.is_alive()? {
            return Err(Error::DaemonAlreadyRunning(pid));
        }
        if let Some(parent) = self.path.parent() {
            fs_err::create_dir_all(parent)?;
        }
        fs_err::write(&self.path, std::process::id().to_string())?;
        Ok(())
    }

    pub fn release(&self) {
        let _ = fs_err::remove_file(&self.path);
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    use nix::{sys::signal, unistd::Pid};
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}
