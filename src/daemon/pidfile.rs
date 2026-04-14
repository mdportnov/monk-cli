use std::{fs::File, path::{Path, PathBuf}};

use crate::{paths, Error, Result};

#[derive(Debug)]
pub struct PidFile {
    path: PathBuf,
    _file: Option<File>,
}

impl PidFile {
    pub fn new() -> Result<Self> {
        Ok(Self {
            path: paths::pid_file()?,
            _file: None,
        })
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self {
            path,
            _file: None,
        }
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

    pub fn acquire(&mut self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs_err::create_dir_all(parent)?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&self.path)?;

        match try_exclusive_lock(&file) {
            Ok(()) => {
                use std::io::{Seek, SeekFrom, Write};
                let mut file = file;
                file.seek(SeekFrom::Start(0))?;
                file.set_len(0)?;
                write!(file, "{}", std::process::id())?;
                file.flush()?;
                file.sync_all()?;
                self._file = Some(file);
                Ok(())
            }
            Err(LockError::WouldBlock) => {
                let existing_pid = self.read().unwrap_or(None);
                if let Some(pid) = existing_pid {
                    if pid_alive(pid) {
                        return Err(Error::DaemonAlreadyRunning(pid));
                    }
                }
                Err(Error::DaemonAlreadyRunning(existing_pid.unwrap_or(0)))
            }
            Err(LockError::Other(e)) => Err(Error::Io(e)),
        }
    }

    pub fn release(&self) {
        let _ = fs_err::remove_file(&self.path);
    }
}

#[derive(Debug)]
enum LockError {
    WouldBlock,
    Other(std::io::Error),
}

fn try_exclusive_lock(file: &File) -> std::result::Result<(), LockError> {
    use fs2::FileExt;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Err(LockError::WouldBlock),
        Err(e) => Err(LockError::Other(e)),
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    use nix::{sys::signal, unistd::Pid};
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

#[cfg(windows)]
#[allow(unsafe_code)]
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
